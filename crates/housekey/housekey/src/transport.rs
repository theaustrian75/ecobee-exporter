pub mod ip;

pub use ip::IpConnection;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("request failed: {0}")]
    RequestFailed(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("connection closed by accessory")]
    ConnectionClosed,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
