//! HAP-over-IP HTTP transport (plain TLV during pairing, encrypted JSON afterward).

use std::net::SocketAddr;

use bytes::{BufMut, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::crypto::session::EncryptedSession;

use super::TransportError;

pub struct IpConnection {
    stream: TcpStream,
    host: String,
    session: Option<EncryptedSession>,
}

impl IpConnection {
    pub async fn connect(host: &str, port: u16) -> Result<Self, TransportError> {
        let addr: SocketAddr = format!("{host}:{port}").parse().map_err(
            |e: std::net::AddrParseError| TransportError::ConnectionFailed(e.to_string()),
        )?;
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        Ok(Self {
            stream,
            host: host.to_string(),
            session: None,
        })
    }

    pub fn set_session(&mut self, write_key: [u8; 32], read_key: [u8; 32]) {
        self.session = Some(EncryptedSession::new(&write_key, &read_key));
    }

    pub async fn post_tlv(&mut self, path: &str, body: &[u8]) -> Result<Vec<u8>, TransportError> {
        let request = format!(
            "POST {path} HTTP/1.1\r\n\
             Host: {}\r\n\
             Content-Type: application/pairing+tlv8\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n",
            self.host,
            body.len()
        );
        self.stream
            .write_all(request.as_bytes())
            .await
            .map_err(TransportError::Io)?;
        self.stream
            .write_all(body)
            .await
            .map_err(TransportError::Io)?;
        self.read_body().await
    }

    pub async fn get_json(&mut self, path: &str) -> Result<serde_json::Value, TransportError> {
        let request = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: {}\r\n\
             Connection: close\r\n\
             \r\n",
            self.host
        );
        self.stream
            .write_all(request.as_bytes())
            .await
            .map_err(TransportError::Io)?;
        let body = self.read_body().await?;
        let plaintext = self.decrypt_body(&body)?;
        serde_json::from_slice(&plaintext).map_err(|e| TransportError::InvalidResponse(e.to_string()))
    }

    async fn read_body(&mut self) -> Result<Vec<u8>, TransportError> {
        let mut buf = BytesMut::with_capacity(4096);
        let mut tmp = [0u8; 4096];
        loop {
            let n = self
                .stream
                .read(&mut tmp)
                .await
                .map_err(TransportError::Io)?;
            if n == 0 {
                break;
            }
            buf.put_slice(&tmp[..n]);
        }

        let raw = buf.freeze();
        let header_end = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| TransportError::InvalidResponse("missing HTTP header terminator".into()))?;
        let headers = &raw[..header_end];
        let body = &raw[header_end + 4..];

        if let Some(status) = parse_status(headers)
            && !(200..300).contains(&status)
        {
            return Err(TransportError::RequestFailed(format!("HTTP {status}")));
        }

        Ok(body.to_vec())
    }

    fn decrypt_body(&mut self, body: &[u8]) -> Result<Vec<u8>, TransportError> {
        let Some(session) = self.session.as_mut() else {
            return Ok(body.to_vec());
        };
        if body.is_empty() {
            return Ok(Vec::new());
        }
        session
            .decrypt(body)
            .map_err(|e| TransportError::InvalidResponse(e.to_string()))
    }
}

fn parse_status(headers: &[u8]) -> Option<u16> {
    let line = std::str::from_utf8(headers).ok()?;
    let first = line.lines().next()?;
    let mut parts = first.split_whitespace();
    parts.next()?;
    parts.next()?.parse().ok()
}
