use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_rustls::TlsAcceptor;

use crate::msg::{AuthResp, Envelope, Message, NewTunnel, Pong, RegProxy, ReqProxy, ReqTunnel, StartProxy};
use crate::pack::{send_pack, PacketReader};
use crate::server::tcp::MyTcpListener;
use crate::server::MyTcpStream;
use crate::unwrap_or;
use crate::util::{forward, rand_id, timeout};

use super::{Client, Context, TcpReader, TcpWriter};

pub struct TunnelListener {
    ctx: Arc<Context>,
}

impl TunnelListener {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { ctx }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let acceptor = if self.ctx.use_ssl {
            let config = self.ctx.ssl_config().unwrap();
            let acceptor = TlsAcceptor::from(Arc::new(config));
            Some(acceptor)
        } else {
            None
        };

        let addr = format!("0.0.0.0:{}", self.ctx.port);
        let listener = TcpListener::bind(&addr).await?;
        println!("Listening for control and proxy connections on {}", addr);

        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let ctx = self.ctx.clone();
            let shutdown = notify_shutdown.subscribe();
            tokio::spawn(async move {
                let stream = if let Some(acceptor) = acceptor {
                    let stream = acceptor.accept(stream).await.unwrap();
                    MyTcpStream::from(stream)
                } else {
                    MyTcpStream::from(stream)
                };
                serve(TunnelHandler::new(ctx), stream, shutdown).await;
            });
        }

        Ok(())
    }
}

async fn serve(mut tunnel_handler: TunnelHandler, stream: MyTcpStream, mut shutdown: broadcast::Receiver<()>) {
    tokio::select! {
        res = tunnel_handler.run(stream) => {
            if let Err(e) = res {
                println!("{}", e);
            }
        }
        _ = shutdown.recv() => {}
    }
}

struct TunnelHandler {
    id: Option<String>,
    ctx: Arc<Context>,
}

impl TunnelHandler {
    fn new(ctx: Arc<Context>) -> Self {
        Self { id: None, ctx }
    }

    async fn run(&mut self, stream: MyTcpStream) -> anyhow::Result<()> {
        let (mut reader, writer) = stream.into_split();
        let mut packet_reader = PacketReader::new(&mut reader);

        let json = unwrap_or!(timeout(self.ctx.so_timeout, packet_reader.read()).await??, return Ok(()));
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

    async fn new_control(&mut self, mut reader: PacketReader<'_, TcpReader>, writer: TcpWriter) -> anyhow::Result<()> {
        let id = rand_id(16);
        self.id = Some(id.clone());
        let client = Arc::new(Client::new(writer, id.clone()));
        self.ctx.client_map.write().unwrap().insert(id.clone(), client.clone());

        send_pack(&mut *client.writer.lock().await, auth_resp(id.clone())).await?;
        send_pack(&mut *client.writer.lock().await, req_proxy()).await?;

        let (notify_shutdown, _) = broadcast::channel::<()>(1);
        loop {
            let json = unwrap_or!(timeout(self.ctx.ping_timeout, reader.read()).await??, return Ok(()));
            println!("{}", json);

            match Message::from_str(&json)? {
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
        mut reader: TcpReader,
        mut writer: TcpWriter,
    ) -> anyhow::Result<()> {
        let id = reg_proxy.client_id;
        let client = unwrap_or!(self.ctx.client_map.read().unwrap().get(&id), {
            println!("No client found for identifier: {}", id);
            return Ok(());
        })
        .clone();

        let request_receiver = &client.request_receiver;
        let recv_request = async move { request_receiver.lock().await.recv().await };
        let mut request = match timeout(60, recv_request).await {
            Ok(req) => unwrap_or!(req, return Ok(())),
            Err(_) => {
                send_pack(&mut *client.writer.lock().await, req_proxy()).await?;
                return Ok(());
            }
        };

        send_pack(&mut writer, start_proxy(request.url, request.public_addr.to_string())).await?;
        request.proxy_writer_sender.send(writer).await?;
        send_pack(&mut *client.writer.lock().await, req_proxy()).await?;

        forward(self.ctx.so_timeout, &mut reader, &mut request.public_writer).await?;
        request.public_writer.shutdown().await?;

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
                let addr = format!("0.0.0.0:{}", port);
                let listener = match TcpListener::bind(&addr).await {
                    Ok(t) => t,
                    Err(e) => {
                        println!("Error binding TCP listener: {}", e);
                        return Ok(());
                    }
                };

                let url = format!("tcp://{}:{}", self.ctx.domain, port);

                tokio::spawn(listen_tcp(MyTcpListener::new(listener, self.ctx.clone(), url.clone()), shutdown));

                self.ctx.tunnel_map.write().unwrap().insert(url.clone(), client.clone());

                send_pack(&mut *client.writer.lock().await, new_tunnel(req_tunnel.req_id, url, protocol.to_string()))
                    .await
            }

            protocol @ ("http" | "https") => {
                let url = if let Some(hostname) = req_tunnel.hostname {
                    format!("{}://{}", protocol, hostname)
                } else if let Some(subdomain) = req_tunnel.subdomain {
                    format!("{}://{}.{}", protocol, subdomain, self.ctx.domain)
                } else {
                    format!("{}://{}.{}", protocol, rand_id(6), self.ctx.domain)
                };

                self.ctx.tunnel_map.write().unwrap().insert(url.clone(), client.clone());

                send_pack(&mut *client.writer.lock().await, new_tunnel(req_tunnel.req_id, url, protocol.to_string()))
                    .await
            }

            _ => Ok(()),
        }
    }
}

async fn listen_tcp(tcp_listener: MyTcpListener, mut shutdown: broadcast::Receiver<()>) {
    tokio::select! {
        _ = tcp_listener.run() => {}
        _ = shutdown.recv() => {}
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

fn start_proxy(url: String, client_addr: String) -> String {
    serde_json::to_string(&Envelope::from(StartProxy { url, client_addr })).unwrap()
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
