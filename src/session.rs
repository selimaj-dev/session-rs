use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::sync::broadcast;

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
    tx: broadcast::Sender<(u32, bool, serde_json::Value)>,
}

impl Session {
    pub fn clone(&self) -> Self {
        Self {
            ws: self.ws.clone(),
            id: self.id.clone(),
            methods: self.methods.clone(),
            tx: self.tx.clone(),
        }
    }
}

impl Session {
    pub fn from_ws(ws: WebSocket) -> Self {
        Self {
            ws,
            id: Arc::new(Mutex::new(0)),
            methods: Arc::new(Mutex::new(HashMap::new())),
            tx: broadcast::channel(8192).0,
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
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });
    }

    pub async fn on<
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

    pub async fn close(&self) -> crate::Result<()> {
        Ok(self.ws.close().await?)
    }
}
