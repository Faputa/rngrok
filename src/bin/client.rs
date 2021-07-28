use std::{env, fs};

use rngrok::client::{Client, Config};

const DEFAULT_FILENAME: &str = "client.yml";

#[tokio::main]
async fn main() {
    let filename = env::args().nth(1).unwrap_or(DEFAULT_FILENAME.to_string());
    let cfg = match fs::read_to_string(filename.clone()) {
        Ok(s) => match serde_yaml::from_str::<Config>(&s) {
            Ok(c) => c,
            Err(e) => {
                println!("Error parsing configuration file {}: {}", filename, e);
                return;
            }
        },
        Err(e) => {
            println!("Failed to read configuration file {}: {}", filename, e);
            return;
        }
    };
    println!("{:?}", cfg);

    let client = Client::new(
        cfg.server_host,
        cfg.server_port,
        cfg.tunnel_list,
        cfg.so_timeout,
        cfg.ping_time,
    );
    client.run().await;
}
