use std::{net::SocketAddr, sync::Arc};

use tokio::{net::TcpListener, time::timeout};

use crate::{session::Session, ws::WebSocket};

pub struct SessionServer {
    listener: TcpListener,
}

impl SessionServer {
    pub async fn bind(addr: &str) -> crate::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr).await?,
        })
    }

    pub async fn accept(&self) -> crate::Result<(Session, SocketAddr)> {
        let (stream, addr) = self.listener.accept().await?;

        let ws = WebSocket::handshake(stream).await?;

        Ok((Session::from_ws(ws), addr))
    }

    pub async fn session_loop<F, Fut>(&self, on_conn: F) -> crate::Result<()>
    where
        F: Fn(Session, SocketAddr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::Result<()>> + Send + 'static,
    {
        let conn_handler = Arc::new(on_conn);

        loop {
            let (stream, addr) = self.listener.accept().await?;
            let conn_handler = conn_handler.clone();

            tokio::spawn(async move {
                match timeout(
                    tokio::time::Duration::from_secs(5),
                    WebSocket::handshake(stream),
                )
                .await
                {
                    Ok(Ok(ws)) => {
                        let session = Session::from_ws(ws);
                        session.start_receiver();

                        if let Err(e) = conn_handler(session, addr).await {
                            eprintln!("Connection error: {:?}", e);
                        }
                    }
                    Ok(Err(e)) => {
                        eprintln!("Handshake failed from {}: {:?}", addr, e);
                    }
                    Err(_) => {
                        eprintln!("Handshake failed from {}: Handshake Timeout", addr);
                    }
                }
            });
        }
    }
}
