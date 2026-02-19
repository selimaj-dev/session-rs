use serde::{Deserialize, Serialize};
use session_rs::{Method, session::Session};

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
    let session = Session::connect("127.0.0.1:8080", "/").await?;

    session.start_receiver();

    println!(
        "Hi: {:?}",
        session
            .request::<Data>("Hello from client".to_string())
            .await?
    );

    println!(
        "Invalid data response: {:?}",
        session
            .request::<Data>("invalid_data".to_string())
            .await?
    );

    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    session.close().await?;
    Ok(())
}
