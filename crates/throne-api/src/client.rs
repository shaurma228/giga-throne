//! Throne API client.
//!
//! [`Client`] owns at most one TCP connection to Throne. Requests are strictly
//! sequential: the connection slot is locked for the whole exchange. The
//! connection is opened lazily, reopened after 50 s of idleness (Throne closes
//! it after 60 s), and a failed GET is retried once on a new connection.
//! Any protocol or authentication failure drops the connection.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

use crate::error::{Error, Result};
use crate::key::ApiKey;
use crate::message::{Method, Request, Response};
use crate::protocol::{self, Direction, FrameOpener, FrameSealer, FRAME_HEADER_SIZE, RANDOM_SIZE};

/// Connections idle for longer than this are reopened before use.
const IDLE_RECONNECT: Duration = Duration::from_secs(50);

/// Default time allowed for connecting and completing the handshake.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default time allowed for one request and its response.
///
/// Requests that wait for Throne (see `timeout` in `api.md`) may need more;
/// raise this with [`Client::request_timeout`].
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Client for a running Throne instance in headless mode.
///
/// Creating a client does not connect. The first request does.
pub struct Client {
    addr: String,
    key: ApiKey,
    connect_timeout: Duration,
    request_timeout: Duration,
    slot: Mutex<Option<Connection>>,
}

impl Client {
    /// Creates a client for `addr` (for example `127.0.0.1:9095`) with `key`.
    pub fn new(addr: impl Into<String>, key: ApiKey) -> Self {
        Self {
            addr: addr.into(),
            key,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            slot: Mutex::new(None),
        }
    }

    /// Sets the time allowed for connecting and the handshake.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the time allowed for one request and its response.
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sends one request and returns the response body.
    ///
    /// A 2xx response yields its `body`, or `Value::Null` when there is none.
    /// Any other status yields [`Error::Api`].
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        query: Option<BTreeMap<String, String>>,
        body: Option<Value>,
    ) -> Result<Value> {
        validate_path(path)?;
        let request = Request {
            method,
            path: path.to_owned(),
            query,
            body,
        };
        let plaintext = serde_json::to_vec(&request)?;

        let mut slot = self.slot.lock().await;
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            // A connection taken out of the slot is dropped on any error, so a
            // cancelled request cannot leave a half-used connection behind.
            let mut conn = match slot.take() {
                Some(conn) if conn.last_used.elapsed() <= IDLE_RECONNECT => conn,
                _ => Connection::open(&self.addr, &self.key, self.connect_timeout).await?,
            };
            match timeout(self.request_timeout, conn.exchange(&plaintext)).await {
                Err(_) => return Err(Error::Timeout),
                Ok(Ok(bytes)) => {
                    let response: Response = serde_json::from_slice(&bytes)?;
                    conn.last_used = Instant::now();
                    *slot = Some(conn);
                    return into_body(response);
                }
                Ok(Err(err)) => {
                    let retry = attempt == 1 && method == Method::Get && is_transport(&err);
                    if retry {
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    /// Shorthand for a GET request without query or body.
    pub async fn get(&self, path: &str) -> Result<Value> {
        self.request(Method::Get, path, None, None).await
    }

    /// Like [`Client::request`], but decodes the response body into `T`.
    pub async fn request_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: Option<BTreeMap<String, String>>,
        body: Option<Value>,
    ) -> Result<T> {
        let value = self.request(method, path, query, body).await?;
        Ok(serde_json::from_value(value)?)
    }
}

fn validate_path(path: &str) -> Result<()> {
    if !path.starts_with('/') || path.contains(['?', '#']) {
        return Err(Error::InvalidPath(path.to_owned()));
    }
    Ok(())
}

/// Errors after which a fresh connection may succeed.
fn is_transport(err: &Error) -> bool {
    matches!(err, Error::Io(_) | Error::Closed)
}

fn into_body(response: Response) -> Result<Value> {
    if (200..300).contains(&response.status) {
        return Ok(response.body.unwrap_or(Value::Null));
    }
    let (code, message) = match response.error {
        Some(error) => (error.code, error.message),
        None => ("unknown".to_owned(), String::new()),
    };
    Err(Error::Api {
        status: response.status,
        code,
        message,
    })
}

/// One open, handshaken connection with its frame state.
struct Connection {
    stream: TcpStream,
    sealer: FrameSealer,
    opener: FrameOpener,
    last_used: Instant,
}

impl Connection {
    async fn open(addr: &str, key: &ApiKey, connect_timeout: Duration) -> Result<Self> {
        timeout(connect_timeout, Self::handshake(addr, key))
            .await
            .map_err(|_| Error::Timeout)?
    }

    async fn handshake(addr: &str, key: &ApiKey) -> Result<Self> {
        let mut stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;

        let client_random: [u8; RANDOM_SIZE] = protocol::random_array()?;
        stream
            .write_all(&protocol::client_hello(&client_random))
            .await?;

        let mut server_random = [0u8; RANDOM_SIZE];
        read_exact(&mut stream, &mut server_random).await?;

        let keys = protocol::derive_keys(key, &server_random, &client_random);
        Ok(Self {
            stream,
            sealer: FrameSealer::new(Direction::ClientToServer, keys.c2s),
            opener: FrameOpener::new(Direction::ServerToClient, keys.s2c),
            last_used: Instant::now(),
        })
    }

    /// Sends one request frame and reads the response frame.
    async fn exchange(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let frame = self.sealer.seal(plaintext)?;
        self.stream.write_all(&frame).await?;

        let mut header = [0u8; FRAME_HEADER_SIZE];
        read_exact(&mut self.stream, &mut header).await?;
        // Check the length before reading the body.
        let len = self.opener.check_header(header)? as usize;
        let mut sealed = vec![0u8; len];
        read_exact(&mut self.stream, &mut sealed).await?;
        self.opener.open(header, &sealed)
    }
}

async fn read_exact(stream: &mut TcpStream, buf: &mut [u8]) -> Result<()> {
    match stream.read_exact(buf).await {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Err(Error::Closed),
        Err(err) => Err(err.into()),
    }
}
