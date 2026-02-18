use std::{
    hash::{Hash, Hasher},
    io::{self, Read, Write},
    net::TcpStream,
};

use serde::{Deserialize, Serialize};

use crate::SessionMessage;

pub struct Session(TcpStream, u64);

impl Session {
    /// Create a client
    pub fn new(mut stream: TcpStream) -> crate::Result<Self> {
        crate::handshake::handle_websocket_handshake(&mut stream)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(std::time::Duration::from_secs(10)))?;
        Ok(Session(stream, rand::random()))
    }

    /// Send a close frame and flush.
    pub fn send_close(&self) -> crate::Result<()> {
        let mut stream = self.0.try_clone()?;
        stream.write_all(&[0x88])?;
        stream.flush()?;
        Ok(())
    }

    /// Send a ping (no payload)
    fn send_ping(&self) -> crate::Result<()> {
        let mut stream = self.0.try_clone()?;
        // FIN + opcode (ping = 0x89), payload length = 0x00
        stream.write_all(&[0x89, 0x00])?;
        stream.flush()?;
        Ok(())
    }

    /// Send a pong (no payload)
    fn send_pong(&self) -> crate::Result<()> {
        let mut stream = self.0.try_clone()?;
        // FIN + opcode (pong = 0x8A), payload length = 0x00
        stream.write_all(&[0x8A, 0x00])?;
        stream.flush()?;
        Ok(())
    }

    /// Send a text/binary frame (server->client must NOT mask)
    pub fn send<T: Serialize>(&self, m: T) -> crate::Result<()> {
        let mut stream = self.0.try_clone()?;

        let payload = serde_json::to_string(&m)?;
        let payload_bytes = payload.as_bytes();
        let len = payload_bytes.len();

        let mut header = Vec::new();
        header.push(0x81); // FIN=1, opcode=0x1 (text)

        if len < 126 {
            header.push(len as u8);
        } else if len <= 65535 {
            header.push(126);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            header.push(127);
            header.extend_from_slice(&(len as u64).to_be_bytes());
        }

        stream.write_all(&header)?;
        stream.write_all(payload_bytes)?;
        stream.flush()?;
        Ok(())
    }

    /// Send a binary WebSocket frame (server -> client)
    pub fn send_bin(&self, payload: &[u8]) -> crate::Result<()> {
        let mut stream = self.0.try_clone()?;

        let mut header = Vec::with_capacity(10);

        // FIN=1, opcode=2 (binary)
        header.push(0x82);

        let len = payload.len();

        if len < 126 {
            header.push(len as u8); // mask bit = 0
        } else if len <= 0xFFFF {
            header.push(126);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            header.push(127);
            header.extend_from_slice(&(len as u64).to_be_bytes());
        }

        stream.write_all(&header)?;
        stream.write_all(payload)?;
        stream.flush()?;

        Ok(())
    }

    /// Read a full WebSocket message, handling fragmentation and control frames.
    ///
    /// Returns:
    /// - Ok(Some(WsMessage)) on an application message (text/binary)
    /// - Ok(None) if the connection should be closed (close received / read EOF)
    /// - Err on protocol or IO errors.
    pub fn read_t<T: Serialize + for<'de> Deserialize<'de>>(
        &self,
    ) -> crate::Result<Option<SessionMessage<T>>> {
        let mut stream = self.0.try_clone()?;

        let mut message_payload = Vec::new();
        let mut expecting_continuation = false;
        let mut message_type: Option<u8> = None; // 0x1 for text, 0x2 for binary

        loop {
            // Read 2-byte header
            let mut header = [0u8; 2];
            match stream.read_exact(&mut header) {
                Ok(_) => {}
                Err(e) => match e.kind() {
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {
                        self.send_ping()?;
                        continue;
                    }
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe => return Ok(None),
                    _ => return Err(e.into()),
                },
            }

            let fin = header[0] & 0x80 != 0;
            let opcode = header[0] & 0x0F;
            let masked = header[1] & 0x80 != 0;
            let mut payload_len = (header[1] & 0x7F) as u64;

            // Extended payload length
            if payload_len == 126 {
                let mut ext_len = [0u8; 2];
                stream.read_exact(&mut ext_len)?;
                payload_len = u16::from_be_bytes(ext_len) as u64;
            } else if payload_len == 127 {
                let mut ext_len = [0u8; 8];
                stream.read_exact(&mut ext_len)?;
                payload_len = u64::from_be_bytes(ext_len);
            }

            // Mask key
            let mut mask = [0u8; 4];
            if masked {
                stream.read_exact(&mut mask)?;
            } else {
                let _ = self.send_close();
                return Ok(None);
            }

            // Control frame checks
            if matches!(opcode, 0x8 | 0x9 | 0xA) {
                if payload_len > 125 {
                    let _ = self.send_close();
                    return Ok(None);
                }
                if !fin {
                    let _ = self.send_close();
                    return Ok(None);
                }
            }

            // Read payload
            let mut payload = vec![0u8; payload_len as usize];
            if payload_len > 0 {
                stream.read_exact(&mut payload)?;
                for i in 0..payload.len() {
                    payload[i] ^= mask[i % 4];
                }
            }

            match opcode {
                0x0 => {
                    // Continuation
                    if !expecting_continuation {
                        let _ = self.send_close();
                        return Ok(None);
                    }
                    message_payload.extend(payload);
                    if fin {
                        break;
                    }
                }
                0x1 => {
                    // Text
                    if expecting_continuation {
                        let _ = self.send_close();
                        return Ok(None);
                    }
                    message_payload.extend(payload);
                    message_type = Some(0x1);
                    if fin {
                        break;
                    } else {
                        expecting_continuation = true;
                    }
                }
                0x2 => {
                    // Binary
                    if expecting_continuation {
                        let _ = self.send_close();
                        return Ok(None);
                    }
                    message_payload.extend(payload);
                    message_type = Some(0x2);
                    if fin {
                        break;
                    } else {
                        expecting_continuation = true;
                    }
                }
                0x8 => {
                    // Close
                    let _ = self.send_close();
                    return Ok(None);
                }
                0x9 => {
                    // Ping
                    self.send_pong()?;
                    continue;
                }
                0xA => {
                    // Pong
                    continue;
                }
                _ => {
                    let _ = self.send_close();
                    return Ok(None);
                }
            }
        }

        // Convert payload into proper message type
        let message = match message_type {
            Some(0x1) => {
                // Text frame → try JSON, otherwise keep text
                match String::from_utf8(message_payload.clone()) {
                    Ok(text) => match serde_json::from_str(&text) {
                        Ok(msg) => SessionMessage::SessionMessage(msg),
                        Err(e) => return Err(crate::Error::Json(e)),
                    },
                    Err(_) => SessionMessage::Binary(message_payload),
                }
            }
            Some(0x2) => SessionMessage::Binary(message_payload),
            _ => return Ok(None), // Should not happen
        };

        Ok(Some(message))
    }

    pub fn close(&self) -> crate::Result<()> {
        self.0.shutdown(std::net::Shutdown::Both)?;
        Ok(())
    }
}

impl Clone for Session {
    fn clone(&self) -> Self {
        Session(
            self.0.try_clone().expect("failed to clone TcpStream"),
            self.1.clone(),
        )
    }
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.1 == other.1
    }
}

impl Eq for Session {}

impl Hash for Session {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.1.hash(state);
    }
}
