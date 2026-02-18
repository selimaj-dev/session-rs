pub mod error;
pub mod handshake;
pub use error::{Error, Result};

use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

#[derive(Debug, Clone)]
pub enum Frame {
    Text(String),
    Binary(Vec<u8>),
    Ping,
    Pong,
    Close,
}

pub struct WebSocket {
    pub(crate) reader: Arc<Mutex<tokio::net::tcp::OwnedReadHalf>>,
    pub(crate) writer: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    pub(crate) id: u64,
    pub(crate) mask_payload: bool,
}

impl Clone for WebSocket {
    fn clone(&self) -> Self {
        WebSocket {
            reader: self.reader.clone(),
            writer: self.writer.clone(),
            mask_payload: self.mask_payload.clone(),
            id: self.id,
        }
    }
}

impl PartialEq for WebSocket {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for WebSocket {}

impl Hash for WebSocket {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl WebSocket {
    async fn send_frame(&self, opcode: u8, payload: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().await;

        let mut header = Vec::with_capacity(10);
        let mask_bit = if self.mask_payload { 0x80 } else { 0x00 };
        header.push(0x80 | opcode); // FIN + opcode

        let len = payload.len();
        if len < 126 {
            header.push((len as u8) | mask_bit);
        } else if len <= 0xFFFF {
            header.push(126 | mask_bit);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            header.push(127 | mask_bit);
            header.extend_from_slice(&(len as u64).to_be_bytes());
        }

        if self.mask_payload {
            // Generate 4-byte mask key
            let mask_key: [u8; 4] = rand::random();
            header.extend_from_slice(&mask_key);

            // Mask the payload
            let mut masked_payload = payload.to_vec();
            for i in 0..masked_payload.len() {
                masked_payload[i] ^= mask_key[i % 4];
            }

            writer.write_all(&header).await?;
            writer.write_all(&masked_payload).await?;
        } else {
            writer.write_all(&header).await?;
            writer.write_all(payload).await?;
        }

        writer.flush().await?;
        Ok(())
    }
}

impl WebSocket {
    pub async fn send(&self, msg: &str) -> Result<()> {
        self.send_frame(0x1, msg.as_bytes()).await
    }

    pub async fn send_bin(&self, payload: &[u8]) -> Result<()> {
        self.send_frame(0x2, payload).await
    }

    pub async fn send_ping(&self) -> Result<()> {
        self.send_frame(0x9, &[]).await
    }

    pub async fn send_pong(&self) -> Result<()> {
        self.send_frame(0xA, &[]).await
    }

    pub async fn close(&self) -> Result<()> {
        self.send_frame(0x8, &[]).await
    }

    pub fn start_ping_loop(&self) {
        let s = self.clone();
        tokio::task::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                interval.tick().await;
                if s.send_ping().await.is_err() {
                    break;
                }
            }
        });
    }
}

impl WebSocket {
    /// Read a full WebSocket frame (handling masking and control frames)
    /// Returns (opcode, payload)
    pub async fn read_frame(&self) -> Result<(bool, u8, Vec<u8>)> {
        let mut reader = self.reader.lock().await;

        // --- 1. Read first 2-byte header ---
        let mut header = [0u8; 2];
        reader.read_exact(&mut header).await?;

        let fin = header[0] & 0x80 != 0;
        let opcode = header[0] & 0x0F;
        let masked = header[1] & 0x80 != 0;
        let mut payload_len = (header[1] & 0x7F) as u64;

        // --- 2. Read extended payload length if necessary ---
        if payload_len == 126 {
            let mut buf = [0u8; 2];
            reader.read_exact(&mut buf).await?;
            payload_len = u16::from_be_bytes(buf) as u64;
        } else if payload_len == 127 {
            let mut buf = [0u8; 8];
            reader.read_exact(&mut buf).await?;
            payload_len = u64::from_be_bytes(buf);
        }

        // --- 3. Read mask key ---
        if !masked && !self.mask_payload {
            // Per spec, client-to-server frames MUST be masked
            self.close().await.ok();
            return Err(Error::InvalidFrame(
                "Received unmasked frame from client".into(),
            ));
        }

        let mut mask = [0u8; 4];
        reader.read_exact(&mut mask).await?;

        // --- 4. Read payload ---
        let mut payload = vec![0u8; payload_len as usize];
        if payload_len > 0 {
            reader.read_exact(&mut payload).await?;
            for i in 0..payload.len() {
                payload[i] ^= mask[i % 4];
            }
        }

        // --- 6. Return opcode + payload ---
        Ok((fin, opcode, payload))
    }

    pub async fn read(&self) -> Result<Frame> {
        let (fin, opcode, mut payload) = self.read_frame().await?;

        if !fin {
            // Continuation loop
            while let (fin, o, mut p) = self.read_frame().await?
                && !fin
            {
                match o {
                    // Continuation
                    0x0 => payload.append(&mut p),
                    // Close
                    0x8 => {
                        self.close().await.ok();
                    }
                    // Ping
                    0x9 => {
                        self.send_pong().await.ok();
                    }
                    // Pong
                    0xA => {}
                    _ => {
                        self.close().await.ok();
                        return Err(Error::InvalidFrame(format!("Unknown opcode: {opcode}")));
                    }
                }
            }
        }

        match opcode {
            // Close
            0x8 => {
                self.close().await.ok();
                Ok(Frame::Close)
            }

            // Ping
            0x9 => {
                self.send_pong().await.ok();
                Ok(Frame::Ping)
            }

            // Pong
            0xA => Ok(Frame::Pong),

            // Text
            0x1 => Ok(Frame::Text(String::from_utf8(payload)?)),

            // Binary
            0x2 => Ok(Frame::Binary(payload)),

            _ => {
                self.close().await.ok();
                Err(Error::InvalidFrame(format!("Unknown opcode: {opcode}")))
            }
        }
    }
}
