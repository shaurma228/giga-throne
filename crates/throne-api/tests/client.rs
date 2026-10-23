//! End-to-end tests of [`throne_api::Client`] against a mock server that
//! speaks the server side of the protocol using the same protocol module.

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use common::{test_key, MockServer};
use serde_json::{json, Value};
use throne_api::{Client, Error, Method};
use tokio::net::TcpListener;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn get_returns_body() {
    let key = test_key(1);
    let server = MockServer::start(key.clone(), Vec::new()).await;
    let client = Client::new(server.addr(), key);
    let body = timeout(TEST_TIMEOUT, client.get("/v1/status"))
        .await
        .expect("no hang")
        .expect("request succeeds");
    assert_eq!(body, json!({"state": "idle"}));
}

#[tokio::test]
async fn query_is_sent_to_server() {
    let key = test_key(2);
    let server = MockServer::start(key.clone(), Vec::new()).await;
    let client = Client::new(server.addr(), key);
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
    let server = MockServer::start(key.clone(), Vec::new()).await;
    let client = Client::new(server.addr(), key);
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
    let server = MockServer::start(key.clone(), Vec::new()).await;
    let client = Client::new(server.addr(), key);
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
    assert_eq!(server.accepted(), 1, "one TCP connection");
}

#[tokio::test]
async fn wrong_key_fails_instead_of_hanging() {
    let server = MockServer::start(test_key(5), Vec::new()).await;
    let client = Client::new(server.addr(), test_key(6));
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
    let server = MockServer::start(test_key(8), Vec::new()).await;
    let client = Client::new(server.addr(), test_key(8));
    let err = client
        .request(Method::Get, "/v1/status?x=1", None, None)
        .await
        .expect_err("query in path");
    assert!(matches!(err, Error::InvalidPath(_)));
    assert_eq!(server.accepted(), 0, "no connection was opened");
}
