use bytes::BufMut;
use rngrok::pack::PacketReader;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Bind a server socket
    let listener = TcpListener::bind("0.0.0.0:17653").await?;

    println!("listening on {:?}", listener.local_addr());

    loop {
        let (mut socket, _) = listener.accept().await?;

        tokio::spawn(async move {
            let mut reader = PacketReader::new(&mut socket);
            while let Some(msg) = reader.read().await? {
                println!("{}", msg);
            }
            let mut buf = reader.get_buf();
            let mut bs = [0; 1024];
            let len = socket.read(&mut bs).await?;
            buf.put(&bs[..len]);
            println!("{:?}", buf);
            Ok::<(), anyhow::Error>(())
        });
    }
}
