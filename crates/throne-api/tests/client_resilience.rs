//! Failure-mode and reconnect tests of [`throne_api::Client`].
//!
//! Each test drives a scripted mock server (see `common`) and asserts on the
//! connections it accepted and the requests it read. Every await is wrapped in
//! a guard timeout so a regression fails instead of hanging.

mod common;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{ok_body, test_key, Action, MockServer};
use serde_json::{json, Value};
use throne_api::types::{ConnectionState, Status};
use throne_api::{Client, Error, Method, Response};
use tokio::net::TcpListener;
use tokio::time::timeout;

/// Upper bound for any single step of a test.
const GUARD: Duration = Duration::from_millis(1500);

async fn within<T>(future: impl Future<Output = T>) -> T {
    timeout(GUARD, future)
        .await
        .expect("step did not finish in time")
}

fn ok_status() -> Response {
    ok_body(json!({"state": "idle"}))
}

fn get_status() -> (Method, String) {
    (Method::Get, "/v1/status".to_owned())
}

fn is_transport_error(err: &Error) -> bool {
    matches!(err, Error::Closed | Error::Io(_))
}

// Idle reconnect

#[tokio::test]
async fn idle_connection_is_reopened_after_threshold() {
    let key = test_key(10);
    let server = MockServer::start(key.clone(), Vec::new()).await;
    let client = Client::new(server.addr(), key).idle_reconnect(Duration::from_millis(100));

    within(client.get("/v1/status"))
        .await
        .expect("first request");
    tokio::time::sleep(Duration::from_millis(200)).await;
    within(client.get("/v1/status"))
        .await
        .expect("second request");

    assert_eq!(server.accepted(), 2, "idle connection was replaced");
}

#[tokio::test]
async fn busy_connection_is_reused_below_threshold() {
    let key = test_key(11);
    let server = MockServer::start(key.clone(), Vec::new()).await;
    let client = Client::new(server.addr(), key).idle_reconnect(Duration::from_millis(100));

    within(client.get("/v1/status"))
        .await
        .expect("first request");
    within(client.get("/v1/status"))
        .await
        .expect("second request");

    assert_eq!(server.accepted(), 1, "connection was reused");
}

// Retry of GET

#[tokio::test]
async fn get_is_retried_once_on_a_new_connection() {
    let key = test_key(12);
    let server = MockServer::start(key.clone(), vec![vec![Action::CloseAfterRead]]).await;
    let client = Client::new(server.addr(), key);

    let body = within(client.get("/v1/status"))
        .await
        .expect("retry succeeds");

    assert_eq!(body, json!({"state": "idle"}));
    assert_eq!(server.accepted(), 2);
    assert_eq!(server.requests(), vec![get_status(), get_status()]);
}

#[tokio::test]
async fn get_gives_up_after_one_retry() {
    let key = test_key(13);
    let server = MockServer::start(
        key.clone(),
        vec![vec![Action::CloseAfterRead], vec![Action::CloseAfterRead]],
    )
    .await;
    let client = Client::new(server.addr(), key);

    let err = within(client.get("/v1/status"))
        .await
        .expect_err("both connections drop");

    assert!(is_transport_error(&err), "unexpected error: {err:?}");
    assert_eq!(server.accepted(), 2, "exactly one retry");
    assert_eq!(server.requests(), vec![get_status(), get_status()]);
}

#[tokio::test]
async fn non_get_requests_are_never_retried() {
    let cases = [
        (Method::Post, "/v1/start", Some(json!({}))),
        (Method::Put, "/v1/profiles/1", Some(json!({"name": "x"}))),
        (Method::Delete, "/v1/profiles/1", None),
    ];
    for (method, path, body) in cases {
        let key = test_key(14);
        let server = MockServer::start(key.clone(), vec![vec![Action::CloseAfterRead]]).await;
        let client = Client::new(server.addr(), key);

        let err = within(client.request(method, path, None, body))
            .await
            .expect_err("connection dropped");

        assert!(
            is_transport_error(&err),
            "{method:?}: unexpected error: {err:?}"
        );
        assert_eq!(server.accepted(), 1, "{method:?} must not reconnect");
        assert_eq!(server.requests(), vec![(method, path.to_owned())]);
    }
}

