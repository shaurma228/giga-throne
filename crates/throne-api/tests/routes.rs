//! Tests of the routing-profile types, codecs and requests (`throne_api::routes`,
//! `throne_api::types`), against `docs/throne-api/routes-proposal.md`.
//!
//! Request tests drive the scripted mock server (see `common`) and assert on
//! the requests it read, bodies included.

mod common;

use std::future::Future;
use std::time::Duration;

use common::{ok_body, test_key, Action, MockServer};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use throne_api::types::{
    AddRouteResponse, GetRouteResponse, Outbound, Route, RouteFetchResult, RouteRule, RouteShare,
    RuleSetsResponse, Status, UpdateRouteResponse,
};
use throne_api::{
    decode_remote_route_link, decode_route_link, encode_route_link, Client, Error, Method, Request,
    Response,
};
use tokio::time::timeout;

/// Upper bound for any single step of a test.
const GUARD: Duration = Duration::from_millis(1500);

/// Bypass Russia, from a real `throne://route/` link.
const BYPASS_RUSSIA_LINK: &str = "throne://route/eyJkZWZhdWx0X291dGJvdW5kIjoicHJveHkiLCJraW5kIjoidGhyb25lLXJvdXRlLXByb2ZpbGUiLCJuYW1lIjoiQnlwYXNzIFJ1c3NpYSIsInJ1bGVzIjpbeyJhY3Rpb24iOiJoaWphY2stZG5zIiwibmFtZSI6ImltcG9ydGVkIHJ1bGUgIzEiLCJwcm90b2NvbCI6ImRucyIsInR5cGUiOiJjdXN0b20ifSx7ImFjdGlvbiI6InJvdXRlIiwiaXBfaXNfcHJpdmF0ZSI6dHJ1ZSwibmFtZSI6ImltcG9ydGVkIHJ1bGUgIzIiLCJvdXRib3VuZCI6ImRpcmVjdCIsInR5cGUiOiJjdXN0b20ifSx7ImFjdGlvbiI6InJvdXRlIiwibmFtZSI6ImltcG9ydGVkIHJ1bGUgIzMiLCJvdXRib3VuZCI6ImRpcmVjdCIsInJ1bGVfc2V0IjpbImdlb2lwLXJ1IiwiZ2Vvc2l0ZS1ydSIsImdlb3NpdGUtY2F0ZWdvcnktcnUiLCJnZW9zaXRlLXJ1LWF2YWlsYWJsZS1vbmx5LWluc2lkZSJdLCJ0eXBlIjoiY3VzdG9tIn1dLCJ2IjoxfQ";

/// Four remote profiles, from a real `throne://remoteroute/` link.
const REMOTE_ROUTES_LINK: &str = "throne://remoteroute/aHR0cHM6Ly9yYXcuZ2l0aHVidXNlcmNvbnRlbnQuY29tL3Rocm9uZXByb2ovcm91dGVwcm9maWxlcy9wcm9maWxlL0J5cGFzc19SdXNzaWEjQnlwYXNzIFJ1c3NpYQpodHRwczovL3Jhdy5naXRodWJ1c2VyY29udGVudC5jb20vdGhyb25lcHJvai9yb3V0ZXByb2ZpbGVzL3Byb2ZpbGUvUHJveHlfUnVzc2lhX0Jsb2NrZWQjUHJveHkgUnVzc2lhIEJsb2NrZWQKaHR0cHM6Ly9yYXcuZ2l0aHVidXNlcmNvbnRlbnQuY29tL3Rocm9uZXByb2ovcm91dGVwcm9maWxlcy9wcm9maWxlL1Byb3h5X0FudGl6YXByZXQjUHJveHkgQW50aXphcHJldApodHRwczovL3Jhdy5naXRodWJ1c2VyY29udGVudC5jb20vdGhyb25lcHJvai9yb3V0ZXByb2ZpbGVzL3Byb2ZpbGUvUHJveHlfUmVmaWx0ZXIjUHJveHkgUmVmaWx0ZXI";

