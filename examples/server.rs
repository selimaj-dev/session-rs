use std::sync::Arc;
use tokio::net::TcpListener;

use session_rs::session::Session;

#[tokio::main(flavor = "current_thread")]
async fn main() -> session_rs::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on ws://127.0.0.1:8080");

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("New connection: {}", addr);

        tokio::spawn(async move {
            // Wrap session in Arc so tasks can share it
            let session = match Session::new(stream).await {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    eprintln!("Handshake failed: {:?}", e);
                    return;
                }
            };

            // Simple ping loop
            let ping_session = Arc::clone(&session);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
                loop {
                    interval.tick().await;
                    if ping_session.send_ping().await.is_err() {
                        break;
                    }
                }
            });

            // Read loop
            loop {
                match session.read_frame().await {
                    Ok(Some((opcode, payload))) => {
                        if opcode == 0x1 {
                            // Text frame → parse JSON if possible
                            let text = String::from_utf8(payload).unwrap_or_default();
                            println!("Received text: {}", text);

                            // Echo back
                            if let Err(e) = session.send(&serde_json::json!({"echo": text})).await {
                                eprintln!("Send error: {:?}", e);
                                break;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(_) => break, // connection closed
                }
            }

            let _ = session.close().await;
            println!("Connection {} closed", addr);
        });
    }
}