// Timeouts

#[tokio::test]
async fn request_timeout_drops_the_connection() {
    let key = test_key(15);
    let server = MockServer::start(
        key.clone(),
        vec![vec![Action::RespondAfter(
            Duration::from_secs(1),
            ok_status(),
        )]],
    )
    .await;
    let client = Client::new(server.addr(), key).request_timeout(Duration::from_millis(200));

    let err = within(client.get("/v1/status"))
        .await
        .expect_err("server is too slow");
    assert!(matches!(err, Error::Timeout), "unexpected error: {err:?}");

    // The timed-out connection is not reused: its late answer would be out of step.
    let body = within(client.get("/v1/status"))
        .await
        .expect("next request");
    assert_eq!(body["state"], "idle");
    assert_eq!(server.accepted(), 2);
}

#[tokio::test]
async fn silent_server_times_out_within_connect_timeout() {
    let key = test_key(16);
    let server = MockServer::start(key.clone(), vec![vec![Action::NoHello]]).await;
    let client = Client::new(server.addr(), key).connect_timeout(Duration::from_millis(200));

    let started = Instant::now();
    let err = within(client.get("/v1/status"))
        .await
        .expect_err("no hello");

    assert!(matches!(err, Error::Timeout), "unexpected error: {err:?}");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn refused_connection_fails_quickly() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    drop(listener);
    let client = Client::new(addr, test_key(17)).connect_timeout(Duration::from_secs(1));

    let started = Instant::now();
    let err = within(client.get("/v1/status"))
        .await
        .expect_err("nothing listens");

    assert!(matches!(err, Error::Io(_)), "unexpected error: {err:?}");
    assert!(started.elapsed() < Duration::from_secs(1));
}

// Frame failures

#[tokio::test]
async fn oversized_server_frame_is_rejected_before_reading_body() {
    let key = test_key(18);
    // Header announces one byte more than the server-to-client limit, and no body follows.
    let header = ((64u32 << 20) + 1).to_be_bytes().to_vec();
    let server = MockServer::start(key.clone(), vec![vec![Action::RespondRaw(header)]]).await;
    let client = Client::new(server.addr(), key);

    let err = within(client.get("/v1/status"))
        .await
        .expect_err("frame is too large");

    assert!(
        matches!(err, Error::FrameTooLarge { .. }),
        "unexpected error: {err:?}"
    );
    assert_eq!(server.accepted(), 1, "a protocol error is not retried");
}

