//! Framing: `[u8 kind][u32 BE len][payload]`. Control payloads are JSON `Msg`;
//! data payloads are `[u32 BE item id][bytes]` and always follow a `Start` for that item.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u32 = 1;
const CONTROL: u8 = 1;
const DATA: u8 = 2;
/// Upper bound for any frame; an Offer for ~200k files fits, a hostile peer can't make us allocate more.
pub const MAX_FRAME: usize = 64 << 20;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "t")]
pub enum Msg {
    Hello {
        name: String,
        kind: String,
        version: u32,
    },
    Offer {
        transfer_id: String,
        items: Vec<Item>,
        token: Option<String>,
    },
    Answer {
        accepted: bool,
        items: Vec<ItemAnswer>,
        reason: Option<String>,
    },
    Start {
        item: u32,
        offset: u64,
    },
    FileEnd {
        item: u32,
        hash: String,
    },
    Verified {
        item: u32,
        ok: bool,
        error: Option<String>,
    },
    Pause,
    Resume,
    Done,
    Cancel {
        reason: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum ItemKind {
    File,
    Dir,
    Text,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Item {
    pub id: u32,
    pub kind: ItemKind,
    /// Relative path using `/`, or a label for text.
    pub path: String,
    pub size: u64,
    /// Unix seconds.
    pub mtime: Option<i64>,
    pub text: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ItemAnswer {
    pub id: u32,
    pub accept: bool,
    /// Bytes the receiver already holds from an interrupted attempt.
    pub offset: u64,
    /// BLAKE3 of those bytes; the sender restarts from 0 if its prefix differs.
    pub prefix_hash: Option<String>,
}

pub enum Frame {
    Control(Msg),
    Data { item: u32, bytes: Vec<u8> },
}

pub async fn write_msg<W: AsyncWrite + Unpin>(w: &mut W, m: &Msg) -> Result<()> {
    let b = serde_json::to_vec(m)?;
    let mut h = [0u8; 5];
    h[0] = CONTROL;
    h[1..].copy_from_slice(&(b.len() as u32).to_be_bytes());
    w.write_all(&h).await?;
    w.write_all(&b).await?;
    w.flush().await?;
    Ok(())
}

pub async fn write_data<W: AsyncWrite + Unpin>(w: &mut W, item: u32, bytes: &[u8]) -> Result<()> {
    let mut h = [0u8; 9];
    h[0] = DATA;
    h[1..5].copy_from_slice(&((bytes.len() + 4) as u32).to_be_bytes());
    h[5..].copy_from_slice(&item.to_be_bytes());
    w.write_all(&h).await?;
    w.write_all(bytes).await?;
    Ok(())
}

pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Frame> {
    let mut h = [0u8; 5];
    r.read_exact(&mut h).await?;
    let len = u32::from_be_bytes([h[1], h[2], h[3], h[4]]) as usize;
    if len > MAX_FRAME {
        bail!("peer sent an oversized frame ({len} bytes)");
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    match h[0] {
        CONTROL => Ok(Frame::Control(serde_json::from_slice(&buf)?)),
        DATA if len >= 4 => {
            let item = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
            buf.drain(..4);
            Ok(Frame::Data { item, bytes: buf })
        }
        k => bail!("peer sent an unknown frame type {k}"),
    }
}

pub async fn read_msg<R: AsyncRead + Unpin>(r: &mut R) -> Result<Msg> {
    match read_frame(r).await? {
        Frame::Control(m) => Ok(m),
        Frame::Data { .. } => bail!("protocol error: unexpected data frame"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_and_limits() {
        let (mut a, mut b) = tokio::io::duplex(1 << 16);
        write_msg(&mut a, &Msg::Pause).await.unwrap();
        write_data(&mut a, 7, b"hello").await.unwrap();
        assert!(matches!(
            read_frame(&mut b).await.unwrap(),
            Frame::Control(Msg::Pause)
        ));
        match read_frame(&mut b).await.unwrap() {
            Frame::Data { item, bytes } => assert_eq!((item, bytes.as_slice()), (7, &b"hello"[..])),
            _ => panic!(),
        }
        // oversized and garbage frames are rejected, not allocated
        a.write_all(&[1, 0xff, 0xff, 0xff, 0xff]).await.unwrap();
        assert!(read_frame(&mut b).await.is_err());
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&[9, 0, 0, 0, 0]).await.unwrap();
        assert!(read_frame(&mut b).await.is_err());
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&[1, 0, 0, 0, 3, b'{', b'x', b'}'])
            .await
            .unwrap();
        assert!(read_frame(&mut b).await.is_err());
    }
}
