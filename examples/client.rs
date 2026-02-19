use serde::{Deserialize, Serialize};
use session_rs::session::Session;

#[derive(Debug, Serialize, Deserialize)]
struct Data {}

type Communication = Session<Data, Data, Data, Data, Data>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> session_rs::Result<()> {
    let session = Communication::connect("127.0.0.1:8080", "/").await?;

    // Spawn read loop
    // tokio::spawn({
    //     let session = session.clone();
    //     async move {
    //         loop {
    //             match session.read().await {
    //                 Ok(Frame::Text(text)) => {
    //                     println!("Server says: {}", text);
    //                 }
    //                 Ok(_) => {}
    //                 Err(_) => break,
    //             }
    //         }
    //     }
    // });

    session.request(&Data {}).await?;

    session.close().await?;
    Ok(())
}