#[tokio::test]
async fn undersized_server_frame_is_rejected() {
    let key = test_key(19);
    let header = 5u32.to_be_bytes().to_vec();
    let server = MockServer::start(key.clone(), vec![vec![Action::RespondRaw(header)]]).await;
    let client = Client::new(server.addr(), key);

    let err = within(client.get("/v1/status"))
        .await
        .expect_err("frame is too short");

    assert!(
        matches!(err, Error::FrameTooShort { len: 5 }),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn response_sealed_with_wrong_key_fails_authentication() {
    let key = test_key(20);
    let server = MockServer::start(
        key.clone(),
        vec![vec![Action::RespondSealed {
            key: Some(test_key(99)),
            counter: 0,
            response: ok_status(),
        }]],
    )
    .await;
    let client = Client::new(server.addr(), key);

    let err = within(client.get("/v1/status"))
        .await
        .expect_err("wrong key");
    assert!(matches!(err, Error::FrameAuth), "unexpected error: {err:?}");

    let body = within(client.get("/v1/status"))
        .await
        .expect("new connection");
    assert_eq!(body["state"], "idle");
    assert_eq!(server.accepted(), 2, "the failed connection was dropped");
}

#[tokio::test]
async fn replayed_response_counter_fails_authentication() {
    let key = test_key(21);
    let server = MockServer::start(
        key.clone(),
        vec![vec![
            Action::Respond(ok_status()),
            // Counter 0 again, but the client expects 1.
            Action::RespondSealed {
                key: None,
                counter: 0,
                response: ok_status(),
            },
        ]],
    )
    .await;
    let client = Client::new(server.addr(), key);

    within(client.get("/v1/status"))
        .await
        .expect("first answer is valid");
    let err = within(client.get("/v1/status"))
        .await
        .expect_err("replayed frame");
    assert!(matches!(err, Error::FrameAuth), "unexpected error: {err:?}");

    let body = within(client.get("/v1/status"))
        .await
        .expect("new connection");
    assert_eq!(body["state"], "idle");
    assert_eq!(server.accepted(), 2);
}

// Responses

#[tokio::test]
async fn error_without_error_object_has_unknown_code() {
    let key = test_key(22);
    let response = Response {
        status: 500,
        body: None,
        error: None,
    };
    let server = MockServer::start(key.clone(), vec![vec![Action::Respond(response)]]).await;
    let client = Client::new(server.addr(), key);

    let err = within(client.get("/v1/status"))
        .await
        .expect_err("500 is an error");

    match err {
        Error::Api { status, code, .. } => {
            assert_eq!(status, 500);
            assert_eq!(code, "unknown");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn success_without_body_yields_null() {
    let key = test_key(23);
    let response = Response {
        status: 204,
        body: None,
        error: None,
    };
    let server = MockServer::start(key.clone(), vec![vec![Action::Respond(response)]]).await;
    let client = Client::new(server.addr(), key);

    let body = within(client.get("/v1/status"))
        .await
        .expect("204 is success");

    assert_eq!(body, Value::Null);
}

#[tokio::test]
async fn invalid_paths_are_rejected_without_connecting() {
    let key = test_key(24);
    let server = MockServer::start(key.clone(), Vec::new()).await;
    let client = Client::new(server.addr(), key);

    for path in ["v1/status", "/v1/x?y=1", "/v1/x#frag", ""] {
        let err = within(client.get(path)).await.expect_err("invalid path");
        assert!(matches!(err, Error::InvalidPath(_)), "{path}: {err:?}");
    }
    assert_eq!(server.accepted(), 0);
}

// Concurrency

#[tokio::test]
async fn concurrent_requests_run_one_at_a_time_on_one_connection() {
    let key = test_key(25);
    let server = MockServer::start(key.clone(), Vec::new()).await;
    let client = Arc::new(Client::new(server.addr(), key));

    let mut tasks = Vec::new();
    for n in 0..10 {
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            let mut query = BTreeMap::new();
            query.insert("n".to_owned(), n.to_string());
            client
                .request(Method::Get, "/v1/echo", Some(query), None)
                .await
                .map(|body| (n, body))
        }));
    }
    for task in tasks {
        let (n, body) = within(task)
            .await
            .expect("task finishes")
            .expect("request succeeds");
        assert_eq!(
            body,
            json!({"n": n.to_string()}),
            "each caller gets its own answer"
        );
    }

    assert_eq!(server.accepted(), 1, "one connection for all requests");
    assert_eq!(server.requests().len(), 10);
}

// Typed decoding

const STATUS_JSON: &str = r#"{
  "state": "idle",
  "profile": null,
  "last_profile_id": 12,
  "tun": true,
  "tun_conflict": null,
  "system_proxy": false,
  "route": {"id": 1, "name": "Default", "active": true},
  "kill_switch": "off",
  "core": true,
  "inbound": {"enabled": true, "address": "127.0.0.1", "port": 2080},
  "version": "1.4.0",
  "capabilities": {"tun": true, "kill_switch": true, "headless": false},
  "restart_needed": false,
  "degraded": null
}"#;

#[tokio::test]
async fn request_json_decodes_status() {
    let key = test_key(26);
    let body: Value = serde_json::from_str(STATUS_JSON).expect("valid JSON");
    let server = MockServer::start(key.clone(), vec![vec![Action::Respond(ok_body(body))]]).await;
    let client = Client::new(server.addr(), key);

    let status: Status = within(client.request_json(Method::Get, "/v1/status", None, None))
        .await
        .expect("status decodes");

    assert_eq!(status.state, ConnectionState::Idle);
    assert!(status.profile.is_none());
    assert_eq!(status.last_profile_id, 12);
    assert_eq!(status.inbound.port, 2080);
    assert_eq!(status.version, "1.4.0");
}
