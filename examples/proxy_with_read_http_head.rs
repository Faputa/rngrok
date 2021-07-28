use bytes::BufMut;
use rngrok::util;
use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listen_addr = String::from("127.0.0.1:80");
    let server_addr = String::from("127.0.0.1:9527");

    println!("Listening on: {}", listen_addr);
    println!("Proxying to: {}", server_addr);

    let listener = TcpListener::bind(listen_addr).await?;

    while let Ok((inbound, _)) = listener.accept().await {
        let transfer = transfer(inbound, server_addr.clone());
        tokio::spawn(async {
            if let Err(e) = transfer.await {
                println!("Failed to transfer; error={:?}", e);
            }
        });
    }

    Ok(())
}

async fn transfer(mut socket: TcpStream, proxy_addr: String) -> anyhow::Result<()> {
    let mut buf = match util::read_buf(&mut socket).await.unwrap() {
        Some(b) => b,
        None => return Ok(()),
    };
    loop {
        let head = match util::read_http_head(&buf) {
            Some(h) => h,
            None => {
                let bs = match util::read_buf(&mut socket).await.unwrap() {
                    Some(b) => b,
                    None => return Ok(()),
                };
                buf.put(bs);
                continue;
            }
        };
        println!("{:#?}", head);

        let mut local = TcpStream::connect(proxy_addr).await?;

        let (mut request_reader, mut request_writer) = socket.split();
        let (mut local_reader, mut local_writer) = local.split();

        let client_to_server = async {
            local_writer.write(&buf).await?;
            local_writer.flush().await?;
            io::copy(&mut request_reader, &mut local_writer).await?;
            local_writer.shutdown().await
        };

        let server_to_client = async {
            io::copy(&mut local_reader, &mut request_writer).await?;
            request_writer.shutdown().await
        };

        tokio::try_join!(client_to_server, server_to_client)?;

        return Ok(());
    }
}
