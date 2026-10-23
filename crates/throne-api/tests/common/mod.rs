//! Scriptable mock of the Throne server side, shared by the client tests.
//!
//! Each accepted connection takes the next script passed to
//! [`MockServer::start`]. A script supplies one [`Action`] per request. When a
//! connection's script runs out, its requests get the default answers of
//! [`answer`], so a test scripts only the requests it cares about.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use throne_api::message::ApiErrorBody;
use throne_api::protocol::{
    self, Direction, FrameOpener, FrameSealer, CLIENT_HELLO_SIZE, FRAME_HEADER_SIZE, RANDOM_SIZE,
};
use throne_api::{ApiKey, Method, Request, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What the server does with one request of a connection.
#[derive(Clone)]
pub enum Action {
    /// Answers with `response` as a normal frame.
    Respond(Response),
    /// Waits for the given time, then answers with `response`.
    RespondAfter(Duration, Response),
    /// Answers with `response` in a frame sealed with the server-to-client key
    /// derived from `key` (the real key when `None`) at frame `counter`.
    /// Then closes the connection.
    RespondSealed {
        key: Option<ApiKey>,
        counter: u64,
        response: Response,
    },
    /// Writes these raw bytes instead of a frame, then closes the connection.
    RespondRaw(Vec<u8>),
    /// Closes the connection without answering.
    CloseAfterRead,
    /// Accepts the connection but never sends the server hello. Only valid as
    /// the first action of a script.
    NoHello,
}

type RequestLog = Arc<Mutex<Vec<(Method, String)>>>;

/// A mock Throne listening on a random loopback port.
pub struct MockServer {
    addr: String,
    accepted: Arc<AtomicUsize>,
    requests: RequestLog,
}

impl MockServer {
    /// Starts a server that speaks the protocol with `key`. Connection `i` uses
    /// `scripts[i]`; connections past the end of `scripts` get no script.
    pub async fn start(key: ApiKey, scripts: Vec<Vec<Action>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let accepted = Arc::new(AtomicUsize::new(0));
        let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
        let accepted_for_task = accepted.clone();
        let requests_for_task = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let index = accepted_for_task.fetch_add(1, Ordering::SeqCst);
                let script: VecDeque<Action> =
                    scripts.get(index).cloned().unwrap_or_default().into();
                tokio::spawn(serve_connection(
                    stream,
                    key.clone(),
                    script,
                    requests_for_task.clone(),
                ));
            }
        });
        Self {
            addr,
            accepted,
            requests,
        }
    }

    /// Address to pass to [`throne_api::Client::new`].
    pub fn addr(&self) -> String {
        self.addr.clone()
    }

    /// Number of TCP connections accepted so far.
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// Method and path of every request read so far, in arrival order.
    pub fn requests(&self) -> Vec<(Method, String)> {
        self.requests.lock().expect("request log").clone()
    }
}

/// Serves one connection: handshake, then one action per request.
async fn serve_connection(
    mut stream: TcpStream,
    key: ApiKey,
    mut script: VecDeque<Action>,
    requests: RequestLog,
) {
    if matches!(script.front(), Some(Action::NoHello)) {
        // Keep the socket open without a hello until the client gives up.
        tokio::time::sleep(Duration::from_secs(3600)).await;
        return;
    }
    let server_random: [u8; RANDOM_SIZE] = protocol::random_array().expect("rng");
    if stream.write_all(&server_random).await.is_err() {
        return;
    }
    let mut hello = [0u8; CLIENT_HELLO_SIZE];
    if stream.read_exact(&mut hello).await.is_err() {
        return;
    }
    let Ok(client_random) = protocol::check_client_hello(&hello) else {
        return;
    };
    let keys = protocol::derive_keys(&key, &server_random, &client_random);
    let mut opener = FrameOpener::new(Direction::ClientToServer, keys.c2s);
    let mut sealer = FrameSealer::new(Direction::ServerToClient, keys.s2c);

    loop {
        let mut header = [0u8; FRAME_HEADER_SIZE];
        if stream.read_exact(&mut header).await.is_err() {
            return;
        }
        let Ok(len) = opener.check_header(header) else {
            return;
        };
        let mut body = vec![0u8; len as usize];
        if stream.read_exact(&mut body).await.is_err() {
            return;
        }
        // A frame that fails authentication ends the connection silently.
        let Ok(plaintext) = opener.open(header, &body) else {
            return;
        };
        if let Ok(request) = serde_json::from_slice::<Request>(&plaintext) {
            requests
                .lock()
                .expect("request log")
                .push((request.method, request.path));
        }

        let action = script
            .pop_front()
            .unwrap_or_else(|| Action::Respond(answer(&plaintext)));
        let keep_open = match action {
            Action::Respond(response) => write_response(&mut stream, &mut sealer, &response).await,
            Action::RespondAfter(delay, response) => {
                tokio::time::sleep(delay).await;
                write_response(&mut stream, &mut sealer, &response).await
            }
            Action::RespondSealed {
                key: wrong_key,
                counter,
                response,
            } => {
                let s2c = match wrong_key {
                    Some(wrong) => {
                        protocol::derive_keys(&wrong, &server_random, &client_random).s2c
                    }
                    None => keys.s2c,
                };
                let bytes = serde_json::to_vec(&response).expect("encode response");
                let frame = protocol::seal_frame(&s2c, counter, &bytes).expect("seal");
                let _ = stream.write_all(&frame).await;
                false
            }
            Action::RespondRaw(bytes) => {
                let _ = stream.write_all(&bytes).await;
                false
            }
            Action::CloseAfterRead | Action::NoHello => false,
        };
        if !keep_open {
            return;
        }
    }
}

async fn write_response(
    stream: &mut TcpStream,
    sealer: &mut FrameSealer,
    response: &Response,
) -> bool {
    let bytes = serde_json::to_vec(response).expect("encode response");
    let frame = sealer.seal(&bytes).expect("seal response");
    stream.write_all(&frame).await.is_ok()
}

/// Default answers for connections whose script has run out.
///
/// `GET /v1/status` returns `{"state": "idle"}`, `GET /v1/echo` returns its
/// query, `POST /v1/start` is `busy`, anything else is `not_found`.
pub fn answer(plaintext: &[u8]) -> Response {
    let request: Request = match serde_json::from_slice(plaintext) {
        Ok(request) => request,
        Err(_) => {
            return Response {
                status: 400,
                body: None,
                error: Some(ApiErrorBody {
                    code: "bad_request".to_owned(),
                    message: "not a request".to_owned(),
                }),
            };
        }
    };
    match (request.method, request.path.as_str()) {
        (Method::Get, "/v1/status") => ok_body(json!({"state": "idle"})),
        (Method::Get, "/v1/echo") => ok_body(json!(request.query.unwrap_or_default())),
        (Method::Post, "/v1/start") => Response {
            status: 409,
            body: None,
            error: Some(ApiErrorBody {
                code: "busy".to_owned(),
                message: "Another profile is starting...".to_owned(),
            }),
        },
        _ => Response {
            status: 404,
            body: None,
            error: Some(ApiErrorBody {
                code: "not_found".to_owned(),
                message: request.path,
            }),
        },
    }
}

/// A 200 response with `body`.
pub fn ok_body(body: Value) -> Response {
    Response {
        status: 200,
        body: Some(body),
        error: None,
    }
}

/// A key whose 32 bytes are all `byte`.
pub fn test_key(byte: u8) -> ApiKey {
    use base64::Engine as _;
    let text = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([byte; 32]);
    ApiKey::from_text(&text).expect("valid key")
}
