use std::sync::Arc;

use tokio::{
    io::AsyncWriteExt,
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream,
    },
    sync::broadcast,
};

use crate::{
    msg::{
        AuthResp, Envelope, Message, NewTunnel, Pong, RegProxy, ReqProxy, ReqTunnel, StartProxy,
    },
    pack::{send_pack, PacketReader},
    server::tcp::MyTcpListener,
    util,
};

use super::{Client, Context};

pub struct TunnelListener {
    ctx: Arc<Context>,
}

impl TunnelListener {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { ctx }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let addr = format!("127.0.0.1:{}", self.ctx.port);
        let listener = TcpListener::bind(&addr).await?;
        println!("Listening for control and proxy connections on {}", addr);

        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        while let Ok((stream, _)) = listener.accept().await {
            let handler = TunnelHandler::new(self.ctx.clone());
            tokio::spawn(handler.run_select(stream, notify_shutdown.subscribe()));
        }

        Ok(())
    }
}

struct TunnelHandler {
    id: Option<String>,
    ctx: Arc<Context>,
}

impl TunnelHandler {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { id: None, ctx }
    }

    pub async fn run_select(self, stream: TcpStream, mut shutdown: broadcast::Receiver<()>) {
        tokio::select! {
            res = self.run(stream) => {
                if let Err(e) = res {
                    println!("{}", e);
                }
            }
            _ = shutdown.recv() => {}
        }
    }

    async fn run(mut self, stream: TcpStream) -> anyhow::Result<()> {
        let (mut reader, writer) = stream.into_split();
        let mut packet_reader = PacketReader::new(&mut reader);

        let json = match packet_reader.read().await? {
            Some(s) => s,
            None => return Ok(()),
        };
        println!("{}", json);

        let msg = match Message::from_str(&json) {
            Ok(m) => m,
            Err(e) => {
                println!("Failed to read message: {}", e);
                return Ok(());
            }
        };

        match msg {
            Message::Auth(_) => self.new_control(packet_reader, writer).await,
            Message::RegProxy(reg_proxy) => self.new_proxy(reg_proxy, reader, writer).await,
            _ => Ok(()),
        }
    }

    async fn new_control(
        &mut self,
        mut reader: PacketReader<'_, OwnedReadHalf>,
        writer: OwnedWriteHalf,
    ) -> anyhow::Result<()> {
        let id = util::rand_id(16);
        self.id = Some(id.clone());
        let client = Arc::new(Client::new(writer, id.clone()));
        self.ctx
            .client_map
            .write()
            .unwrap()
            .insert(id.clone(), client.clone());

        send_pack(&mut *client.writer.lock().await, auth_resp(id.clone())).await?;
        send_pack(&mut *client.writer.lock().await, req_proxy()).await?;

        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        loop {
            let json = match reader.read().await? {
                Some(s) => s,
                None => return Ok(()),
            };
            println!("{}", json);

            let msg = Message::from_str(&json).unwrap();

            match msg {
                Message::ReqTunnel(req_tunnel) => {
                    self.register_tunnel(client.clone(), req_tunnel, notify_shutdown.subscribe())
                        .await?
                }
                Message::Ping(_) => send_pack(&mut *client.writer.lock().await, pong()).await?,
                _ => {}
            }
        }
    }

    async fn new_proxy(
        &mut self,
        reg_proxy: RegProxy,
        mut reader: OwnedReadHalf,
        mut writer: OwnedWriteHalf,
    ) -> anyhow::Result<()> {
        let id = reg_proxy.client_id;
        let client = match self.ctx.client_map.read().unwrap().get(&id) {
            Some(c) => c.clone(),
            None => {
                println!("No client found for identifier: {}", id);
                return Ok(());
            }
        };

        let request_receiver = &client.request_receiver;
        let recv_request = async move { request_receiver.lock().await.recv().await };
        let mut request = match util::timeout(60, recv_request).await {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(()),
            Err(_) => {
                send_pack(&mut *client.writer.lock().await, req_proxy()).await?;
                return Ok(());
            }
        };

        send_pack(&mut writer, start_proxy(request.url)).await?;
        request.proxy_writer_sender.send(writer).await?;
        send_pack(&mut *client.writer.lock().await, req_proxy()).await?;

        let _ = tokio::io::copy(&mut reader, &mut request.request_writer).await;
        let _ = request.request_writer.shutdown().await;

        Ok(())
    }

    async fn register_tunnel(
        &mut self,
        client: Arc<Client>,
        req_tunnel: ReqTunnel,
        shutdown: broadcast::Receiver<()>,
    ) -> anyhow::Result<()> {
        match req_tunnel.protocol.as_str() {
            protocol @ "tcp" => {
                let port = req_tunnel.remote_port.unwrap();
                let addr = format!("127.0.0.1:{}", port);
                let listener = match TcpListener::bind(&addr).await {
                    Ok(t) => t,
                    Err(e) => {
                        println!("Error binding TCP listener: {}", e);
                        return Ok(());
                    }
                };

                let url = format!("tcp://{}:{}", self.ctx.domain, port);
                let tcp_listener = MyTcpListener::new(listener, self.ctx.clone(), url.clone());
                tokio::spawn(tcp_listener.run_select(shutdown));

                self.ctx
                    .tunnel_map
                    .write()
                    .unwrap()
                    .insert(url.clone(), client.clone());

                send_pack(
                    &mut *client.writer.lock().await,
                    new_tunnel(req_tunnel.req_id, url, protocol.to_string()),
                )
                .await
            }

            protocol @ ("http" | "https") => {
                let url = if let Some(hostname) = req_tunnel.hostname {
                    format!("{}://{}", protocol, hostname)
                } else if let Some(subdomain) = req_tunnel.subdomain {
                    format!("{}://{}.{}", protocol, subdomain, self.ctx.domain)
                } else {
                    format!("{}://{}.{}", protocol, util::rand_id(6), self.ctx.domain)
                };

                self.ctx
                    .tunnel_map
                    .write()
                    .unwrap()
                    .insert(url.clone(), client.clone());

                send_pack(
                    &mut *client.writer.lock().await,
                    new_tunnel(req_tunnel.req_id, url, protocol.to_string()),
                )
                .await
            }

            _ => Ok(()),
        }
    }
}

