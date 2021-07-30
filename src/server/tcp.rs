use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};

use crate::util::{relay_data, timeout};

use super::{Context, Request, TcpWriter};

pub struct MyTcpListener {
    listener: TcpListener,
    ctx: Arc<Context>,
    url: String,
}

impl MyTcpListener {
    pub fn new(listener: TcpListener, ctx: Arc<Context>, url: String) -> Self {
        Self { listener, ctx, url }
    }

    pub async fn run(&self) {
        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        while let Ok((stream, _)) = self.listener.accept().await {
            tokio::spawn(serve(
                TcpHandler::new(self.ctx.clone(), self.url.clone()),
                stream,
                notify_shutdown.subscribe(),
            ));
        }
    }
}

impl Drop for MyTcpListener {
    fn drop(&mut self) {
        self.ctx.tunnel_map.write().unwrap().remove(&self.url);
    }
}

async fn serve(tcp_handler: TcpHandler, stream: TcpStream, mut shutdown: broadcast::Receiver<()>) {
    tokio::select! {
        res = tcp_handler.run(stream) => {
            if let Err(e) = res {
                println!("{}", e);
            }
        }
        _ = shutdown.recv() => {}
    }
}

struct TcpHandler {
    ctx: Arc<Context>,
    url: String,
}

impl TcpHandler {
    fn new(ctx: Arc<Context>, url: String) -> Self {
        Self { ctx, url }
    }

    async fn run(&self, stream: TcpStream) -> anyhow::Result<()> {
        let (mut reader, writer) = stream.into_split();
        let (proxy_writer_sender, mut proxy_writer_receiver) = mpsc::channel(1);

        let request = Request::new(self.url.clone(), proxy_writer_sender, TcpWriter::Left(writer));

        let client = match self.ctx.tunnel_map.read().unwrap().get(&self.url) {
            Some(s) => s.clone(),
            None => return Ok(()),
        };
        client.request_sender.send(request).await?;

        let mut proxy_writer = timeout(60, proxy_writer_receiver.recv())
            .await?
            .ok_or(anyhow::anyhow!("No proxy_writer found"))?;

        relay_data(self.ctx.so_timeout, &mut reader, &mut proxy_writer).await
    }
}
