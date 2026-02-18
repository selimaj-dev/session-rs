use sha1::{Digest, Sha1};
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::Mutex,
};

use crate::session::Session;

pub async fn handle_websocket_handshake(stream: &mut TcpStream) -> std::io::Result<()> {
    let (read_half, mut write_half) = stream.split();
    let mut reader = BufReader::new(read_half);

    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    let request_line = request_line.trim_end();

    if request_line.starts_with("HEAD") {
        write_half
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .await?;
        return Ok(());
    }

    if !request_line.starts_with("GET") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid HTTP method",
        ));
    }

    use std::collections::HashMap;
    let mut headers = HashMap::new();
    let mut line = String::new();

    loop {
        line.clear();
        reader.read_line(&mut line).await?;
        if line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    if headers
        .get("upgrade")
        .map(|v| !v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(true)
    {
        write_half
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
            .await?;
        return Ok(());
    }

    let key = headers
        .get("sec-websocket-key")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing key"))?;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as Base64;
    use sha1::{Digest, Sha1};

    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let accept = Base64.encode(hasher.finalize());

    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        accept
    );

    write_half.write_all(response.as_bytes()).await?;
    Ok(())
}

impl Session {
    pub async fn handshake(mut stream: TcpStream) -> crate::Result<Self> {
        crate::handshake::handle_websocket_handshake(&mut stream).await?;

        let (read, write) = stream.into_split();

        Ok(Self {
            id: rand::random(),
            reader: Arc::new(Mutex::new(read)),
            writer: Arc::new(Mutex::new(write)),
            mask_payload: false,
        })
    }

    /// Connect to a WebSocket server and perform the handshake
    pub async fn connect(addr: &str, path: &str) -> crate::Result<Self> {
        // 1. TCP connect
        let mut stream = TcpStream::connect(addr).await?;

        // 2. Generate Sec-WebSocket-Key
        let key_bytes: [u8; 16] = rand::random();
        let key = base64::encode(&key_bytes);

        // 3. Send HTTP Upgrade request
        let request = format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            path, addr, key
        );
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        // 4. Read HTTP response
        let mut reader = BufReader::new(&mut stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).await?;
        if !status_line.starts_with("HTTP/1.1 101") {
            return Err(crate::Error::HandshakeFailed(format!(
                "Expected 101 Switching Protocols, got: {}",
                status_line.trim_end()
            )));
        }

        // Read headers
        let mut sec_accept = None;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await?;
            let line = line.trim_end();
            if line.is_empty() {
                break; // end of headers
            }
            if let Some((k, v)) = line.split_once(':') {
                if k.eq_ignore_ascii_case("sec-websocket-accept") {
                    sec_accept = Some(v.trim().to_string());
                }
            }
        }

        // 5. Verify Sec-WebSocket-Accept
        let expected = {
            let mut sha1 = Sha1::new();
            sha1.update(key.as_bytes());
            sha1.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
            base64::encode(sha1.finalize())
        };
        if sec_accept.as_deref() != Some(expected.as_str()) {
            return Err(crate::Error::HandshakeFailed(
                "Sec-WebSocket-Accept mismatch".into(),
            ));
        }

        // 6. Upgrade succeeded, split stream
        let (read, write) = stream.into_split();

        Ok(Self {
            id: rand::random(),
            reader: Arc::new(Mutex::new(read)),
            writer: Arc::new(Mutex::new(write)),
            mask_payload: true,
        })
    }
}
