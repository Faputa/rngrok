mod http;
mod tcp;
mod tunnel;

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use tokio::io::{split, ReadHalf, WriteHalf};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::server::TlsStream;
use tokio_util::either::Either;

use crate::util::read_ssl_config;

use self::http::{HttpListener, HttpsListener};
use self::tunnel::TunnelListener;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub domain: String,
    pub port: u16,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub ssl_crt: Option<String>,
    pub ssl_key: Option<String>,
    pub so_timeout: Option<u64>,
    pub ping_timeout: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            domain: "ngrok.com".to_string(),
            port: 4443,
            http_port: Some(80),
            https_port: Some(443),
            ssl_crt: None,
            ssl_key: None,
            so_timeout: None,
            ping_timeout: None,
        }
    }
}

type TcpReader = Either<OwnedReadHalf, ReadHalf<TlsStream<TcpStream>>>;
type TcpWriter = Either<OwnedWriteHalf, WriteHalf<TlsStream<TcpStream>>>;

struct MyTcpStream {
    inner: Either<TcpStream, TlsStream<TcpStream>>,
}

impl MyTcpStream {
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

#[derive(Debug)]
pub struct Request {
    pub url: String,
    pub proxy_writer_sender: mpsc::Sender<TcpWriter>,
    pub request_writer: TcpWriter,
}

impl Request {
    pub fn new(url: String, proxy_writer_sender: mpsc::Sender<TcpWriter>, request_writer: TcpWriter) -> Self {
        Self {
            url,
            proxy_writer_sender,
            request_writer,
        }
    }
}

pub struct Client {
    pub id: String,
    pub writer: Mutex<TcpWriter>,
    pub request_sender: mpsc::Sender<Request>,
    pub request_receiver: Mutex<mpsc::Receiver<Request>>,
}

impl Client {
    pub fn new(writer: TcpWriter, id: String) -> Self {
        let (request_sender, request_receiver) = mpsc::channel(1);
        Self {
            id,
            writer: Mutex::new(writer),
            request_sender,
            request_receiver: Mutex::new(request_receiver),
        }
    }
}

pub struct Context {
    pub domain: String,
    pub port: u16,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub ssl_crt: Option<String>,
    pub ssl_key: Option<String>,
    pub so_timeout: u64,
    pub ping_timeout: u64,
    pub client_map: RwLock<HashMap<String, Arc<Client>>>,
    pub tunnel_map: RwLock<HashMap<String, Arc<Client>>>,
}

impl Context {
    pub fn ssl_config(&self) -> anyhow::Result<ServerConfig> {
        match (&self.ssl_crt, &self.ssl_key) {
            (Some(ssl_crt), Some(ssl_key)) => {
                let crt = &mut BufReader::new(File::open(ssl_crt)?);
                let key = &mut BufReader::new(File::open(ssl_key)?);
                read_ssl_config(crt, key)
            }
            _ => {
                let crt = include_bytes!("../../server.crt");
                let key = include_bytes!("../../server.key");
                read_ssl_config(&mut &crt[..], &mut &key[..])
            }
        }
    }
}

pub struct Server {
    ctx: Arc<Context>,
}

impl Server {
    pub fn new(cfg: Config) -> Self {
        let ctx = Arc::new(Context {
            domain: cfg.domain,
            port: cfg.port,
            http_port: cfg.http_port,
            https_port: cfg.https_port,
            ssl_crt: cfg.ssl_crt,
            ssl_key: cfg.ssl_key,
            so_timeout: cfg.so_timeout.unwrap_or(28800),
            ping_timeout: cfg.ping_timeout.unwrap_or(120),
            client_map: RwLock::new(HashMap::new()),
            tunnel_map: RwLock::new(HashMap::new()),
        });
        Self { ctx }
    }

    pub async fn run(&self) {
        if let Some(port) = self.ctx.http_port {
            let http_listener = HttpListener::new(self.ctx.clone());
            tokio::spawn(async move { http_listener.run(port).await });
        }

        if let Some(port) = self.ctx.https_port {
            let https_listener = HttpsListener::new(self.ctx.clone());
            tokio::spawn(async move { https_listener.run(port).await });
        }

        let tunnel_listener = TunnelListener::new(self.ctx.clone());
        if let Err(e) = tunnel_listener.run().await {
            println!("tunnelListener failed with error {}", e);
        }
    }
}
