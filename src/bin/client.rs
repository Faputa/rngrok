use std::{env, fs};

use rngrok::client::{Client, Config};

const DEFAULT_FILENAME: &str = "client.yml";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let filename = env::args().nth(1).unwrap_or(DEFAULT_FILENAME.to_string());
    let s = fs::read_to_string(filename.clone())
        .map_err(|e| anyhow::anyhow!("Failed to read configuration file {}: {}", filename, e))?;
    let cfg = serde_yaml::from_str::<Config>(&s)
        .map_err(|e| anyhow::anyhow!("Error parsing configuration file {}: {}", filename, e))?;
    log::info!("{:?}", cfg);

    let client = Client::new(cfg);
    client.run().await;
    Ok(())
}
