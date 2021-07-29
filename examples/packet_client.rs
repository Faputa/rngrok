use std::time::Duration;

use bytes::Bytes;
use rngrok::pack::send_pack;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Bind a server socket
    let mut socket = TcpStream::connect("0.0.0.0:17653").await?;

    send_pack(&mut socket, String::from("Hello,")).await?;
    sleep(Duration::from_millis(10)).await;
    send_pack(&mut socket, String::from("world!")).await?;
    sleep(Duration::from_millis(10)).await;
    socket.write(&Bytes::from("abcdefg")).await?;

    Ok(())
}
