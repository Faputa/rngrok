use std::sync::Arc;

use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::broadcast;

use crate::msg::{Envelope, RegProxy, StartProxy};
use crate::pack::{send_pack, PacketReader};
use crate::util::{relay_data_and_shutdown, send_buf, timeout};

use super::{Context, Tunnel};

pub struct ProxyConnect {
    ctx: Arc<Context>,
    id: String,
}

impl ProxyConnect {
    pub fn new(ctx: Arc<Context>, id: String) -> Self {
        Self { ctx, id }
    }

    pub async fn run_select(self, mut shutdown: broadcast::Receiver<()>) {
        tokio::select! {
            res = self.run() => {
                if let Err(e) = res {
                    println!("{:?}", e);
                }
            }
            _ = shutdown.recv() => {}
        }
    }

    async fn run(&self) -> anyhow::Result<()> {
        let addr = format!("{}:{}", self.ctx.server_host, self.ctx.server_port);
        let mut stream = match TcpStream::connect(addr).await {
            Ok(t) => t,
            Err(e) => {
                println!("Failed to establish proxy connection: {}", e);
                return Ok(());
            }
        };

        if let Err(e) = send_pack(&mut stream, reg_proxy(self.id.clone())).await {
            println!("Failed to write RegProxy: {}", e);
        }

        let mut packet_reader = PacketReader::new(&mut stream);
        let json = match timeout(self.ctx.so_timeout, packet_reader.read()).await?? {
            Some(s) => s,
            None => return Ok(()),
        };
        println!("{}", json);

        let msg = serde_json::from_str::<Envelope>(&json).unwrap();
        let start_proxy = match serde_json::from_value::<StartProxy>(msg.payload) {
            Ok(m) => m,
            Err(e) => {
                println!("Server failed to write StartProxy: {}", e);
                return Ok(());
            }
        };

        let tunnel = match self.ctx.tunnel_map.lock().unwrap().get(&start_proxy.url) {
            Some(tunnel) => tunnel.clone(),
            None => {
                println!("Couldn't find tunnel for proxy: {}", start_proxy.url);
                return Ok(());
            }
        };

        let local_stream = match TcpStream::connect(&tunnel.local_addr).await {
            Ok(local_stream) => local_stream,
            Err(e) => {
                println!("Failed to open private leg {}: {}", tunnel.local_addr, &e);
                if tunnel.protocol.starts_with("http") {
                    send_buf(&mut stream, bad_gateway(&tunnel).as_bytes()).await?;
                }
                return Ok(());
            }
        };

        let (local_reader, mut local_writer) = local_stream.into_split();
        send_buf(&mut local_writer, &packet_reader.get_buf()).await?;
        let (mut reader, writer) = stream.into_split();

        let (_notify_shutdown, shutdown) = broadcast::channel::<()>(1);
        tokio::spawn(LocalConnect::new(self.ctx.clone(), local_reader, writer).run_select(shutdown));

        relay_data_and_shutdown(self.ctx.so_timeout, &mut reader, &mut local_writer).await
    }
}

struct LocalConnect {
    ctx: Arc<Context>,
    local_reader: OwnedReadHalf,
    remote_writer: OwnedWriteHalf,
}

impl LocalConnect {
    fn new(ctx: Arc<Context>, local_reader: OwnedReadHalf, remote_writer: OwnedWriteHalf) -> Self {
        Self {
            ctx,
            local_reader,
            remote_writer,
        }
    }

    async fn run_select(mut self, mut shutdown: broadcast::Receiver<()>) {
        tokio::select! {
            res = self.run() => {
                if let Err(e) = res {
                    println!("{}", e);
                }
            }
            _ = shutdown.recv() => {}
        }
    }

    async fn run(&mut self) -> anyhow::Result<()> {
        relay_data_and_shutdown(self.ctx.so_timeout, &mut self.local_reader, &mut self.remote_writer).await
    }
}

fn reg_proxy(client_id: String) -> String {
    serde_json::to_string(&Envelope::from(RegProxy { client_id })).unwrap()
}

fn bad_gateway(tunnel: &Tunnel) -> String {
    let body = format!(
        r#"<html>
<body style="background-color: #97a8b9">
<div style="margin:auto; width:400px;padding: 20px 60px; background-color: #D3D3D3; border: 5px solid maroon;">
<h2>Tunnel {} unavailable</h2>
<p>Unable to initiate connection to <strong>{}</strong>. A web server must be running on port <strong>{}</strong> to complete the tunnel.</p>"#,
        tunnel.public_url, tunnel.local_addr, tunnel.local_addr
    );
    let resp = format!(
        r#"HTTP/1.0 502 Bad Gateway
Content-Type: text/html
Content-Length: {}

{}"#,
        body.len(),
        &body
    );
    resp
}
