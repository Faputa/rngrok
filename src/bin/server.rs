use std::{env, fs};

use rngrok::server::{Config, Server};

const DEFAULT_FILENAME: &str = "server.yml";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cfg = match env::args().nth(1) {
        Some(filename) => {
            let s = fs::read_to_string(&filename)
                .map_err(|e| anyhow::anyhow!("Failed to read configuration file {}: {}", filename, e))?;
            serde_yaml::from_str::<Config>(&s)
                .map_err(|e| anyhow::anyhow!("Error parsing configuration file {}: {}", filename, e))?
        }
        None => match fs::read_to_string(DEFAULT_FILENAME) {
            Ok(s) => serde_yaml::from_str::<Config>(&s)
                .map_err(|e| anyhow::anyhow!("Error parsing configuration file {}: {}", DEFAULT_FILENAME, e))?,
            Err(_) => Default::default(),
        },
    };
    log::info!("{:?}", cfg);

    let server = Server::new(cfg);
    server.run().await;
    Ok(())
}
