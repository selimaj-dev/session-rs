pub mod handshake;
pub mod server;
pub mod session;

pub enum SessionFrame<T> {
    Typed(T),
    Binary(Vec<u8>),
}

pub type Result<T> = std::result::Result<T, Error>;

pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidFrame(String),
    ConnectionClosed,
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
