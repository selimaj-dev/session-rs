use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

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
