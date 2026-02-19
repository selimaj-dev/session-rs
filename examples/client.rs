use serde::{Deserialize, Serialize};
use session_rs::{Method, session::Session, ws::Frame};

#[derive(Debug, Serialize, Deserialize)]
struct Data {}

impl Method for Data {
    const NAME: &'static str = "data";
    type Request = ();
    type Response = ();
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> session_rs::Result<()> {
    let session = Session::connect("127.0.0.1:8080", "/").await?;

    tokio::spawn({
        let session = session.clone();
        async move {
            loop {
                match session.ws.read().await {
                    Ok(Frame::Text(text)) => {
                        println!("Server says: {}", text);
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    });

    session.request::<Data>(()).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    session.close().await?;
    Ok(())
}
