use std::collections::HashMap;
use std::io::{self, BufRead};
use std::time::Duration;

use bytes::{BufMut, BytesMut};
use rand::distributions::Alphanumeric;
use rand::Rng;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time;
use tokio_rustls::rustls::internal::pemfile::{certs, pkcs8_private_keys};
use tokio_rustls::rustls::{NoClientAuth, ServerConfig};

pub fn rand_id(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub fn timeout<T: core::future::Future>(secs: u64, future: T) -> time::Timeout<T> {
    time::timeout(Duration::from_secs(secs), future)
}

pub fn read_http_head(buf: &[u8]) -> Option<HashMap<String, String>> {
    let mut map = HashMap::<String, String>::new();
    for line in buf.lines() {
        let line = line.ok()?;
        if line.is_empty() {
            return Some(map);
        }
        let ss = line.split(": ").collect::<Vec<&str>>();
        if ss.len() == 2 {
            map.insert(ss[0].to_string().to_lowercase(), ss[1].to_string());
        }
    }
    None
}

pub async fn read_buf<R>(reader: &mut R) -> anyhow::Result<Option<BytesMut>>
where
    R: AsyncRead + Unpin,
{
    let mut buf = [0; 1024];
    let len = reader.read(&mut buf).await?;
    if len == 0 {
        return Ok(None);
    }
    let mut bs = BytesMut::new();
    bs.put(&buf[..len]);
    Ok(Some(bs))
}

pub async fn send_buf<W>(writer: &mut W, buf: &[u8]) -> anyhow::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn relay_data_and_shutdown<R, W>(so_timeout: u64, reader: &mut R, writer: &mut W) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if let Err(e) = relay_data(so_timeout, reader, writer).await {
        println!("{}", e);
    };
    writer.shutdown().await?;
    Ok(())
}

pub async fn relay_data<R, W>(so_timeout: u64, reader: &mut R, writer: &mut W) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    const BUFFER_SIZE: usize = 16 * 1024;
    let mut buf = [0; BUFFER_SIZE];
    loop {
        let len = timeout(so_timeout, reader.read(&mut buf)).await??;
        if len == 0 {
            return Ok(());
        }
        timeout(so_timeout, writer.write_all(&buf[..len])).await??;
    }
}

pub fn read_ssl_config(crt: &mut dyn BufRead, key: &mut dyn BufRead) -> anyhow::Result<ServerConfig> {
    let certs = certs(crt).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))?;
    let mut keys = pkcs8_private_keys(key).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))?;

    let mut config = ServerConfig::new(NoClientAuth::new());
    config.set_single_cert(certs, keys.remove(0))?;

    Ok(config)
}
