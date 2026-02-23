use std::hash::Hash;
use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::time::timeout;

use crate::BoxFuture;
use crate::{GenericMethod, Method, MethodHandler, ws::WebSocket};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum Message<M: Method> {
    Request {
        id: u32,
        method: String,
        data: M::Request,
    },
    Response {
        id: u32,
        result: M::Response,
    },
    ErrorResponse {
        id: u32,
        error: M::Error,
    },
    Notification {
        method: String,
        data: M::Request,
    },
}

pub struct Session {
    pub ws: WebSocket,
    id: Arc<Mutex<u32>>,
    methods: Arc<Mutex<HashMap<String, MethodHandler>>>,
    on_close_fn:
        Arc<Mutex<Option<Box<dyn Fn() -> BoxFuture<'static, Result<(), String>> + Send + Sync>>>>,
    tx: broadcast::Sender<(u32, bool, serde_json::Value)>,
    pong_tx: broadcast::Sender<()>,
}

impl Session {
    pub fn clone(&self) -> Self {
        Self {
            ws: self.ws.clone(),
            id: self.id.clone(),
            methods: self.methods.clone(),
            on_close_fn: self.on_close_fn.clone(),
            tx: self.tx.clone(),
            pong_tx: self.pong_tx.clone(),
        }
    }
}

impl Session {
    pub fn from_ws(ws: WebSocket) -> Self {
        let (tx, _) = broadcast::channel(8192);
        let (pong_tx, _) = broadcast::channel(16);

        Self {
            ws,
            id: Arc::new(Mutex::new(0)),
            methods: Arc::new(Mutex::new(HashMap::new())),
            on_close_fn: Arc::new(Mutex::new(None)),
            tx,
            pong_tx,
        }
    }

    pub async fn connect(addr: &str, path: &str) -> crate::Result<Self> {
        Ok(Self::from_ws(WebSocket::connect(addr, path).await?))
    }
}

impl Session {
    pub fn start_receiver(&self) {
        let s = self.clone();
        tokio::spawn(async move {
            loop {
                match s.ws.read().await {
                    Ok(crate::ws::Frame::Text(text)) => {
                        let Ok(msg) = serde_json::from_str::<Message<GenericMethod>>(&text) else {
                            continue;
                        };

                        match msg {
                            Message::Request { id, method, data } => {
                                if let Some(m) = s.methods.lock().await.get(&method) {
                                    if let Some((err, res)) = (m)(id, data).await {
                                        if err {
                                            s.respond_error(id, res)
                                                .await
                                                .expect("Failed to respond");
                                        } else {
                                            s.respond(id, res).await.expect("Failed to respond");
                                        }
                                    }
                                }
                            }
                            Message::Response { id, result } => {
                                s.tx.send((id, false, result)).unwrap();
                            }
                            Message::ErrorResponse { id, error } => {
                                s.tx.send((id, true, error)).unwrap();
                            }
                            _ => {}
                        }
                    }
                    Ok(crate::ws::Frame::Pong) => {
                        let _ = s.pong_tx.send(());
                    }
                    Ok(_) => {}
                    Err(_) => {
                        s.trigger_close().await;
                        break;
                    }
                }
            }
        });
    }
    pub fn start_ping(&self, interval: tokio::time::Duration, timeout_dur: tokio::time::Duration) {
        let s = self.clone();

        tokio::spawn(async move {
            let mut pong_rx = s.pong_tx.subscribe();

            loop {
                tokio::time::sleep(interval).await;

                if s.ws.send_ping().await.is_err() {
                    s.trigger_close().await;
                    break;
                }

                let result = timeout(timeout_dur, pong_rx.recv()).await;

                if result.is_err() {
                    // timeout expired
                    let _ = s.close().await;
                    s.trigger_close().await;
                    break;
                }
            }
        });
    }

    pub async fn on_request<
        M: Method,
        Fut: Future<Output = Result<M::Response, M::Error>> + Send + 'static,
    >(
        &self,
        handler: impl Fn(u32, M::Request) -> Fut + Send + Sync + 'static,
    ) {
        let handler = Arc::new(handler);

        self.methods.lock().await.insert(
            M::NAME.to_string(),
            Box::new(move |id, value| {
                let handler = Arc::clone(&handler);

                Box::pin(async move {
                    Some(
                        match handler(id, serde_json::from_value(value).ok()?).await {
                            Ok(v) => (false, serde_json::to_value(v).ok()?),
                            Err(v) => (true, serde_json::to_value(v).ok()?),
                        },
                    )
                })
            }),
        );
    }

    pub async fn on_close<Fut>(&self, handler: impl Fn() -> Fut + Send + Sync + 'static)
    where
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let handler = Arc::new(handler);

        *self.on_close_fn.lock().await = Some(Box::new(move || {
            let handler = handler.clone();
            Box::pin(async move { handler().await })
        }));
    }
}

impl Session {
    pub async fn send<M: Method>(&self, data: &Message<M>) -> crate::Result<()> {
        self.ws
            .send_text_payload(&serde_json::to_vec(&data)?)
            .await?;
        Ok(())
    }

    pub async fn use_id(&self) -> u32 {
        let mut id = self.id.lock().await;
        *id += 1;
        *id
    }

    pub async fn request<M: Method>(
        &self,
        req: M::Request,
    ) -> crate::Result<std::result::Result<M::Response, M::Error>> {
        let id = self.use_id().await;

        self.send::<M>(&Message::Request {
            id,
            method: M::NAME.to_string(),
            data: req,
        })
        .await?;

        let mut rx = self.tx.subscribe();

        loop {
            let r = rx.recv().await?;

            if r.0 == id {
                break Ok(if r.1 {
                    Err(serde_json::from_value(r.2)?)
                } else {
                    Ok(serde_json::from_value(r.2)?)
                });
            }
        }
    }

    pub async fn respond(&self, to: u32, val: serde_json::Value) -> crate::Result<()> {
        self.send::<GenericMethod>(&Message::Response {
            id: to,
            result: val,
        })
        .await
    }

    pub async fn respond_error(&self, to: u32, val: serde_json::Value) -> crate::Result<()> {
        self.send::<GenericMethod>(&Message::ErrorResponse { id: to, error: val })
            .await
    }

    pub async fn notify<M: Method>(&self, data: M::Request) -> crate::Result<()> {
        self.send::<M>(&Message::Notification {
            method: M::NAME.to_string(),
            data,
        })
        .await
    }

    async fn trigger_close(&self) {
        if let Some(handler) = self.on_close_fn.lock().await.as_ref() {
            let _ = handler().await;
        }
    }

    pub async fn close(&self) -> crate::Result<()> {
        let res = self.ws.close().await;
        self.trigger_close().await;
        Ok(res?)
    }
}

impl Hash for Session {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.ws.id.hash(state);
    }
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.ws.id == other.ws.id
    }
}

impl Eq for Session {}
