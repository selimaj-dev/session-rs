use std::pin::Pin;

use serde::{Deserialize, Serialize};

pub mod server;
pub mod session;
pub mod ws;

pub type Result<T> = std::result::Result<T, Error>;
pub type BoxFuture<'a> = Pin<Box<dyn Future<Output = Option<(bool, serde_json::Value)>> + Send + 'a>>;
pub type MethodHandler = Box<dyn Fn(u32, serde_json::Value) -> BoxFuture<'static> + Send + Sync>;

pub trait Method {
    const NAME: &'static str;
    type Request: Serialize + for<'de> Deserialize<'de> + Send + Sync;
    type Response: Serialize + for<'de> Deserialize<'de>;
    type Error: Serialize + for<'de> Deserialize<'de>;
}

pub struct GenericMethod;

impl Method for GenericMethod {
    const NAME: &'static str = "generic_do_not_use";
    type Request = serde_json::Value;
    type Response = serde_json::Value;
    type Error = serde_json::Value;
}

#[derive(Debug)]
pub enum Error {
    WebSocket(ws::Error),
    Json(serde_json::Error),
    Io(std::io::Error),
    RecvError(tokio::sync::broadcast::error::RecvError),
}

impl From<ws::Error> for Error {
    fn from(value: ws::Error) -> Self {
        Self::WebSocket(value)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<tokio::sync::broadcast::error::RecvError> for Error {
    fn from(value: tokio::sync::broadcast::error::RecvError) -> Self {
        Self::RecvError(value)
    }
}
