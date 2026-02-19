use std::{net::SocketAddr, sync::Arc};

use tokio::net::TcpListener;

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

    pub async fn session_loop<Fut: Future<Output = crate::Result<()>> + Send + 'static>(
        &self,
        on_conn: impl Fn(Session, SocketAddr) -> Fut + 'static,
    ) -> crate::Result<()> {
        let conn_handler = Arc::new(on_conn);

        loop {
            let (session, addr) = self.accept().await?;
            let conn_handler = conn_handler.clone();

            session.start_receiver();

            tokio::spawn(conn_handler(session, addr));
        }
    }
}
