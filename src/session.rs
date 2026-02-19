use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{GenericMethod, Method, ws::WebSocket};

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
        error: bool,
        result: M::Response,
    },
    Notification {
        method: String,
        data: M::Request,
    },
}

pub struct Session {
    pub ws: WebSocket,
    id: Arc<Mutex<u32>>,
    methods: Arc<Mutex<HashMap<String, Box<dyn Fn(u32, serde_json::Value) + Send + Sync>>>>,
}

impl Session {
    pub fn clone(&self) -> Self {
        Self {
            ws: self.ws.clone(),
            id: self.id.clone(),
            methods: self.methods.clone(),
        }
    }
}

impl Session {
    pub fn from_ws(ws: WebSocket) -> Self {
        Self {
            ws,
            id: Arc::new(Mutex::new(0)),
            methods: Arc::new(Mutex::new(HashMap::new())),
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
                                    (m)(id, data)
                                }
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

    pub async fn on<M: Method>(&self, handler: impl Fn(u32, M::Request) + Send + Sync + 'static) {
        self.methods.lock().await.insert(
            M::NAME.to_string(),
            Box::new(move |id, value| {
                if let Ok(req) = serde_json::from_value(value) {
                    (handler)(id, req)
                }
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

    pub async fn request<M: Method>(&self, req: M::Request) -> crate::Result<()> {
        self.send::<M>(&Message::Request {
            id: self.use_id().await,
            method: M::NAME.to_string(),
            data: req,
        })
        .await
    }

    pub async fn respond<M: Method>(
        &self,
        to: u32,
        error: bool,
        res: M::Response,
    ) -> crate::Result<()> {
        self.send::<M>(&Message::Response {
            id: to,
            error,
            result: res,
        })
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
