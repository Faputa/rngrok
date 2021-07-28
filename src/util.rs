use std::{
    collections::HashMap,
    io::{self, BufRead},
    time::Duration,
};

use bytes::{BufMut, BytesMut};
use rand::{distributions::Alphanumeric, Rng};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    time,
};
use tokio_rustls::rustls::{
    internal::pemfile::{certs, pkcs8_private_keys},
    NoClientAuth, ServerConfig,
};

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

pub fn read_ssl_config(
    crt: &mut dyn BufRead,
    key: &mut dyn BufRead,
) -> anyhow::Result<ServerConfig> {
    let certs =
        certs(crt).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))?;
    let mut keys = pkcs8_private_keys(key)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))?;

    let mut config = ServerConfig::new(NoClientAuth::new());
    config.set_single_cert(certs, keys.remove(0))?;

    Ok(config)
}
