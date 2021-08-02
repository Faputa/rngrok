use std::collections::HashMap;
use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::broadcast;

use crate::client::proxy::ProxyConnect;
use crate::client::{Tunnel, TunnelConfig};
use crate::msg::{Auth, AuthResp, Envelope, Message, Ping, ReqTunnel};
use crate::pack::{send_pack, PacketReader};
use crate::unwrap_or;
use crate::util::{rand_id, timeout};

use super::Context;

pub struct ControlConnect {
    ctx: Arc<Context>,
}

impl ControlConnect {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { ctx }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let addr = format!("{}:{}", self.ctx.server_host, self.ctx.server_port);
        let mut stream = TcpStream::connect(addr).await?;
        let (mut reader, mut writer) = stream.split();
        let mut reader = PacketReader::new(&mut reader);

        send_pack(&mut writer, auth(self.ctx.auth_token.clone())).await?;
        let json = unwrap_or!(timeout(self.ctx.so_timeout, reader.read()).await??, return Ok(()));
        println!("{}", json);

        let msg = serde_json::from_str::<Envelope>(&json)?;
        let auth_resp = serde_json::from_value::<AuthResp>(msg.payload)?;

        if let Some(err) = auth_resp.error {
            println!("Failed to authenticate to server: {}", &err);
            return Ok(());
        }

        let id = auth_resp.client_id.unwrap();
        println!("Authenticated with server, client id: {}", &id);

        let mut req_id_to_tunnel_config = HashMap::<String, &TunnelConfig>::new();
        for tunnel in &self.ctx.tunnel_list {
            let req_id = rand_id(8);
            send_pack(&mut writer, req_tunnel(tunnel.clone(), req_id.clone())).await?;
            req_id_to_tunnel_config.insert(req_id, &tunnel);
        }

        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        loop {
            let json = match timeout(self.ctx.ping_time, reader.read()).await {
                Ok(Ok(s)) => unwrap_or!(s, return Ok(())),
                Ok(Err(e)) => Err(e)?,
                Err(_) => {
                    send_pack(&mut writer, ping()).await?;
                    continue;
                }
            };
            println!("{}", json);

            match Message::from_str(&json)? {
                Message::NewTunnel(new_tunnel) => {
                    if let Some(err) = new_tunnel.error {
                        println!("Server failed to allocate tunnel: {}", &err);
                        return Ok(());
                    }
                    let &tunnel = unwrap_or!(req_id_to_tunnel_config.get(&new_tunnel.req_id.unwrap()), return Ok(()));
                    self.ctx.tunnel_map.lock().unwrap().insert(
                        new_tunnel.url.clone().unwrap(),
                        Arc::new(Tunnel {
                            public_url: new_tunnel.url.clone().unwrap(),
                            protocol: new_tunnel.protocol.unwrap(),
                            local_addr: format!("{}:{}", tunnel.local_host, tunnel.local_port),
                        }),
                    );
                    println!("Tunnel established at {}", new_tunnel.url.unwrap());
                }
                Message::ReqProxy(_) => {
                    tokio::spawn(connect_proxy(
                        ProxyConnect::new(self.ctx.clone(), id.clone()),
                        notify_shutdown.subscribe(),
                    ));
                }
                Message::Pong(_) => {}
                _ => {}
            }
        }
    }
}

async fn connect_proxy(proxy_connect: ProxyConnect, mut shutdown: broadcast::Receiver<()>) {
    tokio::select! {
        res = proxy_connect.run() => {
            if let Err(e) = res {
                println!("{:?}", e);
            }
        }
        _ = shutdown.recv() => {}
    }
}

fn auth(auth_token: String) -> String {
    serde_json::to_string(&Envelope::from(Auth {
        version: "2".to_string(),
        mm_version: "1.7".to_string(),
        user: auth_token,
        password: "".to_string(),
        os: "darwin".to_string(),
        arch: "amd64".to_string(),
        client_id: "".to_string(),
    }))
    .unwrap()
}

fn req_tunnel(tunnel: TunnelConfig, req_id: String) -> String {
    serde_json::to_string(&Envelope::from(ReqTunnel {
        req_id,
        protocol: tunnel.protocol,
        hostname: tunnel.hostname,
        subdomain: tunnel.subdomain,
        http_auth: "".to_string(),
        remote_port: tunnel.remote_port,
    }))
    .unwrap()
}

fn ping() -> String {
    serde_json::to_string(&Envelope::from(Ping {})).unwrap()
}
