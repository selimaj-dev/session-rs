use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Mutex,
};

pub struct Session {
    reader: Arc<Mutex<tokio::net::tcp::OwnedReadHalf>>,
    writer: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    id: u64,
}

impl Session {
    pub async fn new(mut stream: TcpStream) -> crate::Result<Self> {
        crate::handshake::handle_websocket_handshake(&mut stream).await?;

        let (read, write) = stream.into_split();

        Ok(Self {
            reader: Arc::new(Mutex::new(read)),
            writer: Arc::new(Mutex::new(write)),
            id: rand::random(),
        })
    }
}

impl Clone for Session {
    fn clone(&self) -> Self {
        Session {
            reader: self.reader.clone(),
            writer: self.writer.clone(),
            id: self.id,
        }
    }
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Session {}

impl Hash for Session {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Session {
    async fn send_frame(&self, opcode: u8, payload: &[u8]) -> crate::Result<()> {
        let mut writer = self.writer.lock().await;

        let mut header = Vec::with_capacity(10);
        header.push(0x80 | opcode);

        let len = payload.len();
        if len < 126 {
            header.push(len as u8);
        } else if len <= 0xFFFF {
            header.push(126);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            header.push(127);
            header.extend_from_slice(&(len as u64).to_be_bytes());
        }

        writer.write_all(&header).await?;
        writer.write_all(payload).await?;
        Ok(())
    }
}

impl Session {
    pub async fn send<T: serde::Serialize>(&self, msg: &T) -> crate::Result<()> {
        let payload = serde_json::to_vec(msg)?;
        self.send_frame(0x1, &payload).await
    }

    pub async fn send_bin(&self, payload: &[u8]) -> crate::Result<()> {
        self.send_frame(0x2, payload).await
    }

    pub async fn send_ping(&self) -> crate::Result<()> {
        let mut writer = self.writer.lock().await;
        // FIN + opcode = 0x89 (ping), payload length = 0
        writer.write_all(&[0x89, 0x00]).await?;
        writer.flush().await?;
        Ok(())
    }

    pub async fn send_pong(&self) -> crate::Result<()> {
        let mut writer = self.writer.lock().await;
        // FIN + opcode = 0x8A (pong), payload length = 0
        writer.write_all(&[0x8A, 0x00]).await?;
        writer.flush().await?;
        Ok(())
    }

    pub fn start_ping(self: Arc<Self>) {
        tokio::task::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                interval.tick().await;
                if self.send_ping().await.is_err() {
                    break;
                }
            }
        });
    }

    /// Send a close frame and flush
    pub async fn close(&self) -> crate::Result<()> {
        let mut writer = self.writer.lock().await;

        // FIN=1, opcode=0x8 (close), payload length=0
        let frame = [0x88, 0x00];
        writer.write_all(&frame).await?;
        writer.flush().await?;
        Ok(())
    }
}

impl Session {
    /// Read a full WebSocket frame (handling masking and control frames)
    /// Returns (opcode, payload)
    pub async fn read_frame(&self) -> crate::Result<Option<(u8, Vec<u8>)>> {
        let mut reader = self.reader.lock().await;

        // --- 1. Read first 2-byte header ---
        let mut header = [0u8; 2];
        reader.read_exact(&mut header).await?;

        // let fin = header[0] & 0x80 != 0;
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
        if !masked {
            // Per spec, client-to-server frames MUST be masked
            self.close().await.ok();
            return Err(crate::Error::InvalidFrame(
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

        // --- 5. Handle control frames immediately ---
        match opcode {
            0x8 => {
                // Close
                self.close().await.ok();
                return Err(crate::Error::ConnectionClosed);
            }
            0x9 => {
                // Ping
                self.send_pong().await.ok();
                return Ok(None);
            }
            0xA => {
                // Pong, ignore
                return Ok(None);
            }
            0x0 | 0x1 | 0x2 => {
                // Continuation / Text / Binary → valid payload
            }
            _ => {
                self.close().await.ok();
                return Err(crate::Error::InvalidFrame(format!(
                    "Unknown opcode: {}",
                    opcode
                )));
            }
        }

        // --- 6. Return opcode + payload ---
        Ok(Some((opcode, payload)))
    }
}