/// The proposal's Route example.
const ROUTE_JSON: &str = r#"{"id": 2, "name": "Bypass Russia", "active": false, "remote": true,
 "remote_url": "https://raw.githubusercontent.com/throneproj/routeprofiles/profile/Bypass_Russia",
 "auto_update": true, "updated_at": 1760000000, "raw": false, "rule_count": 3}"#;

/// The proposal's share object example.
const SHARE_JSON: &str = r#"{
  "kind": "throne-route-profile", "v": 1, "name": "Bypass Russia",
  "default_outbound": "proxy",
  "rules": [
    {"name": "dns", "type": "custom", "action": "hijack-dns", "protocol": "dns"},
    {"name": "private", "type": "custom", "action": "route", "ip_is_private": true, "outbound": "direct"},
    {"name": "ru", "type": "custom", "action": "route", "outbound": "direct",
     "rule_set": ["geoip-ru", "geosite-ru", "geosite-category-ru"]}
  ]
}"#;

/// The `api.md` Status example, returned by `PUT /v1/routes/active`.
const STATUS_JSON: &str = r#"{
  "state": "running",
  "profile": {"id": 12, "name": "DE Frankfurt", "...": "a Profile object"},
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

fn parse<T: DeserializeOwned>(json: &str) -> T {
    serde_json::from_str(json).expect("fixture parses")
}

/// The Bypass Russia share object, decoded from its link.
fn bypass_russia() -> RouteShare {
    decode_route_link(BYPASS_RUSSIA_LINK).expect("fixture link decodes")
}

async fn within<T>(future: impl Future<Output = T>) -> T {
    timeout(GUARD, future)
        .await
        .expect("step did not finish in time")
}

/// A client whose first connection answers `responses` in order.
async fn client_with(responses: Vec<Action>) -> (MockServer, Client) {
    let key = test_key(21);
    let server = MockServer::start(key.clone(), vec![responses]).await;
    let client = Client::new(server.addr(), key);
    (server, client)
}

fn respond(body: Value) -> Action {
    Action::Respond(ok_body(body))
}

/// An error answer with `status` and `code`.
fn api_error(status: u16, code: &str) -> Action {
    Action::Respond(Response {
        status,
        body: None,
        error: Some(throne_api::ApiErrorBody {
            code: code.to_owned(),
            message: format!("{code} from the mock"),
        }),
    })
}

fn sent(method: Method, path: &str, body: Option<Value>) -> Request {
    Request {
        method,
        path: path.to_owned(),
        query: None,
        body,
    }
}

fn expect_api_error(err: Error, status: u16, code: &str) {
    match err {
        Error::Api {
            status: got_status,
            code: got_code,
            message,
        } => {
            assert_eq!(got_status, status);
            assert_eq!(got_code, code);
            assert_eq!(message, format!("{code} from the mock"));
        }
        other => panic!("expected Error::Api, got {other:?}"),
    }
}

// Objects and bodies: the proposal's examples

