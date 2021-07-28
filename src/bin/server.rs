use std::{env, fs};

use rngrok::server::{Config, Server};

const DEFAULT_FILENAME: &str = "server.yml";

#[tokio::main]
async fn main() {
    let cfg = match env::args().nth(1) {
        Some(filename) => match fs::read_to_string(&filename) {
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
        },
        None => match fs::read_to_string(DEFAULT_FILENAME) {
            Ok(s) => match serde_yaml::from_str::<Config>(&s) {
                Ok(c) => c,
                Err(e) => {
                    println!(
                        "Error parsing configuration file {}: {}",
                        DEFAULT_FILENAME, e
                    );
                    return;
                }
            },
            Err(_) => Config::default(),
        },
    };
    println!("{:?}", cfg);

    let mut server = Server::new(
        cfg.domain,
        cfg.port,
        cfg.http_port,
        cfg.https_port,
        cfg.ssl_crt,
        cfg.ssl_key,
    );
    server.run().await;
}
