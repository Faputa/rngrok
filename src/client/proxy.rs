use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;
use tokio::sync::broadcast;

use crate::client::MyTcpStream;
use crate::msg::{Envelope, RegProxy, StartProxy};
use crate::pack::{send_pack, PacketReader};
use crate::unwrap_or;
use crate::util::{forward, send_buf, timeout};

use super::{Context, TcpWriter, Tunnel};

pub struct ProxyConnect {
    ctx: Arc<Context>,
    id: String,
}

impl ProxyConnect {
    pub fn new(ctx: Arc<Context>, id: String) -> Self {
        Self { ctx, id }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let stream = match MyTcpStream::connect(&self.ctx.server_host, self.ctx.server_port, self.ctx.use_ssl).await {
            Ok(t) => t,
            Err(e) => {
                log::error!("Failed to establish proxy connection: {}", e);
                return Ok(());
            }
        };
        let (mut reader, mut writer) = stream.into_split();

        if let Err(e) = send_pack(&mut writer, reg_proxy(self.id.clone())).await {
            log::error!("Failed to write RegProxy: {}", e);
        }

        let mut packet_reader = PacketReader::new(&mut reader);
        let json = unwrap_or!(timeout(self.ctx.so_timeout, packet_reader.read()).await??, return Ok(()));
        log::info!("{}", json);

        let msg = serde_json::from_str::<Envelope>(&json)?;
        let start_proxy = match serde_json::from_value::<StartProxy>(msg.payload) {
            Ok(m) => m,
            Err(e) => {
                log::error!("Server failed to write StartProxy: {}", e);
                return Ok(());
            }
        };

        let tunnel = unwrap_or!(self.ctx.tunnel_map.lock().unwrap().get(&start_proxy.url), {
            log::warn!("Couldn't find tunnel for proxy: {}", start_proxy.url);
            return Ok(());
        })
        .clone();

        let local_stream = match TcpStream::connect(&tunnel.local_addr).await {
            Ok(local_stream) => local_stream,
            Err(e) => {
                log::error!("Failed to open private leg {}: {}", tunnel.local_addr, &e);
                if tunnel.protocol.starts_with("http") {
                    send_buf(&mut writer, bad_gateway(&tunnel).as_bytes()).await?;
                    writer.shutdown().await?;
                }
                return Ok(());
            }
        };

        let (local_reader, mut local_writer) = local_stream.into_split();
        let (_notify_shutdown, shutdown) = broadcast::channel::<()>(1);
        tokio::spawn(run_local(LocalConnect::new(self.ctx.clone(), local_reader, writer), shutdown));

        send_buf(&mut local_writer, &packet_reader.get_buf()).await?;
        forward(self.ctx.so_timeout, &mut reader, &mut local_writer).await?;
        local_writer.shutdown().await?;

        Ok(())
    }
}

async fn run_local(mut local_connect: LocalConnect, mut shutdown: broadcast::Receiver<()>) {
    tokio::select! {
        res = local_connect.run() => {
            if let Err(e) = res {
                log::error!("{}", e);
            }
        }
        _ = shutdown.recv() => {}
    }
}

struct LocalConnect {
    ctx: Arc<Context>,
    local_reader: OwnedReadHalf,
    remote_writer: TcpWriter,
}

impl LocalConnect {
    fn new(ctx: Arc<Context>, local_reader: OwnedReadHalf, remote_writer: TcpWriter) -> Self {
        Self {
            ctx,
            local_reader,
            remote_writer,
        }
    }

    async fn run(&mut self) -> anyhow::Result<()> {
        forward(self.ctx.so_timeout, &mut self.local_reader, &mut self.remote_writer).await?;
        self.remote_writer.shutdown().await?;
        Ok(())
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
