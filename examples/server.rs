use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use session_rs::{Method, session::Session, ws::WebSocket};

#[derive(Debug, Serialize, Deserialize)]
struct Data;

impl Method for Data {
    const NAME: &'static str = "data";
    type Request = String;
    type Response = String;
    type Error = String;
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> session_rs::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on ws://127.0.0.1:8080");

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("New connection: {}", addr);

        tokio::spawn(async move {
            // Wrap session in Arc so tasks can share it
            let session = Session::from_ws(
                WebSocket::handshake(stream)
                    .await
                    .expect("Failed to initialize websocket"),
            );

            session.start_receiver();

            session
                .on::<Data, _>(async |_, req| {
                    println!("Msg from client: {req}");

                    if req == "invalid_data" {
                        return Err("Invalid data".to_string());
                    }

                    Ok("Hello from server".to_string())
                })
                .await;
        });
    }
}
