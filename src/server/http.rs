use std::net::SocketAddr;
use std::sync::Arc;

use bytes::BufMut;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_rustls::TlsAcceptor;

use crate::server::Request;
use crate::unwrap_or;
use crate::util::{forward, read_buf, read_http_head, send_buf, timeout};

use super::{Context, MyTcpStream};

pub struct HttpListener {
    ctx: Arc<Context>,
}

impl HttpListener {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { ctx }
    }

    pub async fn run(&self, port: u16) -> anyhow::Result<()> {
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr).await?;
        log::info!("Listening for public http connections on {}", addr);

        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        while let Ok((stream, _)) = listener.accept().await {
            let peer_addr = stream.peer_addr().unwrap();
            let stream = MyTcpStream::from(stream);
            tokio::spawn(serve(
                HttpHandler::new(self.ctx.clone(), "http", peer_addr),
                stream,
                notify_shutdown.subscribe(),
            ));
        }

        Ok(())
    }
}

pub struct HttpsListener {
    ctx: Arc<Context>,
}

impl HttpsListener {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { ctx }
    }

    pub async fn run(&self, port: u16) -> anyhow::Result<()> {
        let config = self.ctx.ssl_config()?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr).await?;
        log::info!("Listening for public https connections on {}", addr);

        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let ctx = self.ctx.clone();
            let shutdown = notify_shutdown.subscribe();
            tokio::spawn(async move {
                let peer_addr = stream.peer_addr().unwrap();
                let stream = acceptor.accept(stream).await.unwrap();
                let stream = MyTcpStream::from(stream);
                serve(HttpHandler::new(ctx, "https", peer_addr), stream, shutdown).await;
            });
        }

        Ok(())
    }
}

async fn serve(http_handler: HttpHandler<'_>, stream: MyTcpStream, mut shutdown: broadcast::Receiver<()>) {
    tokio::select! {
        res = http_handler.run(stream) => {
            if let Err(e) = res {
                log::error!("{}", e);
            }
        }
        _ = shutdown.recv() => {}
    }
}

struct HttpHandler<'a> {
    ctx: Arc<Context>,
    protocol: &'a str,
    peer_addr: SocketAddr,
}

impl<'a> HttpHandler<'a> {
    fn new(ctx: Arc<Context>, protocol: &'a str, peer_addr: SocketAddr) -> Self {
        Self {
            ctx,
            protocol,
            peer_addr,
        }
    }

    async fn run(&self, stream: MyTcpStream) -> anyhow::Result<()> {
        let (mut reader, writer) = stream.into_split();
        let mut buf = unwrap_or!(timeout(self.ctx.so_timeout, read_buf(&mut reader)).await??, return Ok(()));
        loop {
            let head = unwrap_or!(read_http_head(&buf), {
                let bs = unwrap_or!(timeout(self.ctx.so_timeout, read_buf(&mut reader)).await??, return Ok(()));
                buf.put(bs);
                continue;
            });

            let url = format!("{}://{}", self.protocol, head.get("host").unwrap());
            let (proxy_writer_sender, mut proxy_writer_receiver) = mpsc::channel(1);
            let request = Request::new(url.clone(), proxy_writer_sender, writer, self.peer_addr);

            let client = unwrap_or!(self.ctx.tunnel_map.read().unwrap().get(&url), return Ok(())).clone();
            client.request_sender.send(request).await?;

            let mut proxy_writer = timeout(60, proxy_writer_receiver.recv())
                .await?
                .ok_or(anyhow::anyhow!("No proxy_writer found"))?;

            send_buf(&mut proxy_writer, &buf).await?;
            forward(self.ctx.so_timeout, &mut reader, &mut proxy_writer).await?;
            proxy_writer.shutdown().await?;

            return Ok(());
        }
    }
}
