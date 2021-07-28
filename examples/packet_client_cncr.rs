use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use rngrok::pack::send_pack;
use tokio::{io::AsyncWriteExt, net::TcpStream, sync::Mutex, time::sleep};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Bind a server socket
    let socket = TcpStream::connect("127.0.0.1:17653").await?;
    let (_, writer) = socket.into_split();
    let writer = Arc::new(Mutex::new(writer));

    let writer1 = writer.clone();
    tokio::spawn(async move {
        send_pack(&mut *writer1.lock().await, String::from("Hello,")).await?;
        Ok::<(), anyhow::Error>(())
    });

    let writer2 = writer.clone();
    tokio::spawn(async move {
        send_pack(&mut *writer2.lock().await, String::from("world!")).await?;
        Ok::<(), anyhow::Error>(())
    });

    sleep(Duration::from_millis(10)).await;
    writer.lock().await.write(&Bytes::from("abcdefg")).await?;

    Ok(())
}
