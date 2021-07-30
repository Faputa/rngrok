use std::{env, fs};

use rngrok::client::{Client, Config};

const DEFAULT_FILENAME: &str = "client.yml";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filename = env::args().nth(1).unwrap_or(DEFAULT_FILENAME.to_string());
    let s = fs::read_to_string(filename.clone())
        .map_err(|e| anyhow::anyhow!("Failed to read configuration file {}: {}", filename, e))?;
    let cfg = serde_yaml::from_str::<Config>(&s)
        .map_err(|e| anyhow::anyhow!("Error parsing configuration file {}: {}", filename, e))?;
    println!("{:?}", cfg);

    let client = Client::new(cfg.server_host, cfg.server_port, cfg.tunnel_list, cfg.so_timeout, cfg.ping_time);
    Ok(client.run().await)
}
