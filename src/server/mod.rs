mod http;
mod tcp;
mod tunnel;

use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{split, ReadHalf, WriteHalf},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::{mpsc, Mutex},
};
use tokio_rustls::{rustls::ServerConfig, server::TlsStream};
use tokio_util::either::Either;

use crate::util;

use self::{
    http::{HttpListener, HttpsListener},
    tunnel::TunnelListener,
};

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub domain: String,
    pub port: u16,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub ssl_crt: Option<String>,
    pub ssl_key: Option<String>,
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
    pub proxy_writer_sender: mpsc::Sender<OwnedWriteHalf>,
    pub request_writer: TcpWriter,
}

impl Request {
    pub fn new(
        url: String,
        proxy_writer_sender: mpsc::Sender<OwnedWriteHalf>,
        request_writer: TcpWriter,
    ) -> Self {
        Self {
            url,
            proxy_writer_sender,
            request_writer,
        }
    }
}

pub struct Client {
    pub id: String,
    pub writer: Mutex<OwnedWriteHalf>,
    pub request_sender: mpsc::Sender<Request>,
    pub request_receiver: Mutex<mpsc::Receiver<Request>>,
}

impl Client {
    pub fn new(writer: OwnedWriteHalf, id: String) -> Self {
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
    pub client_map: RwLock<HashMap<String, Arc<Client>>>,
    pub tunnel_map: RwLock<HashMap<String, Arc<Client>>>,
}

impl Context {
    pub fn new(
        domain: String,
        port: u16,
        http_port: Option<u16>,
        https_port: Option<u16>,
        ssl_crt: Option<String>,
        ssl_key: Option<String>,
    ) -> Self {
        Self {
            domain,
            port,
            http_port,
            https_port,
            ssl_crt,
            ssl_key,
            client_map: RwLock::new(HashMap::new()),
            tunnel_map: RwLock::new(HashMap::new()),
        }
    }

    pub fn ssl_config(&self) -> anyhow::Result<ServerConfig> {
        match (&self.ssl_crt, &self.ssl_key) {
            (Some(ssl_crt), Some(ssl_key)) => {
                let crt = &mut BufReader::new(File::open(ssl_crt)?);
                let key = &mut BufReader::new(File::open(ssl_key)?);
                util::read_ssl_config(crt, key)
            }
            _ => {
                let crt = include_bytes!("../../server.crt");
                let key = include_bytes!("../../server.key");
                util::read_ssl_config(&mut &crt[..], &mut &key[..])
            }
        }
    }
}

pub struct Server {
    ctx: Arc<Context>,
}

impl Server {
    pub fn new(
        domain: String,
        port: u16,
        http_port: Option<u16>,
        https_port: Option<u16>,
        ssl_crt: Option<String>,
        ssl_key: Option<String>,
    ) -> Self {
        let ctx = Arc::new(Context::new(
            domain, port, http_port, https_port, ssl_crt, ssl_key,
        ));
        Self { ctx }
    }

    pub async fn run(&mut self) {
        if let Some(port) = self.ctx.http_port {
            let http_listener = HttpListener::new(self.ctx.clone());
            tokio::spawn(http_listener.run(port));
        }

        if let Some(port) = self.ctx.https_port {
            let https_listener = HttpsListener::new(self.ctx.clone());
            tokio::spawn(https_listener.run(port));
        }

        let tunnel_listener = TunnelListener::new(self.ctx.clone());
        if let Err(e) = tunnel_listener.run().await {
            println!("tunnelListener failed with error {}", e);
        }
    }
}
