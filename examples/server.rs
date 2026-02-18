use std::sync::Arc;
use tokio::net::TcpListener;

use session_rs::ws::{Frame, WebSocket};

#[tokio::main(flavor = "current_thread")]
async fn main() -> session_rs::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on ws://127.0.0.1:8080");

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("New connection: {}", addr);

        tokio::spawn(async move {
            // Wrap session in Arc so tasks can share it
            let session = match WebSocket::handshake(stream).await {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    eprintln!("Handshake failed: {:?}", e);
                    return;
                }
            };

            session.start_ping_loop();

            // Read loop
            loop {
                match session.read().await {
                    Ok(Frame::Text(text)) => {
                        println!("Received text: {}", text);

                        // Echo back
                        if let Err(e) = session.send(&serde_json::json!({"echo": text}).to_string()).await {
                            eprintln!("Send error: {:?}", e);
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("{e:?}");
                        break;
                    }
                }
            }
        });
    }
}
