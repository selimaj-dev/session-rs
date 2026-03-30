use base64::Engine;
use base64::engine::general_purpose::STANDARD as Base64;
use sha1::{Digest, Sha1};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::Mutex,
    time::{Duration, timeout},
};

use super::WebSocket;

pub async fn handle_websocket_handshake(stream: &mut TcpStream) -> std::io::Result<()> {
    let (read_half, mut write_half) = stream.split();
    let mut reader = BufReader::new(read_half);

    // ---- 1. Read request line with timeout ----
    let mut request_line = String::new();
    timeout(Duration::from_secs(5), reader.read_line(&mut request_line)).await??;

    let request_line = request_line.trim_end();

    if !request_line.starts_with("GET") {
        write_half
            .write_all(
                b"HTTP/1.1 405 Method Not Allowed\r\n\
                Content-Length: 0\r\n\
                Connection: close\r\n\r\n",
            )
            .await?;
        write_half.shutdown().await?;
        return Ok(());
    }

    // ---- 2. Read headers with timeout ----
    let mut headers = HashMap::new();

    loop {
        let mut line = String::new();
        timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;

        if line == "\r\n" {
            break;
        }

        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    // ---- 3. Check if this is a WebSocket upgrade ----
    let is_upgrade = headers
        .get("upgrade")
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    let has_connection_upgrade = headers
        .get("connection")
        .map(|v| v.to_lowercase().contains("upgrade"))
        .unwrap_or(false);

    if !is_upgrade || !has_connection_upgrade {
        // Normal HTTP response (important for browsers)
        let body = b"OK";

        write_half
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: text/plain\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\
                     \r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await?;

        write_half.write_all(body).await?;
        write_half.flush().await?;
        write_half.shutdown().await?;

        return Ok(());
    }

    // ---- 4. Validate required headers ----
    let key = headers.get("sec-websocket-key").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing Sec-WebSocket-Key")
    })?;

    let version_ok = headers
        .get("sec-websocket-version")
        .map(|v| v == "13")
        .unwrap_or(false);

    if !version_ok {
        write_half
            .write_all(
                b"HTTP/1.1 426 Upgrade Required\r\n\
                Sec-WebSocket-Version: 13\r\n\
                Content-Length: 0\r\n\
                Connection: close\r\n\r\n",
            )
            .await?;
        write_half.shutdown().await?;
        return Ok(());
    }

    // ---- 5. Generate Sec-WebSocket-Accept ----
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");

    let accept = Base64.encode(hasher.finalize());

    // ---- 6. Send upgrade response ----
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         \r\n",
        accept
    );

    write_half.write_all(response.as_bytes()).await?;
    write_half.flush().await?;

    Ok(())
}

impl WebSocket {
    pub async fn handshake(mut stream: TcpStream) -> super::Result<Self> {
        handle_websocket_handshake(&mut stream).await?;

        let (read, write) = stream.into_split();

        Ok(Self {
            id: rand::random(),
            reader: Arc::new(Mutex::new(read)),
            writer: Arc::new(Mutex::new(write)),
            is_server: false,
        })
    }

    /// Connect to a WebSocket server and perform the handshake
    pub async fn connect(addr: &str, path: &str) -> super::Result<Self> {
        // 1. TCP connect
        let mut stream = TcpStream::connect(addr).await?;

        // 2. Generate Sec-WebSocket-Key
        let key_bytes: [u8; 16] = rand::random();
        let key = base64::prelude::BASE64_STANDARD.encode(&key_bytes);

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
        timeout(
            tokio::time::Duration::from_secs(5),
            reader.read_line(&mut status_line),
        )
        .await??;
        if !status_line.starts_with("HTTP/1.1 101") {
            return Err(super::Error::HandshakeFailed(format!(
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
            base64::prelude::BASE64_STANDARD.encode(sha1.finalize())
        };
        if sec_accept.as_deref() != Some(expected.as_str()) {
            return Err(super::Error::HandshakeFailed(
                "Sec-WebSocket-Accept mismatch".into(),
            ));
        }

        // 6. Upgrade succeeded, split stream
        let (read, write) = stream.into_split();

        Ok(Self {
            id: rand::random(),
            reader: Arc::new(Mutex::new(read)),
            writer: Arc::new(Mutex::new(write)),
            is_server: true,
        })
    }
}
