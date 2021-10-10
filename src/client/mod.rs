mod control;
mod proxy;

use control::ControlConnect;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{split, ReadHalf, WriteHalf};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::time;
use tokio_native_tls::native_tls::TlsConnector;
use tokio_native_tls::TlsStream;
use tokio_util::either::Either;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub server_host: String,
    pub server_port: u16,
    pub tunnel_list: Vec<TunnelConfig>,
    pub so_timeout: Option<u64>,
    pub ping_time: Option<u64>,
    pub auth_token: String,
    pub use_ssl: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TunnelConfig {
    pub protocol: String,
    pub hostname: Option<String>,
    pub subdomain: Option<String>,
    pub remote_port: Option<u16>,
    pub local_host: String,
    pub local_port: u16,
}

pub struct Tunnel {
    pub public_url: String,
    pub protocol: String,
    pub local_addr: String,
}

pub struct Context {
    pub server_host: String,
    pub server_port: u16,
    pub tunnel_list: Vec<TunnelConfig>,
    pub tunnel_map: Mutex<HashMap<String, Arc<Tunnel>>>,
    pub so_timeout: u64,
    pub ping_time: u64,
    pub auth_token: String,
    pub use_ssl: bool,
}

type TcpReader = Either<OwnedReadHalf, ReadHalf<TlsStream<TcpStream>>>;
type TcpWriter = Either<OwnedWriteHalf, WriteHalf<TlsStream<TcpStream>>>;

struct MyTcpStream {
    inner: Either<TcpStream, TlsStream<TcpStream>>,
}

impl MyTcpStream {
    pub async fn connect(server_host: &str, server_port: u16, use_ssl: bool) -> anyhow::Result<MyTcpStream> {
        let addr = format!("{}:{}", server_host, server_port);
        let stream = TcpStream::connect(addr).await?;
        if use_ssl {
            let cx = TlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
                .use_sni(false)
                .build()?;
            let cx = tokio_native_tls::TlsConnector::from(cx);
            let stream = cx.connect(server_host, stream).await?;
            Ok(MyTcpStream::from(stream))
        } else {
            Ok(MyTcpStream::from(stream))
        }
    }

    pub fn into_split(self) -> (TcpReader, TcpWriter) {
        match self.inner {
            Either::Left(stream) => {
                let (reader, writer) = stream.into_split();
                (TcpReader::Left(reader), TcpWriter::Left(writer))
            }
            Either::Right(stream) => {
                let (reader, writer) = split(stream);
                (TcpReader::Right(reader), TcpWriter::Right(writer))
            }
        }
    }
}

impl From<TcpStream> for MyTcpStream {
    fn from(stream: TcpStream) -> Self {
        Self {
            inner: Either::Left(stream),
        }
    }
}

impl From<TlsStream<TcpStream>> for MyTcpStream {
    fn from(stream: TlsStream<TcpStream>) -> Self {
        Self {
            inner: Either::Right(stream),
        }
    }
}

pub struct Client {
    ctx: Arc<Context>,
}

impl Client {
    pub fn new(cfg: Config) -> Self {
        let ctx = Arc::new(Context {
            server_host: cfg.server_host,
            server_port: cfg.server_port,
            tunnel_list: cfg.tunnel_list,
            auth_token: cfg.auth_token,
            so_timeout: cfg.so_timeout.unwrap_or(28800),
            ping_time: cfg.ping_time.unwrap_or(10),
            use_ssl: cfg.use_ssl.unwrap_or(false),
            tunnel_map: Mutex::new(HashMap::new()),
        });
        Self { ctx }
    }

    pub async fn run(&self) {
        let control_connect = ControlConnect::new(self.ctx.clone());
        while let Err(e) = control_connect.run().await {
            println!("{}", e);
            time::sleep(Duration::from_millis(1000)).await;
        }
    }
}
