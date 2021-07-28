mod control;
mod proxy;

use control::ControlConnect;
use std::time::Duration;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::time;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub server_host: String,
    pub server_port: u16,
    pub tunnel_list: Vec<TunnelConfig>,
    pub so_timeout: Option<u64>,
    pub ping_time: Option<u64>,
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
}

impl Context {
    pub fn new(
        server_host: String,
        server_port: u16,
        tunnel_list: Vec<TunnelConfig>,
        so_timeout: Option<u64>,
        ping_time: Option<u64>,
    ) -> Self {
        Self {
            server_host,
            server_port,
            tunnel_list,
            tunnel_map: Mutex::new(HashMap::new()),
            so_timeout: so_timeout.unwrap_or(28800),
            ping_time: ping_time.unwrap_or(10),
        }
    }
}

pub struct Client {
    ctx: Arc<Context>,
}

impl Client {
    pub fn new(
        server_host: String,
        server_port: u16,
        tunnel_list: Vec<TunnelConfig>,
        so_timeout: Option<u64>,
        ping_time: Option<u64>,
    ) -> Self {
        let ctx = Arc::new(Context::new(
            server_host,
            server_port,
            tunnel_list,
            so_timeout,
            ping_time,
        ));
        Self { ctx }
    }

    pub async fn run(&self) {
        loop {
            let connect = ControlConnect::new(self.ctx.clone());
            if let Ok(_) = connect.run().await {
                return;
            };
            time::sleep(Duration::from_millis(1000)).await;
        }
    }
}
