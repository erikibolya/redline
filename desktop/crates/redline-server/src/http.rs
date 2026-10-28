//! Minimal HTTP/1.1 client for the loopback API.
//!
//! Deliberately hand-rolled: redline only ever talks plain HTTP to `127.0.0.1`, so a TLS stack
//! and an async client would be a lot of dependency for four request shapes.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::Value;

#[derive(Debug)]
pub enum ClientError {
    /// Nothing is listening (or the socket died): the usual "app not running" case.
    Transport(String),
    /// The server answered with an error payload we can show the user.
    Http(u16, Value),
    Protocol(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Transport(message) => write!(f, "{message}"),
            ClientError::Http(status, body) => {
                let detail = body
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("request failed");
                write!(f, "HTTP {status}: {detail}")
            }
            ClientError::Protocol(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ClientError {}

pub struct Client {
    host: String,
    port: u16,
    token: String,
}

impl Client {
    pub fn new(base_url: &str, token: impl Into<String>) -> Result<Self, ClientError> {
        let rest = base_url.strip_prefix("http://").ok_or_else(|| {
            ClientError::Protocol(format!("only http:// is supported: {base_url}"))
        })?;
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        let (host, port) = match rest.rsplit_once(':') {
            Some((host, port)) => (
                host.to_string(),
                port.parse()
                    .map_err(|_| ClientError::Protocol(format!("invalid port in {base_url}")))?,
            ),
            None => (rest.to_string(), 80),
        };
        Ok(Self {
            host,
            port,
            token: token.into(),
        })
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<(u16, Vec<u8>), ClientError> {
        let address = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|err| ClientError::Transport(format!("cannot resolve {}: {err}", self.host)))?
            .next()
            .ok_or_else(|| ClientError::Transport("no address found".to_string()))?;

        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|_| {
            ClientError::Transport(format!(
                "cannot reach redline at http://{}:{} — is it running? (start it with `redline serve`)",
                self.host, self.port
            ))
        })?;
        stream.set_read_timeout(Some(Duration::from_secs(120))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(30))).ok();

        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n",
            self.host, self.port, self.token
        );
        if let Some(body) = body {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        request.push_str("\r\n");

        stream
            .write_all(request.as_bytes())
            .and_then(|_| match body {
                Some(body) => stream.write_all(body),
                None => Ok(()),
            })
            .and_then(|_| stream.flush())
            .map_err(|err| ClientError::Transport(format!("request failed: {err}")))?;

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|err| ClientError::Transport(format!("reading the response failed: {err}")))?;
        parse_response(&raw)
    }

    pub fn get(&self, path: &str) -> Result<(u16, Value), ClientError> {
        self.json(self.send("GET", path, None)?)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<(u16, Value), ClientError> {
        let payload =
            serde_json::to_vec(body).map_err(|err| ClientError::Protocol(err.to_string()))?;
        self.json(self.send("POST", path, Some(&payload))?)
    }

    pub fn delete(&self, path: &str) -> Result<(u16, Value), ClientError> {
        self.json(self.send("DELETE", path, None)?)
    }

    fn json(&self, response: (u16, Vec<u8>)) -> Result<(u16, Value), ClientError> {
        let (status, bytes) = response;
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .map_err(|err| ClientError::Protocol(format!("invalid JSON from server: {err}")))?
        };
        if (200..300).contains(&status) {
            Ok((status, value))
        } else {
            Err(ClientError::Http(status, value))
        }
    }
}

/// Split headers from body, honouring `Transfer-Encoding: chunked`.
fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>), ClientError> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            ClientError::Protocol("malformed response (no header terminator)".to_string())
        })?;
    let headers = String::from_utf8_lossy(&raw[..split]).to_string();
    let body = &raw[split + 4..];

    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| ClientError::Protocol("malformed status line".to_string()))?;

    let chunked = headers.lines().any(|line| {
        line.to_ascii_lowercase().starts_with("transfer-encoding:")
            && line.to_ascii_lowercase().contains("chunked")
    });

    if !chunked {
        return Ok((status, body.to_vec()));
    }

    let mut decoded = Vec::new();
    let mut rest = body;
    loop {
        let line_end = rest
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| ClientError::Protocol("truncated chunk header".to_string()))?;
        let size_text = String::from_utf8_lossy(&rest[..line_end]);
        let size = usize::from_str_radix(size_text.trim().split(';').next().unwrap_or("0"), 16)
            .map_err(|_| ClientError::Protocol("invalid chunk size".to_string()))?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size + 2 {
            return Err(ClientError::Protocol("truncated chunk body".to_string()));
        }
        decoded.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
    Ok((status, decoded))
}
