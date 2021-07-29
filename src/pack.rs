use std::io::Cursor;
use std::mem;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub struct PacketReader<'a, R: AsyncRead + Unpin> {
    reader: &'a mut R,
    buf: BytesMut,
}

impl<'a, R: AsyncRead + Unpin> PacketReader<'a, R> {
    pub fn new(reader: &'a mut R) -> Self {
        Self {
            reader,
            buf: BytesMut::new(),
        }
    }

    pub async fn read(&mut self) -> anyhow::Result<Option<String>> {
        const HEAD_LEN: usize = mem::size_of::<u64>();
        loop {
            if self.buf.len() >= HEAD_LEN {
                let size = Cursor::new(&mut *self.buf).get_u64_le() as usize;
                if self.buf.len() >= size + HEAD_LEN {
                    self.buf.advance(HEAD_LEN);
                    let buf = self.buf.split_to(size);
                    return Ok(Some(String::from_utf8(buf.to_vec())?));
                }
            }
            let mut buf = [0; 1024];
            let len = self.reader.read(&mut buf).await?;
            if len == 0 {
                return Ok(None);
            }
            self.buf.put(&buf[..len]);
        }
    }

    pub fn get_buf(&mut self) -> BytesMut {
        self.buf.clone()
    }
}

pub async fn send_pack<W: AsyncWrite + Unpin>(writer: &mut W, msg: String) -> anyhow::Result<()> {
    let msg = Bytes::from(msg);
    let mut buf = BytesMut::new();
    buf.put_u64_le(msg.len() as u64);
    buf.put(msg);
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}
