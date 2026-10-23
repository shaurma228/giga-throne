//! End-to-end tests of [`throne_api::Client`] against a mock server that
//! speaks the server side of the protocol using the same protocol module.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use throne_api::message::ApiErrorBody;
use throne_api::protocol::{
    self, Direction, FrameOpener, FrameSealer, CLIENT_HELLO_SIZE, FRAME_HEADER_SIZE, RANDOM_SIZE,
};
use throne_api::{ApiKey, Client, Error, Method, Request, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Serves one connection: handshake, then answer requests until the peer
/// goes away or a frame fails authentication.
async fn serve_connection(mut stream: TcpStream, key: ApiKey, served: Arc<AtomicUsize>) {
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
        served.fetch_add(1, Ordering::SeqCst);

        let response = answer(&plaintext);
        let bytes = serde_json::to_vec(&response).expect("encode response");
        let Ok(frame) = sealer.seal(&bytes) else {
            return;
        };
        if stream.write_all(&frame).await.is_err() {
            return;
        }
    }
}

fn answer(plaintext: &[u8]) -> Response {
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
        (Method::Get, "/v1/status") => Response {
            status: 200,
            body: Some(json!({"state": "idle"})),
            error: None,
        },
        (Method::Get, "/v1/echo") => Response {
            status: 200,
            body: request.query.map(|q| json!(q)),
            error: None,
        },
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

/// Starts a mock server on a random loopback port. Returns its address and a
/// counter of accepted connections.
async fn spawn_server(key: ApiKey) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_for_task = accepted.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            accepted_for_task.fetch_add(1, Ordering::SeqCst);
            let key = key.clone();
            tokio::spawn(serve_connection(stream, key, Arc::new(AtomicUsize::new(0))));
        }
    });
    (addr, accepted)
}

fn test_key(byte: u8) -> ApiKey {
    ApiKey::from_text(&protocol_key_text(byte)).expect("valid key")
}

fn protocol_key_text(byte: u8) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([byte; 32])
}

#[tokio::test]
async fn get_returns_body() {
    let key = test_key(1);
    let (addr, _) = spawn_server(key.clone()).await;
    let client = Client::new(addr, key);
    let body = timeout(TEST_TIMEOUT, client.get("/v1/status"))
        .await
        .expect("no hang")
        .expect("request succeeds");
    assert_eq!(body, json!({"state": "idle"}));
}

#[tokio::test]
async fn query_is_sent_to_server() {
    let key = test_key(2);
    let (addr, _) = spawn_server(key.clone()).await;
    let client = Client::new(addr, key);
    let mut query = BTreeMap::new();
    query.insert("group".to_owned(), "3".to_owned());
    let body = timeout(
        TEST_TIMEOUT,
        client.request(Method::Get, "/v1/echo", Some(query), None),
    )
    .await
    .expect("no hang")
    .expect("request succeeds");
    assert_eq!(body, json!({"group": "3"}));
}

#[tokio::test]
async fn error_status_maps_to_api_error() {
    let key = test_key(3);
    let (addr, _) = spawn_server(key.clone()).await;
    let client = Client::new(addr, key);
    let err = timeout(
        TEST_TIMEOUT,
        client.request(Method::Post, "/v1/start", None, Some(json!({}))),
    )
    .await
    .expect("no hang")
    .expect_err("409 is an error");
    match err {
        Error::Api { status, code, .. } => {
            assert_eq!(status, 409);
            assert_eq!(code, "busy");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn sequential_requests_share_one_connection() {
    let key = test_key(4);
    let (addr, accepted) = spawn_server(key.clone()).await;
    let client = Client::new(addr, key);
    for _ in 0..3 {
        let body = timeout(TEST_TIMEOUT, client.get("/v1/status"))
            .await
            .expect("no hang")
            .expect("request succeeds");
        assert_eq!(body["state"], "idle");
    }
    // An error response keeps the connection too.
    let _ = timeout(TEST_TIMEOUT, client.get("/v1/missing"))
        .await
        .expect("no hang");
    let body: Value = timeout(TEST_TIMEOUT, client.get("/v1/status"))
        .await
        .expect("no hang")
        .expect("request succeeds");
    assert_eq!(body["state"], "idle");
    assert_eq!(accepted.load(Ordering::SeqCst), 1, "one TCP connection");
}

#[tokio::test]
async fn wrong_key_fails_instead_of_hanging() {
    let (addr, _) = spawn_server(test_key(5)).await;
    let client = Client::new(addr, test_key(6));
    let result = timeout(TEST_TIMEOUT, client.get("/v1/status"))
        .await
        .expect("request must not hang");
    let err = result.expect_err("wrong key must fail");
    assert!(
        matches!(err, Error::Closed | Error::Io(_)),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn unreachable_server_fails_within_connect_timeout() {
    // Bind and drop to get a port nobody listens on.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    drop(listener);
    let client = Client::new(addr, test_key(7)).connect_timeout(Duration::from_secs(2));
    let result = timeout(TEST_TIMEOUT, client.get("/v1/status"))
        .await
        .expect("request must not hang");
    assert!(result.is_err());
}

#[tokio::test]
async fn invalid_path_is_rejected_before_connecting() {
    let client = Client::new("127.0.0.1:1", test_key(8));
    let err = client
        .request(Method::Get, "/v1/status?x=1", None, None)
        .await
        .expect_err("query in path");
    assert!(matches!(err, Error::InvalidPath(_)));
}
