use std::sync::Arc;

use bytes::BufMut;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_rustls::TlsAcceptor;

use crate::server::Request;
use crate::util::{read_buf, read_http_head, send_buf, timeout};

use super::{Context, MyTcpStream};

pub struct HttpListener {
    ctx: Arc<Context>,
}

impl HttpListener {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { ctx }
    }

    pub async fn run(self, port: u16) {
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr).await.unwrap();
        println!("Listening for public http connections on {}", addr);

        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        while let Ok((stream, _)) = listener.accept().await {
            let stream = MyTcpStream::from(stream);
            tokio::spawn(serve_select(
                stream,
                self.ctx.clone(),
                "http",
                notify_shutdown.subscribe(),
            ));
        }
    }
}

pub struct HttpsListener {
    ctx: Arc<Context>,
}

impl HttpsListener {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { ctx }
    }

    pub async fn run(self, port: u16) {
        let config = self.ctx.ssl_config().unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr).await.unwrap();
        println!("Listening for public https connections on {}", addr);

        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let ctx = self.ctx.clone();
            let shutdown = notify_shutdown.subscribe();
            tokio::spawn(async move {
                let stream = acceptor.accept(stream).await.unwrap();
                let stream = MyTcpStream::from(stream);
                serve_select(stream, ctx, "https", shutdown).await;
            });
        }
    }
}

async fn serve_select(
    stream: MyTcpStream,
    ctx: Arc<Context>,
    protocol: &str,
    mut shutdown: broadcast::Receiver<()>,
) {
    tokio::select! {
        res = serve(stream, ctx, protocol) => {
            if let Err(e) = res {
                println!("{}", e);
            }
        }
        _ = shutdown.recv() => {}
    }
}

async fn serve(stream: MyTcpStream, ctx: Arc<Context>, protocol: &str) -> anyhow::Result<()> {
    let (mut reader, writer) = stream.into_split();
    let mut buf = match read_buf(&mut reader).await? {
        Some(b) => b,
        None => return Ok(()),
    };
    loop {
        let head = match read_http_head(&buf) {
            Some(h) => h,
            None => {
                let bs = match read_buf(&mut reader).await? {
                    Some(b) => b,
                    None => return Ok(()),
                };
                buf.put(bs);
                continue;
            }
        };

        let url = format!("{}://{}", protocol, head.get("host").unwrap());
        let (proxy_writer_sender, mut proxy_writer_receiver) = mpsc::channel(1);
        let request = Request::new(url.clone(), proxy_writer_sender, writer);

        let client = match ctx.tunnel_map.read().unwrap().get(&url) {
            Some(s) => s.clone(),
            None => return Ok(()),
        };
        client.request_sender.send(request).await?;

        let mut proxy_writer = timeout(60, proxy_writer_receiver.recv())
            .await?
            .ok_or(anyhow::anyhow!("No proxy_writer found"))?;

        send_buf(&mut proxy_writer, &buf).await?;
        let _ = io::copy(&mut reader, &mut proxy_writer).await;
        let _ = proxy_writer.shutdown().await;

        return Ok(());
    }
}
