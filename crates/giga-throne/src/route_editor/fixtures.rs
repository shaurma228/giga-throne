//! Profiles the route editor tests share: the real Bypass Russia link from
//! `crates/throne-api/tests/routes.rs` and a synthetic profile that holds every
//! kind of rule the editor must not lose.

use throne_api::decode_route_link;
use throne_api::types::RouteShare;

use super::EditorModel;

/// Bypass Russia, from a real `throne://route/` link.
pub(super) const BYPASS_RUSSIA_LINK: &str = "throne://route/eyJkZWZhdWx0X291dGJvdW5kIjoicHJveHkiLCJraW5kIjoidGhyb25lLXJvdXRlLXByb2ZpbGUiLCJuYW1lIjoiQnlwYXNzIFJ1c3NpYSIsInJ1bGVzIjpbeyJhY3Rpb24iOiJoaWphY2stZG5zIiwibmFtZSI6ImltcG9ydGVkIHJ1bGUgIzEiLCJwcm90b2NvbCI6ImRucyIsInR5cGUiOiJjdXN0b20ifSx7ImFjdGlvbiI6InJvdXRlIiwiaXBfaXNfcHJpdmF0ZSI6dHJ1ZSwibmFtZSI6ImltcG9ydGVkIHJ1bGUgIzIiLCJvdXRib3VuZCI6ImRpcmVjdCIsInR5cGUiOiJjdXN0b20ifSx7ImFjdGlvbiI6InJvdXRlIiwibmFtZSI6ImltcG9ydGVkIHJ1bGUgIzMiLCJvdXRib3VuZCI6ImRpcmVjdCIsInJ1bGVfc2V0IjpbImdlb2lwLXJ1IiwiZ2Vvc2l0ZS1ydSIsImdlb3NpdGUtY2F0ZWdvcnktcnUiLCJnZW9zaXRlLXJ1LWF2YWlsYWJsZS1vbmx5LWluc2lkZSJdLCJ0eXBlIjoiY3VzdG9tIn1dLCJ2IjoxfQ";

/// Simple rules of all three buckets, a process-path rule, an endpoint rule,
/// a hijacked DNS rule, an unknown action, unknown fields at both levels, a
/// disabled rule, an unknown outbound and a `warp-bypass` outbound.
pub(super) const SYNTHETIC: &str = r#"{
  "kind": "throne-route-profile", "v": 1, "name": "Synthetic",
  "default_outbound": "direct",
  "dns": {"servers": [{"tag": "remote"}], "final": "remote"},
  "raw": {"log": {"level": "warn"}},
  "outbound_names": ["proxy", "direct"],
  "rules": [
    {"type": "custom", "name": "dns hijack", "action": "hijack-dns", "protocol": "dns", "sniffer_hint": 1},
    {"type": "simple_address_block", "name": "Сайты: Блокировать", "action": "route", "outbound": "block",
     "domain_suffix": ["ads.example.com"], "rule_set": ["geosite-category-ads-all"]},
    {"type": "simple_process_name_bypass", "name": "", "action": "route", "outbound": "direct",
     "process_name": ["steam.exe"], "disabled": true},
    {"type": "simple_address_proxy", "action": "route", "outbound": "proxy",
     "domain": ["youtube.com"], "ip_cidr": ["10.0.0.0/8"]},
    {"type": "simple_process_path_proxy", "name": "paths", "action": "route", "outbound": "proxy",
     "process_path": ["/usr/bin/foo"]},
    {"type": "endpoint_preferred_by", "name": "ep", "action": "route", "outbound": "warp-bypass",
     "endpoint": "wg0"},
    {"type": "custom", "name": "reject ads", "action": "reject", "domain_keyword": ["ads"], "method": "drop"},
    {"type": "custom", "name": "ports", "action": "route", "outbound": "custom-out",
     "port": [443, "8080"], "port_range": ["1000:2000"], "network": "tcp", "invert": true},
    {"type": "custom", "name": "unknown outbound", "action": "route", "outbound": "proxy-b",
     "ip_is_private": false, "source_port": [1]}
  ]
}"#;

/// Bypass Russia, decoded from its link.
pub(super) fn bypass_russia() -> RouteShare {
    decode_route_link(BYPASS_RUSSIA_LINK).expect("the fixture link decodes")
}

/// The synthetic profile.
pub(super) fn synthetic() -> RouteShare {
    serde_json::from_str(SYNTHETIC).expect("the fixture parses")
}

/// An editor over the profile `json`, which is not remote.
pub(super) fn model_of(json: &str) -> EditorModel {
    let share: RouteShare = serde_json::from_str(json).expect("the test profile parses");
    EditorModel::from_share(&share, false)
}
