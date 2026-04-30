/// Minimal HTTP/1.1 client.
///
/// On Unix: connects over a Unix domain socket.
/// On Windows: connects over TCP to 127.0.0.1:<port>.
///
/// Each method opens a fresh connection — adequate for the low-request rate of
/// a TUI. Long-lived SSE streaming is handled separately via `sse_connect`.
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode, body::Incoming};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

#[cfg(unix)]
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("connect: {0}")]
    Connect(#[from] std::io::Error),
    #[error("hyper: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("http: {0}")]
    Http(#[from] hyper::http::Error),
    #[error("server returned {0}")]
    Status(StatusCode),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone)]
pub struct Client {
    #[cfg(unix)]
    socket_path: PathBuf,
    #[cfg(windows)]
    port: u16,
}

impl Client {
    #[cfg(unix)]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self { socket_path: socket_path.into() }
    }

    #[cfg(windows)]
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    async fn connect(&self) -> Result<http1::SendRequest<Full<Bytes>>, ClientError> {
        #[cfg(unix)]
        let io = {
            use tokio::net::UnixStream;
            TokioIo::new(UnixStream::connect(&self.socket_path).await?)
        };
        #[cfg(windows)]
        let io = {
            use tokio::net::TcpStream;
            TokioIo::new(TcpStream::connect(("127.0.0.1", self.port)).await?)
        };

        let (sender, conn) = http1::handshake(io).await?;
        tokio::spawn(async move { let _ = conn.await; });
        Ok(sender)
    }

    /// GET — returns (status, body bytes).
    pub async fn get(&self, path: &str) -> Result<(StatusCode, Bytes), ClientError> {
        let req = Request::builder()
            .method(Method::GET)
            .uri(path)
            .header(hyper::header::HOST, "localhost")
            .body(Full::new(Bytes::new()))?;

        let mut sender = self.connect().await?;
        let resp = sender.send_request(req).await?;
        let status = resp.status();
        let body = resp.into_body().collect().await?.to_bytes();
        Ok((status, body))
    }

    /// GET — deserialise body as JSON.
    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        let (status, body) = self.get(path).await?;
        if !status.is_success() {
            return Err(ClientError::Status(status));
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// POST with a JSON body — returns status.
    pub async fn post_json<B: serde::Serialize>(
        &self,
        path: &str,
        payload: &B,
    ) -> Result<StatusCode, ClientError> {
        let body = Bytes::from(serde_json::to_vec(payload)?);
        let req = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(hyper::header::HOST, "localhost")
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(Full::new(body))?;

        let mut sender = self.connect().await?;
        let resp = sender.send_request(req).await?;
        Ok(resp.status())
    }

    /// POST with no body — returns status.
    pub async fn post_empty(&self, path: &str) -> Result<StatusCode, ClientError> {
        let req = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(hyper::header::HOST, "localhost")
            .body(Full::new(Bytes::new()))?;

        let mut sender = self.connect().await?;
        let resp = sender.send_request(req).await?;
        Ok(resp.status())
    }

    /// Open a streaming GET for SSE. Returns the raw `Incoming` body.
    pub async fn sse_connect(&self, path: &str) -> Result<Incoming, ClientError> {
        let req = Request::builder()
            .method(Method::GET)
            .uri(path)
            .header(hyper::header::HOST, "localhost")
            .header(hyper::header::ACCEPT, "text/event-stream")
            .header(hyper::header::CACHE_CONTROL, "no-cache")
            .body(Full::new(Bytes::new()))?;

        #[cfg(unix)]
        let io = {
            use tokio::net::UnixStream;
            TokioIo::new(UnixStream::connect(&self.socket_path).await?)
        };
        #[cfg(windows)]
        let io = {
            use tokio::net::TcpStream;
            TokioIo::new(TcpStream::connect(("127.0.0.1", self.port)).await?)
        };

        let (mut sender, conn) = http1::handshake(io).await?;
        tokio::spawn(async move { let _ = conn.with_upgrades().await; });

        let resp = sender.send_request(req).await?;
        if !resp.status().is_success() {
            return Err(ClientError::Status(resp.status()));
        }
        Ok(resp.into_body())
    }
}
