use crate::SessionMessage;

use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};
use tokio::{io::AsyncWriteExt, net::TcpStream, sync::Mutex};

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
