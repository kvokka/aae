//! A client of the API on the daemon's unix socket: JSON requests and the SSE event stream.

use http_body_util::{BodyExt, Full};
use hyper::{
    Request,
    body::{Bytes, Incoming},
    client::conn::http1,
};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::{fmt, io, path::PathBuf};
use tokio::net::UnixStream;

#[derive(Debug)]
pub enum ClientError {
    /// No daemon on the socket, or the connection broke.
    Io(io::Error),
    /// The daemon answered with an error: `error` is the `{"code", "message", ...}` of the reply.
    Api { status: u16, error: Value },
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "daemon socket: {e}"),
            Self::Api { status, error } => {
                let text = |key: &str| error[key].as_str().unwrap_or_default().to_owned();
                write!(f, "{} ({status} {})", text("message"), text("code"))?;
                // The errors of a config a reload refused, one per line.
                (error["errors"].as_array().into_iter().flatten())
                    .try_for_each(|e| write!(f, "\n{}", e.as_str().unwrap_or_default()))
            }
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<hyper::Error> for ClientError {
    fn from(e: hyper::Error) -> Self {
        Self::Io(io::Error::other(e))
    }
}

pub struct Client {
    socket: PathBuf,
}

impl Client {
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }

    /// Sends `body` (if any) with `method` to `path` and returns the JSON reply of a success.
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        idempotency_key: Option<&str>,
    ) -> Result<Value, ClientError> {
        let response = self.send(method, path, body, idempotency_key, None).await?;
        let bytes = response.into_body().collect().await?.to_bytes();
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Io(io::Error::other(e)))
    }

    /// Opens the event stream from `from` (absent: only new events).
    pub async fn events(&self, from: Option<i64>) -> Result<Events, ClientError> {
        let path = from.map_or_else(|| "/v1/events".to_owned(), |f| format!("/v1/events?from={f}"));
        self.stream(&path).await
    }

    /// Opens an SSE stream at `path`.
    pub async fn stream(&self, path: &str) -> Result<Events, ClientError> {
        let response = self.send("GET", path, None, None, Some("text/event-stream")).await?;
        Ok(Events {
            body: response.into_body(),
            buf: String::new(),
        })
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        idempotency_key: Option<&str>,
        accept: Option<&str>,
    ) -> Result<hyper::Response<Incoming>, ClientError> {
        let stream = UnixStream::connect(&self.socket).await?;
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream)).await?;
        tokio::spawn(connection);
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "aae")
            .header("content-type", "application/json");
        if let Some(key) = idempotency_key {
            request = request.header("idempotency-key", key);
        }
        if let Some(accept) = accept {
            request = request.header("accept", accept);
        }
        let body = Full::new(Bytes::from(body.map(Value::to_string).unwrap_or_default()));
        let request = request.body(body).map_err(io::Error::other)?;
        let response = sender.send_request(request).await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let bytes = response.into_body().collect().await?.to_bytes();
        let reply: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        Err(ClientError::Api {
            status: status.as_u16(),
            error: reply["error"].clone(),
        })
    }
}

/// An SSE event: `id` is the `seq`, `data` the envelope (or the message of `gap`).
#[derive(Debug)]
pub struct SseEvent {
    pub id: Option<String>,
    pub event: String,
    pub data: String,
}

pub struct Events {
    body: Incoming,
    buf: String,
}

impl Events {
    /// The next event; `None` once the daemon ended the stream.
    pub async fn next(&mut self) -> Result<Option<SseEvent>, ClientError> {
        loop {
            if let Some(end) = self.buf.find("\n\n") {
                let block: String = self.buf.drain(..end + 2).collect();
                let mut event = SseEvent {
                    id: None,
                    event: "message".into(),
                    data: String::new(),
                };
                for line in block.lines() {
                    match line.split_once(':').map(|(k, v)| (k, v.strip_prefix(' ').unwrap_or(v))) {
                        Some(("id", v)) => event.id = Some(v.to_owned()),
                        Some(("event", v)) => v.clone_into(&mut event.event),
                        Some(("data", v)) => event.data.push_str(v),
                        _ => {}
                    }
                }
                // A block of comments only is a keep-alive.
                if event.id.is_some() || !event.data.is_empty() {
                    return Ok(Some(event));
                }
                continue;
            }
            let Some(frame) = self.body.frame().await.transpose()? else {
                return Ok(None);
            };
            if let Ok(data) = frame.into_data() {
                self.buf.push_str(&String::from_utf8_lossy(&data));
            }
        }
    }
}
