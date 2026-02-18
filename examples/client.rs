use std::sync::Arc;

use session_rs::ws::WebSocket;

#[tokio::main(flavor = "current_thread")]
async fn main() -> session_rs::Result<()> {
    let session = Arc::new(WebSocket::connect("127.0.0.1:8080", "/").await?);

    // Spawn read loop
    let read_session = Arc::clone(&session);
    tokio::spawn(async move {
        loop {
            match read_session.read().await {
                Ok(Some((opcode, payload))) => {
                    if opcode == 0x1 {
                        let text = String::from_utf8(payload).unwrap_or_default();
                        println!("Server says: {}", text);
                    }
                }
                Ok(None) => {}
                Err(_) => break,
            }
        }
    });

    // Send a few messages
    for i in 0..5 {
        println!("sending");
        let msg = serde_json::json!({ "hello": i });
        session.send(&msg).await?;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    session.close().await?;
    Ok(())
}