#[test]
fn route_old_three_field_json_parses() {
    let route: Route = parse(r#"{"id": 1, "name": "Default", "active": true}"#);
    assert_eq!(route.id, 1);
    assert_eq!(route.name, "Default");
    assert!(route.active);
    assert!(!route.remote);
    assert_eq!(route.remote_url, "");
    assert!(!route.auto_update);
    assert_eq!(route.updated_at, 0);
    assert!(!route.raw);
    assert_eq!(route.rule_count, 0);
}

#[test]
fn route_proposal_example_parses() {
    let route: Route = parse(ROUTE_JSON);
    assert_eq!(route.id, 2);
    assert_eq!(route.name, "Bypass Russia");
    assert!(!route.active);
    assert!(route.remote);
    assert_eq!(
        route.remote_url,
        "https://raw.githubusercontent.com/throneproj/routeprofiles/profile/Bypass_Russia"
    );
    assert!(route.auto_update);
    assert_eq!(route.updated_at, 1760000000);
    assert!(!route.raw);
    assert_eq!(route.rule_count, 3);
}

#[test]
fn share_proposal_example_parses() {
    let share: RouteShare = parse(SHARE_JSON);
    assert_eq!(share.kind, "throne-route-profile");
    assert_eq!(share.v, 1);
    assert_eq!(share.name, "Bypass Russia");
    assert_eq!(share.default_outbound, Outbound::Proxy);
    assert_eq!(share.rules.len(), 3);
    assert_eq!(share.rules[0].action, "hijack-dns");
    assert_eq!(share.rules[0].protocol.as_deref(), Some("dns"));
    assert_eq!(share.rules[1].ip_is_private, Some(true));
    assert_eq!(share.rules[1].outbound, Some(Outbound::Direct));
    assert_eq!(
        share.rules[2].rule_set.as_deref(),
        Some(
            &[
                "geoip-ru".to_owned(),
                "geosite-ru".to_owned(),
                "geosite-category-ru".to_owned()
            ][..]
        )
    );
    assert!(share.extra.is_empty());
}

#[test]
fn get_route_response_example_parses() {
    let json = format!(
        r#"{{"route": {ROUTE_JSON}, "profile": {SHARE_JSON}, "link": "{BYPASS_RUSSIA_LINK}"}}"#
    );
    let resp: GetRouteResponse = parse(&json);
    assert_eq!(resp.route.id, 2);
    assert_eq!(resp.profile.rules.len(), 3);
    assert_eq!(resp.link, BYPASS_RUSSIA_LINK);
}

#[test]
fn post_routes_request_examples_parse() {
    // The three bodies of POST /v1/routes, with the fixture link and profile.
    let by_link: Value = json!({"link": BYPASS_RUSSIA_LINK});
    assert_eq!(by_link["link"], BYPASS_RUSSIA_LINK);

    let by_profile: Value = json!({"profile": parse::<Value>(SHARE_JSON)});
    let profile: RouteShare = serde_json::from_value(by_profile["profile"].clone())
        .expect("profile body parses as a share object");
    assert_eq!(profile.name, "Bypass Russia");

    let by_remote: Value = json!({"remote": REMOTE_ROUTES_LINK, "auto_update": true});
    assert_eq!(by_remote["auto_update"], true);
    assert_eq!(
        decode_remote_route_link(by_remote["remote"].as_str().expect("string"))
            .expect("remote decodes")
            .len(),
        4
    );
}

#[test]
fn post_routes_response_example_parses() {
    let json = format!(
        r#"{{"routes": [{ROUTE_JSON}], "results": [{{"id": 3, "ok": true}}, {{"id": 4, "ok": false, "error": "fetch failed"}}], "warnings": ""}}"#
    );
    let resp: AddRouteResponse = parse(&json);
    assert_eq!(resp.routes.len(), 1);
    assert_eq!(
        resp.results,
        vec![
            RouteFetchResult {
                id: 3,
                ok: true,
                error: None,
                updated_at: None
            },
            RouteFetchResult {
                id: 4,
                ok: false,
                error: Some("fetch failed".to_owned()),
                updated_at: None
            },
        ]
    );
    assert_eq!(resp.warnings, "");
}

#[test]
fn put_routes_examples_parse() {
    let by_profile: Value = json!({"profile": parse::<Value>(SHARE_JSON)});
    let share: RouteShare = serde_json::from_value(by_profile["profile"].clone()).expect("share");
    assert_eq!(share.default_outbound, Outbound::Proxy);

    let renamed: Value = json!({"name": "Renamed"});
    assert_eq!(renamed["name"], "Renamed");
    let auto: Value = json!({"auto_update": false});
    assert_eq!(auto["auto_update"], false);

    let resp: UpdateRouteResponse = parse(&format!(r#"{{"route": {ROUTE_JSON}, "warnings": ""}}"#));
    assert_eq!(resp.route.id, 2);
}

#[test]
fn post_update_result_examples_parse() {
    let ok: RouteFetchResult = parse(r#"{"id": 2, "ok": true, "updated_at": 1760000000}"#);
    assert_eq!(ok.id, 2);
    assert!(ok.ok);
    assert_eq!(ok.updated_at, Some(1760000000));
    assert_eq!(ok.error, None);

    let failed: RouteFetchResult = parse(r#"{"id": 2, "ok": false, "error": "..."}"#);
    assert!(!failed.ok);
    assert_eq!(failed.error.as_deref(), Some("..."));
}

#[test]
fn get_rule_sets_example_parses() {
    let resp: RuleSetsResponse =
        parse(r#"{"rule_sets": ["geoip-ru", "geoip-ru-blocked", "geosite-ru"]}"#);
    assert_eq!(resp.rule_sets.len(), 3);
    assert_eq!(resp.rule_sets[1], "geoip-ru-blocked");
}

// Codecs: real-world fixtures and round trips

#[test]
fn bypass_russia_link_decodes() {
    let share = bypass_russia();
    assert_eq!(share.name, "Bypass Russia");
    assert_eq!(share.kind, "throne-route-profile");
    assert_eq!(share.v, 1);
    assert_eq!(share.default_outbound, Outbound::Proxy);
    assert_eq!(share.rules.len(), 3);

    let dns = &share.rules[0];
    assert_eq!(dns.name, "imported rule #1");
    assert_eq!(dns.action, "hijack-dns");
    assert_eq!(dns.protocol.as_deref(), Some("dns"));
    assert_eq!(dns.outbound, None);

    let private = &share.rules[1];
    assert_eq!(private.ip_is_private, Some(true));
    assert_eq!(private.outbound, Some(Outbound::Direct));

    let ru = &share.rules[2];
    assert_eq!(ru.rule_set.as_ref().map(Vec::len), Some(4));
    assert_eq!(
        ru.rule_set.as_ref().expect("rule_set")[3],
        "geosite-ru-available-only-inside"
    );
}

#[test]
fn remote_routes_link_decodes() {
    let entries = decode_remote_route_link(REMOTE_ROUTES_LINK).expect("remote link decodes");
    let base = "https://raw.githubusercontent.com/throneproj/routeprofiles/profile/";
    assert_eq!(
        entries,
        vec![
            (format!("{base}Bypass_Russia"), "Bypass Russia".to_owned()),
            (
                format!("{base}Proxy_Russia_Blocked"),
                "Proxy Russia Blocked".to_owned()
            ),
            (
                format!("{base}Proxy_Antizapret"),
                "Proxy Antizapret".to_owned()
            ),
            (format!("{base}Proxy_Refilter"), "Proxy Refilter".to_owned()),
        ]
    );
}

#[test]
fn route_link_round_trip_is_identical() {
    let share = bypass_russia();
    let encoded = encode_route_link(&share);
    assert!(encoded.starts_with("throne://route/"));
    assert!(!encoded.ends_with('='), "links carry no padding");
    let decoded = decode_route_link(&encoded).expect("re-encoded link decodes");
    assert_eq!(decoded, share);

    // Encoding again gives a link that decodes to the same profile.
    let again = decode_route_link(&encode_route_link(&decoded)).expect("decodes");
    assert_eq!(again, share);
}

#[test]
fn route_link_without_changes_keeps_every_field() {
    let share = bypass_russia();
    let original: Value = serde_json::from_slice(&base64_url_decode(
        BYPASS_RUSSIA_LINK.trim_start_matches("throne://route/"),
    ))
    .expect("fixture JSON");
    assert_eq!(serde_json::to_value(&share).expect("to_value"), original);
}

#[test]
fn padded_route_link_is_accepted() {
    let payload = BYPASS_RUSSIA_LINK.trim_start_matches("throne://route/");
    let padding = "=".repeat((4 - payload.len() % 4) % 4);
    let padded = format!("throne://route/{payload}{padding}");
    assert_eq!(decode_route_link(&padded).expect("padded"), bypass_russia());
}

#[test]
fn unknown_fields_survive_a_round_trip() {
    let mut value: Value = parse(SHARE_JSON);
    value["dns"] = json!({"servers": ["1.1.1.1"], "strategy": "ipv4_only"});
    value["future_top_level"] = json!({"nested": [1, 2, 3]});
    value["rules"][1]["future_rule_field"] = json!("kept");
    value["rules"][2]["outbound"] = json!("future-outbound");

    let share: RouteShare = serde_json::from_value(value.clone()).expect("parses");
    assert!(share.extra.contains_key("dns"));
    assert!(share.extra.contains_key("future_top_level"));
    assert_eq!(
        share.rules[1].extra.get("future_rule_field"),
        Some(&json!("kept"))
    );
    assert_eq!(
        share.rules[2].outbound,
        Some(Outbound::Unknown("future-outbound".to_owned()))
    );

    let link = encode_route_link(&share);
    let decoded = decode_route_link(&link).expect("decodes");
    assert_eq!(decoded, share);
    assert_eq!(serde_json::to_value(&decoded).expect("to_value"), value);
}

#[test]
fn outbound_names_map_and_survive() {
    for (text, outbound) in [
        ("proxy", Outbound::Proxy),
        ("direct", Outbound::Direct),
        ("block", Outbound::Block),
        ("warp-bypass", Outbound::WarpBypass),
        ("future", Outbound::Unknown("future".to_owned())),
    ] {
        let parsed: Outbound = serde_json::from_value(json!(text)).expect("outbound");
        assert_eq!(parsed, outbound);
        assert_eq!(
            serde_json::to_value(&parsed).expect("to_value"),
            json!(text)
        );
    }
    assert_eq!(Outbound::default(), Outbound::Proxy);
}

#[test]
fn port_numbers_are_read_as_text() {
    let rule: RouteRule = parse(
        r#"{"action": "route", "outbound": "warp-bypass", "port": [80, "443"], "port_range": ["1000:2000"]}"#,
    );
    assert_eq!(rule.port, Some(vec!["80".to_owned(), "443".to_owned()]));
    assert_eq!(rule.port_range, Some(vec!["1000:2000".to_owned()]));
    assert_eq!(rule.outbound, Some(Outbound::WarpBypass));
    assert_eq!(rule.r#type, "custom");
    assert!(!rule.disabled);

    let value = serde_json::to_value(&rule).expect("to_value");
    assert_eq!(value["port"], json!([80, 443]));
    assert!(value.get("domain").is_none(), "absent fields stay absent");
}

#[test]
fn port_numbers_round_trip_as_numbers() {
    let rule: RouteRule = parse(r#"{"action": "route", "port": [80, 443], "source_port": [5000]}"#);
    let value = serde_json::to_value(&rule).expect("to_value");
    assert_eq!(value["port"], json!([80, 443]));
    assert_eq!(value["source_port"], json!([5000]));
}

#[test]
fn numeric_port_text_is_written_as_number() {
    let rule: RouteRule = parse(r#"{"action": "route", "port": ["8080"]}"#);
    let value = serde_json::to_value(&rule).expect("to_value");
    assert_eq!(value["port"], json!([8080]));
}

#[test]
fn port_range_stays_a_string() {
    let rule: RouteRule =
        parse(r#"{"action": "route", "port_range": ["1000:2000"], "source_port_range": ["1:2"]}"#);
    let value = serde_json::to_value(&rule).expect("to_value");
    assert_eq!(value["port_range"], json!(["1000:2000"]));
    assert_eq!(value["source_port_range"], json!(["1:2"]));
}

#[test]
fn minimal_share_gets_defaults() {
    let share: RouteShare = parse(r#"{"name": "Mine"}"#);
    assert_eq!(share.kind, "throne-route-profile");
    assert_eq!(share.v, 1);
    assert_eq!(share.default_outbound, Outbound::Proxy);
    assert!(share.rules.is_empty());
    assert_eq!(
        serde_json::to_value(&share).expect("to_value"),
        json!({"kind": "throne-route-profile", "v": 1, "name": "Mine", "default_outbound": "proxy", "rules": []})
    );
}

#[test]
fn malformed_links_are_rejected() {
    assert!(matches!(
        decode_route_link(REMOTE_ROUTES_LINK),
        Err(Error::Protocol(_))
    ));
    assert!(matches!(
        decode_route_link("throne://route/!!not base64!!"),
        Err(Error::Protocol(_))
    ));
    assert!(matches!(
        decode_remote_route_link(BYPASS_RUSSIA_LINK),
        Err(Error::Protocol(_))
    ));
    let no_name = format!(
        "throne://remoteroute/{}",
        base64_url_encode(b"https://example.com/profile")
    );
    assert!(matches!(
        decode_remote_route_link(&no_name),
        Err(Error::Protocol(_))
    ));
}

fn base64_url_decode(text: &str) -> Vec<u8> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    URL_SAFE_NO_PAD.decode(text).expect("base64url")
}

fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    URL_SAFE_NO_PAD.encode(bytes)
}

// Requests: method, path and body, and error mapping

#[tokio::test]
async fn routes_lists_profiles() {
    let (server, client) = client_with(vec![respond(
        json!({"routes": [parse::<Value>(ROUTE_JSON)]}),
    )])
    .await;

    let routes = within(client.routes()).await.expect("routes");

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].id, 2);
    assert_eq!(
        server.recorded(),
        vec![sent(Method::Get, "/v1/routes", None)]
    );
}

#[tokio::test]
async fn route_fetches_one_profile() {
    let body = json!({"route": parse::<Value>(ROUTE_JSON), "profile": parse::<Value>(SHARE_JSON), "link": BYPASS_RUSSIA_LINK});
    let (server, client) = client_with(vec![respond(body)]).await;

    let resp = within(client.route(2)).await.expect("route");

    assert_eq!(resp.route.name, "Bypass Russia");
    assert_eq!(resp.profile.rules.len(), 3);
    assert_eq!(
        server.recorded(),
        vec![sent(Method::Get, "/v1/routes/2", None)]
    );
}

#[tokio::test]
async fn add_route_link_sends_link() {
    let body = json!({"routes": [parse::<Value>(ROUTE_JSON)], "warnings": ""});
    let (server, client) = client_with(vec![respond(body)]).await;

    let resp = within(client.add_route_link(BYPASS_RUSSIA_LINK))
        .await
        .expect("add");

    assert_eq!(resp.routes.len(), 1);
    assert!(resp.results.is_empty());
    assert_eq!(
        server.recorded(),
        vec![sent(
            Method::Post,
            "/v1/routes",
            Some(json!({"link": BYPASS_RUSSIA_LINK}))
        )]
    );
}

#[tokio::test]
async fn add_route_profile_sends_share_object() {
    let share = bypass_russia();
    let (server, client) = client_with(vec![respond(json!({"routes": [], "warnings": ""}))]).await;

    within(client.add_route_profile(&share)).await.expect("add");

    assert_eq!(
        server.recorded(),
        vec![sent(
            Method::Post,
            "/v1/routes",
            Some(json!({"profile": serde_json::to_value(&share).expect("to_value")}))
        )]
    );
}

#[tokio::test]
async fn add_remote_routes_waits_for_fetches() {
    let body = json!({
        "routes": [parse::<Value>(ROUTE_JSON)],
        "results": [{"id": 2, "ok": true, "updated_at": 1760000000}, {"id": 3, "ok": false, "error": "fetch failed"}],
        "warnings": ""
    });
    let (server, client) = client_with(vec![respond(body)]).await;

    let resp: AddRouteResponse = within(client.add_remote_routes(REMOTE_ROUTES_LINK, true))
        .await
        .expect("add remote");

    assert_eq!(resp.results.len(), 2);
    assert!(!resp.results[1].ok);
    assert_eq!(
        server.recorded(),
        vec![sent(
            Method::Post,
            "/v1/routes",
            Some(json!({"remote": REMOTE_ROUTES_LINK, "auto_update": true, "timeout": 30}))
        )]
    );
}

#[tokio::test]
async fn update_route_profile_sends_profile_and_wait() {
    let share = bypass_russia();
    let body = json!({"route": parse::<Value>(ROUTE_JSON), "warnings": "restarted"});
    let (server, client) = client_with(vec![respond(body)]).await;

    let resp = within(client.update_route_profile(2, &share))
        .await
        .expect("update");

    assert_eq!(resp.warnings, "restarted");
    assert_eq!(
        server.recorded(),
        vec![sent(
            Method::Put,
            "/v1/routes/2",
            Some(
                json!({"profile": serde_json::to_value(&share).expect("to_value"), "timeout": 30})
            )
        )]
    );
}

#[tokio::test]
async fn rename_route_sends_only_the_name() {
    let (server, client) = client_with(vec![respond(
        json!({"route": parse::<Value>(ROUTE_JSON), "warnings": ""}),
    )])
    .await;

    within(client.rename_route(2, "Renamed"))
        .await
        .expect("rename");

    assert_eq!(
        server.recorded(),
        vec![sent(
            Method::Put,
            "/v1/routes/2",
            Some(json!({"name": "Renamed"}))
        )]
    );
}

#[tokio::test]
async fn set_route_auto_update_sends_only_the_flag() {
    let (server, client) = client_with(vec![respond(
        json!({"route": parse::<Value>(ROUTE_JSON), "warnings": ""}),
    )])
    .await;

    within(client.set_route_auto_update(2, false))
        .await
        .expect("auto update");

    assert_eq!(
        server.recorded(),
        vec![sent(
            Method::Put,
            "/v1/routes/2",
            Some(json!({"auto_update": false}))
        )]
    );
}

#[tokio::test]
async fn delete_route_sends_delete() {
    let (server, client) = client_with(vec![Action::Respond(ok_body(Value::Null))]).await;

    within(client.delete_route(5)).await.expect("delete");

    assert_eq!(
        server.recorded(),
        vec![sent(Method::Delete, "/v1/routes/5", None)]
    );
}

#[tokio::test]
async fn refresh_route_posts_update_with_wait() {
    let (server, client) = client_with(vec![respond(
        json!({"id": 2, "ok": true, "updated_at": 1760000000}),
    )])
    .await;

    let result = within(client.refresh_route(2)).await.expect("refresh");

    assert!(result.ok);
    assert_eq!(result.updated_at, Some(1760000000));
    assert_eq!(
        server.recorded(),
        vec![sent(
            Method::Post,
            "/v1/routes/2/update",
            Some(json!({"timeout": 30}))
        )]
    );
}

#[tokio::test]
async fn set_active_route_puts_id_and_returns_status() {
    let (server, client) = client_with(vec![respond(parse(STATUS_JSON))]).await;

    let status: Status = within(client.set_active_route(2))
        .await
        .expect("set active");

    assert_eq!(status.version, "1.4.0");
    assert_eq!(
        server.recorded(),
        vec![sent(
            Method::Put,
            "/v1/routes/active",
            Some(json!({"id": 2, "timeout": 30}))
        )]
    );
}

#[tokio::test]
async fn rule_sets_lists_names() {
    let (server, client) = client_with(vec![respond(
        json!({"rule_sets": ["geoip-ru", "geosite-ru"]}),
    )])
    .await;

    let resp: RuleSetsResponse = within(client.rule_sets()).await.expect("rule sets");

    assert_eq!(
        resp.rule_sets,
        vec!["geoip-ru".to_owned(), "geosite-ru".to_owned()]
    );
    assert_eq!(
        server.recorded(),
        vec![sent(Method::Get, "/v1/rule-sets", None)]
    );
}

#[tokio::test]
async fn remote_route_error_is_mapped() {
    let (_server, client) = client_with(vec![api_error(409, "remote_route")]).await;
    let err = within(client.update_route_profile(2, &bypass_russia()))
        .await
        .expect_err("remote profile refuses a profile");
    expect_api_error(err, 409, "remote_route");
}

#[tokio::test]
async fn route_active_error_is_mapped() {
    let (_server, client) = client_with(vec![api_error(409, "route_active")]).await;
    let err = within(client.delete_route(2))
        .await
        .expect_err("active profile");
    expect_api_error(err, 409, "route_active");
}

#[tokio::test]
async fn last_route_error_is_mapped() {
    let (_server, client) = client_with(vec![api_error(409, "last_route")]).await;
    let err = within(client.delete_route(1))
        .await
        .expect_err("last profile");
    expect_api_error(err, 409, "last_route");
}

#[tokio::test]
async fn not_remote_error_is_mapped() {
    let (_server, client) = client_with(vec![api_error(409, "not_remote")]).await;
    let err = within(client.refresh_route(1))
        .await
        .expect_err("local profile");
    expect_api_error(err, 409, "not_remote");
}

#[tokio::test]
async fn not_found_error_is_mapped() {
    let (_server, client) = client_with(vec![api_error(404, "not_found")]).await;
    let err = within(client.route(99)).await.expect_err("unknown profile");
    expect_api_error(err, 404, "not_found");
}
