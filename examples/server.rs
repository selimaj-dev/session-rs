use serde::{Deserialize, Serialize};
use session_rs::{Method, server::SessionServer};

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
    let server = SessionServer::bind("127.0.0.1:8080").await?;

    server
        .session_loop(async |session, _| {
            session
                .on::<Data, _>(async |_, req| {
                    println!("Msg from client: {req}");

                    if req == "invalid_data" {
                        return Err("Invalid data".to_string());
                    }

                    Ok("Hello from server".to_string())
                })
                .await;

            Ok(())
        })
        .await
}
