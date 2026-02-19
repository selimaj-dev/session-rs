use std::{marker::PhantomData, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::ws::WebSocket;

#[derive(Debug, Serialize, Deserialize)]
pub enum SessionMessageKind {
    Request,
    Response,
    Notification,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionMessage<T: Serialize> {
    id: u32,
    kind: SessionMessageKind,
    data: T,
}

pub struct Session<
    Req: Serialize + for<'a> Deserialize<'a>,
    Res: Serialize + for<'a> Deserialize<'a>,
    PeerReq: Serialize + for<'a> Deserialize<'a>,
    PeerRes: Serialize + for<'a> Deserialize<'a>,
    Notification: Serialize + for<'a> Deserialize<'a>,
> {
    _pd: (
        PhantomData<Req>,
        PhantomData<Res>,
        PhantomData<PeerReq>,
        PhantomData<PeerRes>,
        PhantomData<Notification>,
    ),
    pub ws: WebSocket,
    id: Arc<Mutex<u32>>,
}

impl<
    Req: Serialize + for<'a> Deserialize<'a>,
    Res: Serialize + for<'a> Deserialize<'a>,
    PeerReq: Serialize + for<'a> Deserialize<'a>,
    PeerRes: Serialize + for<'a> Deserialize<'a>,
    Notification: Serialize + for<'a> Deserialize<'a>,
> Session<Req, Res, PeerReq, PeerRes, Notification>
{
    pub fn clone(&self) -> Self {
        Self {
            _pd: (
                PhantomData,
                PhantomData,
                PhantomData,
                PhantomData,
                PhantomData,
            ),
            ws: self.ws.clone(),
            id: self.id.clone(),
        }
    }
}

impl<
    Req: Serialize + for<'a> Deserialize<'a>,
    Res: Serialize + for<'a> Deserialize<'a>,
    PeerReq: Serialize + for<'a> Deserialize<'a>,
    PeerRes: Serialize + for<'a> Deserialize<'a>,
    Notification: Serialize + for<'a> Deserialize<'a>,
> Session<Req, Res, PeerReq, PeerRes, Notification>
{
    pub fn from_ws(ws: WebSocket) -> Self {
        Self {
            _pd: (
                PhantomData,
                PhantomData,
                PhantomData,
                PhantomData,
                PhantomData,
            ),
            ws,
            id: Arc::new(Mutex::new(0)),
        }
    }

    pub async fn connect(addr: &str, path: &str) -> crate::Result<Self> {
        Ok(Self::from_ws(WebSocket::connect(addr, path).await?))
    }
}

impl<
    Req: Serialize + for<'a> Deserialize<'a>,
    Res: Serialize + for<'a> Deserialize<'a>,
    PeerReq: Serialize + for<'a> Deserialize<'a>,
    PeerRes: Serialize + for<'a> Deserialize<'a>,
    Notification: Serialize + for<'a> Deserialize<'a>,
> Session<Req, Res, PeerReq, PeerRes, Notification>
{
    pub async fn send_id<T: Serialize>(
        &self,
        id: u32,
        kind: SessionMessageKind,
        data: &T,
    ) -> crate::Result<()> {
        self.ws
            .send_text_payload(&serde_json::to_vec(&SessionMessage { id, kind, data })?)
            .await?;

        Ok(())
    }

    pub async fn send<T: Serialize>(
        &self,
        kind: SessionMessageKind,
        data: &T,
    ) -> crate::Result<()> {
        self.send_id(
            {
                let mut i = self.id.lock().await;
                *i += 1;
                *i
            },
            kind,
            data,
        )
        .await
    }

    pub async fn request(&self, data: &Req) -> crate::Result<()> {
        self.send(SessionMessageKind::Request, data).await
    }

    pub async fn respond(&self, to_req: &SessionMessage<PeerReq>, data: &Res) -> crate::Result<()> {
        self.send_id(to_req.id, SessionMessageKind::Response, data)
            .await
    }

    pub async fn notify(&self, data: &Res) -> crate::Result<()> {
        self.send(SessionMessageKind::Notification, data).await
    }

    pub async fn close(&self) -> crate::Result<()> {
        Ok(self.ws.close().await?)
    }
}