impl Drop for TunnelHandler {
    fn drop(&mut self) {
        if let Some(id) = self.id.clone() {
            self.ctx.client_map.write().unwrap().remove(&id);

            let urls = self
                .ctx
                .tunnel_map
                .read()
                .unwrap()
                .iter()
                .filter(|(_, v)| v.id == id)
                .map(|(k, _)| k.clone())
                .collect::<Vec<_>>();
            for url in urls {
                self.ctx.tunnel_map.write().unwrap().remove(&url);
            }
        }
    }
}

fn auth_resp(client_id: String) -> String {
    serde_json::to_string(&Envelope::from(AuthResp {
        version: Some("2".to_string()),
        mm_version: Some("1.7".to_string()),
        client_id: Some(client_id),
        error: None,
    }))
    .unwrap()
}

fn req_proxy() -> String {
    serde_json::to_string(&Envelope::from(ReqProxy {})).unwrap()
}

fn start_proxy(url: String) -> String {
    serde_json::to_string(&Envelope::from(StartProxy {
        url,
        client_addr: "".to_string(),
    }))
    .unwrap()
}

fn new_tunnel(req_id: String, url: String, protocol: String) -> String {
    serde_json::to_string(&Envelope::from(NewTunnel {
        req_id: Some(req_id),
        url: Some(url),
        protocol: Some(protocol),
        error: None,
    }))
    .unwrap()
}

fn pong() -> String {
    serde_json::to_string(&Envelope::from(Pong {})).unwrap()
}
