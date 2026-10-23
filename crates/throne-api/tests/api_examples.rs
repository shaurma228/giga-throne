//! Integration tests checking typed API structs in `throne_api::types`
//! against the documentation in `docs/throne-api/api.md`.

use throne_api::types::*;

// ============================================================================
// Core Objects (api.md: Objects & Status)
// ============================================================================

/// Profile object example (api.md lines 389-406).
#[test]
fn profile_object() {
    let json = r#"{
  "id": 12,
  "group": 3,
  "name": "DE Frankfurt",
  "type": "vless",
  "address": "de1.example.com:443",
  "latency_ms": 85,
  "latency_at": 1760000000,
  "download_bps": 48300000,
  "upload_bps": 9100000,
  "country": "DE",
  "ip_out": "203.0.113.7",
  "traffic_up": 1048576,
  "traffic_down": 73400320,
  "running": false
}"#;
    let profile: Profile = serde_json::from_str(json).expect("failed to deserialize Profile");
    assert_eq!(profile.id, 12);
    assert_eq!(profile.group, 3);
    assert_eq!(profile.name, "DE Frankfurt");
    assert_eq!(profile.r#type, "vless");
    assert_eq!(profile.address, "de1.example.com:443");
    assert_eq!(profile.latency_ms, 85);
    assert_eq!(profile.latency_at, 1760000000);
    assert_eq!(profile.download_bps, 48300000);
    assert_eq!(profile.upload_bps, 9100000);
    assert_eq!(profile.country, "DE");
    assert_eq!(profile.ip_out, "203.0.113.7");
    assert_eq!(profile.traffic_up, 1048576);
    assert_eq!(profile.traffic_down, 73400320);
    assert!(!profile.running);

    // Round-trip check
    let val = serde_json::to_value(&profile).expect("failed to serialize Profile");
    let rt: Profile =
        serde_json::from_value(val).expect("failed to deserialize Profile round-trip");
    assert_eq!(profile, rt);
}

/// Group object example (api.md lines 423-425).
#[test]
fn group_object() {
    let json = r#"{"id": 3, "name": "Provider", "subscription": true, "archived": false, "profile_count": 120, "updated_at": 1760000000}"#;
    let group: Group = serde_json::from_str(json).expect("failed to deserialize Group");
    assert_eq!(group.id, 3);
    assert_eq!(group.name, "Provider");
    assert!(group.subscription);
    assert!(!group.archived);
    assert_eq!(group.profile_count, 120);
    assert_eq!(group.updated_at, 1760000000);

    // Round-trip check
    let val = serde_json::to_value(&group).expect("failed to serialize Group");
    let rt: Group = serde_json::from_value(val).expect("failed to deserialize Group round-trip");
    assert_eq!(group, rt);
}

/// Route object example (api.md lines 431-433).
#[test]
fn route_object() {
    let json = r#"{"id": 1, "name": "Default", "active": true}"#;
    let route: Route = serde_json::from_str(json).expect("failed to deserialize Route");
    assert_eq!(route.id, 1);
    assert_eq!(route.name, "Default");
    assert!(route.active);

    // Round-trip check
    let val = serde_json::to_value(&route).expect("failed to serialize Route");
    let rt: Route = serde_json::from_value(val).expect("failed to deserialize Route round-trip");
    assert_eq!(route, rt);
}

/// Status object example (api.md lines 437-454).
#[test]
fn status_object() {
    // placeholder entry removed from the doc example
    let json = r#"{
  "state": "running",
  "profile": {"id": 12, "name": "DE Frankfurt"},
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
    let status: Status = serde_json::from_str(json).expect("failed to deserialize Status");
    assert_eq!(status.state, ConnectionState::Running);
    assert_eq!(status.last_profile_id, 12);
    assert!(status.tun);
    assert_eq!(status.tun_conflict, None);
    assert!(!status.system_proxy);
    assert_eq!(status.kill_switch, KillSwitchState::Off);
    assert!(status.core);
    assert_eq!(status.inbound.port, 2080);
    assert_eq!(status.inbound.address, "127.0.0.1");
    assert!(status.inbound.enabled);
    assert_eq!(status.version, "1.4.0");
    assert_eq!(status.capabilities.tun, Some(true));
    assert!(status.capabilities.kill_switch);
    assert!(!status.capabilities.headless);
    assert!(!status.restart_needed);
    assert_eq!(status.degraded, None);

    let prof = status.profile.as_ref().expect("profile must be present");
    assert_eq!(prof.id, 12);
    assert_eq!(prof.name, "DE Frankfurt");

    let route = status.route.as_ref().expect("route must be present");
    assert_eq!(route.id, 1);
    assert_eq!(route.name, "Default");
    assert!(route.active);

    // Round-trip check
    let val = serde_json::to_value(&status).expect("failed to serialize Status");
    let rt: Status = serde_json::from_value(val).expect("failed to deserialize Status round-trip");
    assert_eq!(status, rt);
}

/// Job object example (api.md lines 481-491).
#[test]
fn job_object() {
    // placeholder entry removed from the doc example
    let json = r#"{
  "id": 7,
  "kind": "latency",
  "state": "done",
  "profile_ids": [12, 15, 16],
  "started_at": 1760000000,
  "finished_at": 1760000042,
  "results": [{"id": 12, "latency_ms": 85}]
}"#;
    let job: Job = serde_json::from_str(json).expect("failed to deserialize Job");
    assert_eq!(job.id, 7);
    assert_eq!(job.kind, JobKind::Latency);
    assert_eq!(job.state, JobState::Done);
    assert_eq!(job.profile_ids, vec![12, 15, 16]);
    assert_eq!(job.started_at, 1760000000);
    assert_eq!(job.finished_at, Some(1760000042));

    let results = job.results.as_ref().expect("results should be present");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, 12);
    assert_eq!(results[0].latency_ms, 85);

    // Round-trip check
    let val = serde_json::to_value(&job).expect("failed to serialize Job");
    let rt: Job = serde_json::from_value(val).expect("failed to deserialize Job round-trip");
    assert_eq!(job, rt);
}

/// Update result object examples (api.md lines 504-507).
#[test]
fn update_result_object() {
    let ok_json = r#"{"ok": true, "added": 3, "updated": 10, "removed": 1, "kept": 0}"#;
    let res_ok: UpdateResult =
        serde_json::from_str(ok_json).expect("failed to deserialize ok UpdateResult");
    assert!(res_ok.ok);
    assert_eq!(res_ok.added, Some(3));
    assert_eq!(res_ok.updated, Some(10));
    assert_eq!(res_ok.removed, Some(1));
    assert_eq!(res_ok.kept, Some(0));

    // Round-trip check
    let val_ok = serde_json::to_value(&res_ok).expect("failed to serialize ok UpdateResult");
    let rt_ok: UpdateResult =
        serde_json::from_value(val_ok).expect("failed to deserialize ok UpdateResult round-trip");
    assert_eq!(res_ok, rt_ok);

    let err_json = r#"{"ok": false, "error": "..."}"#;
    let res_err: UpdateResult =
        serde_json::from_str(err_json).expect("failed to deserialize error UpdateResult");
    assert!(!res_err.ok);
    assert_eq!(res_err.error.as_deref(), Some("..."));

    // Round-trip check
    let val_err = serde_json::to_value(&res_err).expect("failed to serialize error UpdateResult");
    let rt_err: UpdateResult = serde_json::from_value(val_err)
        .expect("failed to deserialize error UpdateResult round-trip");
    assert_eq!(res_err, rt_err);
}

// ============================================================================
// Groups and Profiles (api.md: Groups and profiles)
// ============================================================================

/// POST /v1/import request body examples (api.md lines 548-551).
#[test]
fn post_import_request() {
    let json_profiles = r#"{"as": "profiles", "text": "vless://...\nss://...", "group": 3}"#;
    let req1: ImportRequest =
        serde_json::from_str(json_profiles).expect("failed to deserialize profiles ImportRequest");
    assert_eq!(req1.r#as, ImportAs::Profiles);
    assert_eq!(req1.text.as_deref(), Some("vless://...\nss://..."));
    assert_eq!(req1.group, Some(3));

    let json_url = r#"{"as": "url", "url": "https://example.com/list.txt", "group": 3}"#;
    let req2: ImportRequest =
        serde_json::from_str(json_url).expect("failed to deserialize url ImportRequest");
    assert_eq!(req2.r#as, ImportAs::Url);
    assert_eq!(req2.url.as_deref(), Some("https://example.com/list.txt"));
    assert_eq!(req2.group, Some(3));
}

/// POST /v1/import response body example (api.md lines 566-578).
#[test]
fn import_response() {
    let json = r#"{
  "group": 3,
  "imported": 12,
  "profile_ids": [40, 41, 42],
  "failed": 1,
  "warned": 2,
  "issues": [
    {"severity": "error", "field": "port", "message": "...", "count": 1, "examples": ["vless://host:70000 #name"]}
  ],
  "errors": []
}"#;
    let resp: ImportResponse =
        serde_json::from_str(json).expect("failed to deserialize ImportResponse");
    assert_eq!(resp.group, 3);
    assert_eq!(resp.imported, Some(12));
    assert_eq!(resp.profile_ids, vec![40, 41, 42]);
    assert_eq!(resp.failed, Some(1));
    assert_eq!(resp.warned, Some(2));
    assert_eq!(resp.issues.len(), 1);
    assert_eq!(resp.issues[0].severity, ImportIssueSeverity::Error);
    assert_eq!(resp.issues[0].field, "port");
    assert_eq!(resp.issues[0].message, "...");
    assert_eq!(resp.issues[0].count, 1);
    assert_eq!(resp.issues[0].examples, vec!["vless://host:70000 #name"]);
    assert!(resp.errors.is_empty());

    // Round-trip check
    let val = serde_json::to_value(&resp).expect("failed to serialize ImportResponse");
    let rt: ImportResponse =
        serde_json::from_value(val).expect("failed to deserialize ImportResponse round-trip");
    assert_eq!(resp, rt);
}

/// POST /v1/groups request body example (api.md lines 601-603).
#[test]
fn post_groups_request() {
    let json = r#"{"name": "Provider", "url": "https://sub.example/...", "auto_update": true, "update": true}"#;
    let req: CreateGroupRequest =
        serde_json::from_str(json).expect("failed to deserialize CreateGroupRequest");
    assert_eq!(req.name, "Provider");
    assert_eq!(req.url.as_deref(), Some("https://sub.example/..."));
    assert_eq!(req.auto_update, Some(true));
    assert_eq!(req.update, Some(true));
}

/// POST /v1/groups/update response body example (api.md lines 637-639).
#[test]
fn post_groups_update_response() {
    let json = r#"{"results": [{"id": 3, "ok": true, "added": 3, "updated": 10, "removed": 1, "kept": 0}, {"id": 4, "ok": false, "error": "..."}]}"#;
    let resp: UpdateAllGroupsResponse =
        serde_json::from_str(json).expect("failed to deserialize UpdateAllGroupsResponse");
    assert_eq!(resp.results.len(), 2);
    assert_eq!(resp.results[0].id, Some(3));
    assert!(resp.results[0].ok);
    assert_eq!(resp.results[0].added, Some(3));
    assert_eq!(resp.results[0].updated, Some(10));
    assert_eq!(resp.results[1].id, Some(4));
    assert!(!resp.results[1].ok);
    assert_eq!(resp.results[1].error.as_deref(), Some("..."));

    // Round-trip check
    let val = serde_json::to_value(&resp).expect("failed to serialize UpdateAllGroupsResponse");
    let rt: UpdateAllGroupsResponse = serde_json::from_value(val)
        .expect("failed to deserialize UpdateAllGroupsResponse round-trip");
    assert_eq!(resp, rt);
}

// ============================================================================
// Connection (api.md: Connection)
// ============================================================================

/// PUT /v1/connection request body examples (api.md lines 684-688).
#[test]
fn put_connection_select_lowest_latency() {
    let json_lowest =
        r#"{"select": "lowest_latency", "group": 3, "name": "DE|NL", "max_age": 3600}"#;
    let req: StartConnectionRequest =
        serde_json::from_str(json_lowest).expect("failed to deserialize lowest_latency request");
    assert_eq!(req.select, Some(ProfileSelectMode::LowestLatency));
    assert_eq!(req.group, Some(3));
    assert_eq!(req.name.as_deref(), Some("DE|NL"));
    assert_eq!(req.max_age, Some(3600));

    // Also verify the other request variants in this code block
    let json_profile = r#"{"profile": 12}"#;
    let req_profile: StartConnectionRequest =
        serde_json::from_str(json_profile).expect("failed to deserialize profile request");
    assert_eq!(req_profile.profile, Some(12));

    let json_last = r#"{"select": "last"}"#;
    let req_last: StartConnectionRequest =
        serde_json::from_str(json_last).expect("failed to deserialize last request");
    assert_eq!(req_last.select, Some(ProfileSelectMode::Last));
}

// ============================================================================
// Modes and Routing (api.md: Modes and routing)
// ============================================================================

/// PUT /v1/modes request body example (api.md lines 723-725).
#[test]
fn put_modes_request() {
    let json = r#"{"tun": true, "system_proxy": false}"#;
    let req: SetModesRequest =
        serde_json::from_str(json).expect("failed to deserialize SetModesRequest");
    assert_eq!(req.tun, Some(true));
    assert_eq!(req.system_proxy, Some(false));
}

/// PUT /v1/routes/active request body example (api.md lines 751-753).
#[test]
fn put_routes_active_request() {
    let json = r#"{"id": 2}"#;
    let req: SetActiveRouteRequest =
        serde_json::from_str(json).expect("failed to deserialize SetActiveRouteRequest");
    assert_eq!(req.id, 2);
    assert_eq!(req.wait, None);
}

// ============================================================================
// Tests (api.md: Tests)
// ============================================================================

/// POST /v1/tests request body example (api.md lines 768-770).
#[test]
fn post_tests_request() {
    let json = r#"{"kind": "latency", "group": 3, "name": "DE|NL"}"#;
    let req: StartTestRequest =
        serde_json::from_str(json).expect("failed to deserialize StartTestRequest");
    assert_eq!(req.kind, JobKind::Latency);
    assert_eq!(req.group, Some(3));
    assert_eq!(req.name.as_deref(), Some("DE|NL"));
}

// ============================================================================
// Statistics (api.md: Statistics)
// ============================================================================

/// GET /v1/stats/live response body example (api.md lines 808-810).
#[test]
fn get_stats_live_response() {
    let json = r#"{"proxy": {"up": 1200, "down": 56000}, "direct": {"up": 0, "down": 310}}"#;
    let resp: LiveStatsResponse =
        serde_json::from_str(json).expect("failed to deserialize LiveStatsResponse");
    assert_eq!(resp.proxy.up, 1200);
    assert_eq!(resp.proxy.down, 56000);
    assert_eq!(resp.direct.up, 0);
    assert_eq!(resp.direct.down, 310);

    // Round-trip check
    let val = serde_json::to_value(&resp).expect("failed to serialize LiveStatsResponse");
    let rt: LiveStatsResponse =
        serde_json::from_value(val).expect("failed to deserialize LiveStatsResponse round-trip");
    assert_eq!(resp, rt);
}

/// GET /v1/stats/traffic response body example (api.md lines 823-832).
#[test]
fn get_stats_traffic_response() {
    let json = r#"{
  "by": "profile",
  "from": 1759913600,
  "to": 1760000000,
  "items": [
    {"id": 12, "name": "DE Frankfurt", "up": 10485760, "down": 734003200},
    {"id": -101, "name": "Direct", "up": 2048, "down": 409600}
  ]
}"#;
    let resp: TrafficStatsResponse =
        serde_json::from_str(json).expect("failed to deserialize TrafficStatsResponse");
    assert_eq!(resp.by, TrafficBy::Profile);
    assert_eq!(resp.from, 1759913600);
    assert_eq!(resp.to, 1760000000);
    assert_eq!(resp.items.len(), 2);
    assert_eq!(resp.items[0].id, Some(12));
    assert_eq!(resp.items[0].name, "DE Frankfurt");
    assert_eq!(resp.items[0].up, 10485760);
    assert_eq!(resp.items[0].down, 734003200);
    assert_eq!(resp.items[1].id, Some(-101));
    assert_eq!(resp.items[1].name, "Direct");
    assert_eq!(resp.items[1].up, 2048);
    assert_eq!(resp.items[1].down, 409600);

    // Round-trip check
    let val = serde_json::to_value(&resp).expect("failed to serialize TrafficStatsResponse");
    let rt: TrafficStatsResponse =
        serde_json::from_value(val).expect("failed to deserialize TrafficStatsResponse round-trip");
    assert_eq!(resp, rt);
}

// ============================================================================
// Settings (api.md: Settings)
// ============================================================================

/// PUT /v1/settings request body example (api.md lines 853-855).
#[test]
fn put_settings_request() {
    let json = r#"{"settings": {"log_level": "debug", "kill_switch": true}}"#;
    let req: UpdateSettingsRequest<Settings> =
        serde_json::from_str(json).expect("failed to deserialize UpdateSettingsRequest");
    assert_eq!(req.settings.log_level.as_deref(), Some("debug"));
    assert_eq!(req.settings.kill_switch, Some(true));
}

// ============================================================================
// Rule-Sets and Geo Files (api.md: Rule-sets and geo files)
// ============================================================================

/// POST /v1/rule-sets/update response body example (api.md lines 947-949).
#[test]
fn post_rule_sets_update_response() {
    let json = r#"{"updated": 3, "failures": [{"tag": "geosite-x", "error": "..."}]}"#;
    let resp: RuleSetsUpdateResponse =
        serde_json::from_str(json).expect("failed to deserialize RuleSetsUpdateResponse");
    assert_eq!(resp.updated, Some(3));
    assert_eq!(resp.failures.len(), 1);
    assert_eq!(resp.failures[0].tag, "geosite-x");
    assert_eq!(resp.failures[0].error, "...");

    // Round-trip check
    let val = serde_json::to_value(&resp).expect("failed to serialize RuleSetsUpdateResponse");
    let rt: RuleSetsUpdateResponse = serde_json::from_value(val)
        .expect("failed to deserialize RuleSetsUpdateResponse round-trip");
    assert_eq!(resp, rt);
}

/// GET /v1/geo-assets response body example (api.md lines 958-960).
#[test]
fn get_geo_assets_response() {
    let json = r#"{"files": [{"name": "geoip.dat", "url": "https://...", "present": true, "size": 1234, "updated_at": 1760000000}]}"#;
    let resp: GeoAssetsResponse =
        serde_json::from_str(json).expect("failed to deserialize GeoAssetsResponse");
    assert_eq!(resp.files.len(), 1);
    assert_eq!(resp.files[0].name, "geoip.dat");
    assert_eq!(resp.files[0].url, "https://...");
    assert!(resp.files[0].present);
    assert_eq!(resp.files[0].size, 1234);
    assert_eq!(resp.files[0].updated_at, 1760000000);

    // Round-trip check
    let val = serde_json::to_value(&resp).expect("failed to serialize GeoAssetsResponse");
    let rt: GeoAssetsResponse =
        serde_json::from_value(val).expect("failed to deserialize GeoAssetsResponse round-trip");
    assert_eq!(resp, rt);
}

/// POST /v1/geo-assets/update request body example (api.md lines 968-970).
#[test]
fn post_geo_assets_update_request() {
    let json = r#"{"files": ["geoip", "geosite"], "only_missing": false}"#;
    let req: GeoAssetsUpdateRequest =
        serde_json::from_str(json).expect("failed to deserialize GeoAssetsUpdateRequest");
    assert_eq!(
        req.files,
        Some(vec!["geoip".to_string(), "geosite".to_string()])
    );
    assert_eq!(req.only_missing, Some(false));
}

/// POST /v1/geo-assets/update response body example (api.md lines 977-979).
#[test]
fn post_geo_assets_update_response() {
    let json = r#"{"files": [{"name": "geoip.dat", "ok": true}, {"name": "geosite.dat", "ok": false, "error": "..."}]}"#;
    let resp: GeoAssetsUpdateResponse =
        serde_json::from_str(json).expect("failed to deserialize GeoAssetsUpdateResponse");
    assert_eq!(resp.files.len(), 2);
    assert_eq!(resp.files[0].name, "geoip.dat");
    assert!(resp.files[0].ok);
    assert_eq!(resp.files[1].name, "geosite.dat");
    assert!(!resp.files[1].ok);
    assert_eq!(resp.files[1].error.as_deref(), Some("..."));

    // Round-trip check
    let val = serde_json::to_value(&resp).expect("failed to serialize GeoAssetsUpdateResponse");
    let rt: GeoAssetsUpdateResponse = serde_json::from_value(val)
        .expect("failed to deserialize GeoAssetsUpdateResponse round-trip");
    assert_eq!(resp, rt);
}

// ============================================================================
// Backup and Restore (api.md: Backup and restore)
// ============================================================================

/// POST /v1/backups request body example (api.md lines 1007-1009).
#[test]
fn post_backups_request() {
    let json = r#"{"parts": ["profiles", "routes"]}"#;
    let req: BackupRequest =
        serde_json::from_str(json).expect("failed to deserialize BackupRequest");
    assert_eq!(
        req.parts,
        Some(vec!["profiles".to_string(), "routes".to_string()])
    );
}

/// GET /v1/backups/{id} response body example (api.md lines 1035-1037).
#[test]
fn get_backups_id_response() {
    let json = r#"{"id": 1, "size": 123456, "sha256": "<hex>", "parts": ["profiles", "routes"], "offset": 0, "data": "<base64>", "eof": true}"#;
    let resp: BackupChunkResponse =
        serde_json::from_str(json).expect("failed to deserialize BackupChunkResponse");
    assert_eq!(resp.id, 1);
    assert_eq!(resp.size, 123456);
    assert_eq!(resp.sha256, "<hex>");
    assert_eq!(
        resp.parts,
        vec!["profiles".to_string(), "routes".to_string()]
    );
    assert_eq!(resp.offset, 0);
    assert_eq!(resp.data, "<base64>");
    assert!(resp.eof);

    // Round-trip check
    let val = serde_json::to_value(&resp).expect("failed to serialize BackupChunkResponse");
    let rt: BackupChunkResponse =
        serde_json::from_value(val).expect("failed to deserialize BackupChunkResponse round-trip");
    assert_eq!(resp, rt);
}

/// POST /v1/restores request body example (api.md lines 1054-1056).
#[test]
fn post_restores_request() {
    let json = r#"{"size": 123456, "sha256": "<hex>"}"#;
    let req: RestoreOpenRequest =
        serde_json::from_str(json).expect("failed to deserialize RestoreOpenRequest");
    assert_eq!(req.size, 123456);
    assert_eq!(req.sha256, "<hex>");
}

/// PUT /v1/restores/{id} request body example (api.md lines 1069-1071).
#[test]
fn put_restores_id_request() {
    let json = r#"{"offset": 0, "data": "<base64>"}"#;
    let req: RestoreChunkRequest =
        serde_json::from_str(json).expect("failed to deserialize RestoreChunkRequest");
    assert_eq!(req.offset, 0);
    assert_eq!(req.data, "<base64>");
}

/// PUT /v1/restores/{id} response body example (api.md lines 1081-1088).
#[test]
fn put_restores_id_response() {
    let json = r#"{
  "id": 2,
  "size": 123456,
  "received": 123456,
  "archive": {"created_at": "Thu Oct 16 15:04:05 2026", "platform": "linux", "parts": ["profiles", "routes", "settings"]}
}"#;
    let resp: RestoreChunkResponse =
        serde_json::from_str(json).expect("failed to deserialize RestoreChunkResponse");
    assert_eq!(resp.id, 2);
    assert_eq!(resp.size, 123456);
    assert_eq!(resp.received, 123456);

    let archive = resp.archive.as_ref().expect("archive metadata");
    assert_eq!(archive.created_at, "Thu Oct 16 15:04:05 2026");
    assert_eq!(archive.platform, "linux");
    assert_eq!(
        archive.parts,
        vec![
            "profiles".to_string(),
            "routes".to_string(),
            "settings".to_string(),
        ]
    );

    // Round-trip check
    let val = serde_json::to_value(&resp).expect("failed to serialize RestoreChunkResponse");
    let rt: RestoreChunkResponse =
        serde_json::from_value(val).expect("failed to deserialize RestoreChunkResponse round-trip");
    assert_eq!(resp, rt);
}

/// GET /v1/restores/{id} response body examples (api.md lines 1109-1112).
#[test]
fn get_restores_id_response() {
    let json_complete = r#"{"id": 2, "size": 123456, "received": 123456, "state": "complete", "archive": {"created_at": "...", "platform": "linux", "parts": ["profiles"]}}"#;
    let resp1: RestoreStatusResponse = serde_json::from_str(json_complete)
        .expect("failed to deserialize complete RestoreStatusResponse");
    assert_eq!(resp1.id, 2);
    assert_eq!(resp1.size, 123456);
    assert_eq!(resp1.received, 123456);
    assert_eq!(resp1.state, RestoreState::Complete);

    let archive = resp1.archive.as_ref().expect("archive metadata");
    assert_eq!(archive.created_at, "...");
    assert_eq!(archive.platform, "linux");
    assert_eq!(archive.parts, vec!["profiles".to_string()]);

    // Round-trip check
    let val1 =
        serde_json::to_value(&resp1).expect("failed to serialize complete RestoreStatusResponse");
    let rt1: RestoreStatusResponse = serde_json::from_value(val1)
        .expect("failed to deserialize complete RestoreStatusResponse round-trip");
    assert_eq!(resp1, rt1);

    let json_failed = r#"{"id": 2, "size": 123456, "received": 123456, "state": "failed", "error": "The archive holds nothing Throne can restore"}"#;
    let resp2: RestoreStatusResponse = serde_json::from_str(json_failed)
        .expect("failed to deserialize failed RestoreStatusResponse");
    assert_eq!(resp2.id, 2);
    assert_eq!(resp2.size, 123456);
    assert_eq!(resp2.received, 123456);
    assert_eq!(resp2.state, RestoreState::Failed);
    assert_eq!(
        resp2.error.as_deref(),
        Some("The archive holds nothing Throne can restore")
    );

    // Round-trip check
    let val2 =
        serde_json::to_value(&resp2).expect("failed to serialize failed RestoreStatusResponse");
    let rt2: RestoreStatusResponse = serde_json::from_value(val2)
        .expect("failed to deserialize failed RestoreStatusResponse round-trip");
    assert_eq!(resp2, rt2);
}

/// POST /v1/restores/{id}/apply request body example (api.md lines 1127-1129).
#[test]
fn post_restores_id_apply_request() {
    let json = r#"{"parts": ["profiles", "routes", "otp", "ip_lists"]}"#;
    let req: RestoreApplyRequest =
        serde_json::from_str(json).expect("failed to deserialize RestoreApplyRequest");
    assert_eq!(
        req.parts,
        Some(vec![
            "profiles".to_string(),
            "routes".to_string(),
            "otp".to_string(),
            "ip_lists".to_string(),
        ])
    );
    assert_eq!(req.wait, None);
}

// ============================================================================
// Headless Throne Health (api.md: Headless Throne)
// ============================================================================

/// Throne --health JSON output example (api.md line 166).
#[test]
fn health_status_object() {
    let json = r#"{"api":"listening","core_restarts":0,"degraded":null,"headless_dialogs":0,"ok":true,"previous_crash":false,"previous_exit":{"at":1760700000,"code":75,"reason":"restart requested"},"ready":true,"state":"running","stuck_s":0}"#;
    let health: HealthStatus =
        serde_json::from_str(json).expect("failed to deserialize HealthStatus");
    assert!(health.ok);
    assert!(health.ready);
    assert_eq!(health.state, ConnectionState::Running);
    assert_eq!(health.api, ApiListenerState::Listening);
    assert_eq!(health.degraded, None);
    assert_eq!(health.stuck_s, 0);
    assert_eq!(health.core_restarts, 0);

    let exit = health.previous_exit.as_ref().expect("previous_exit");
    assert_eq!(exit.code, 75);
    assert_eq!(exit.reason, "restart requested");
    assert_eq!(exit.at, 1760700000);

    // Round-trip check
    let val = serde_json::to_value(&health).expect("failed to serialize HealthStatus");
    let rt: HealthStatus =
        serde_json::from_value(val).expect("failed to deserialize HealthStatus round-trip");
    assert_eq!(health, rt);
}

// ============================================================================
// Enums & Extra Fields Tolerance Tests
// ============================================================================

/// Verifies that all typed enums deserialize unknown string values to the `Unknown` variant.
#[test]
fn unknown_enum_fallbacks() {
    assert_eq!(
        serde_json::from_str::<ConnectionState>(r#""future_state""#).unwrap(),
        ConnectionState::Unknown
    );
    assert_eq!(
        serde_json::from_str::<KillSwitchState>(r#""super_armed""#).unwrap(),
        KillSwitchState::Unknown
    );
    assert_eq!(
        serde_json::from_str::<KillSwitchError>(r#""kernel_panic""#).unwrap(),
        KillSwitchError::Unknown
    );
    assert_eq!(
        serde_json::from_str::<DegradedReason>(r#""hardware_failure""#).unwrap(),
        DegradedReason::Unknown
    );
    assert_eq!(
        serde_json::from_str::<ApiListenerState>(r#""suspended""#).unwrap(),
        ApiListenerState::Unknown
    );
    assert_eq!(
        serde_json::from_str::<JobKind>(r#""dns_leak""#).unwrap(),
        JobKind::Unknown
    );
    assert_eq!(
        serde_json::from_str::<JobState>(r#""paused""#).unwrap(),
        JobState::Unknown
    );
    assert_eq!(
        serde_json::from_str::<ImportAs>(r#""qr_code""#).unwrap(),
        ImportAs::Unknown
    );
    assert_eq!(
        serde_json::from_str::<ImportIssueSeverity>(r#""critical""#).unwrap(),
        ImportIssueSeverity::Unknown
    );
    assert_eq!(
        serde_json::from_str::<ProfileSelectMode>(r#""random""#).unwrap(),
        ProfileSelectMode::Unknown
    );
    assert_eq!(
        serde_json::from_str::<TrafficBy>(r#""interface""#).unwrap(),
        TrafficBy::Unknown
    );
    assert_eq!(
        serde_json::from_str::<RestoreState>(r#""validating""#).unwrap(),
        RestoreState::Unknown
    );
}

/// Verifies that composite objects (Status, Job) with unknown enum variants deserialize
/// without error, assigning the `Unknown` enum variant.
#[test]
fn unknown_enums_in_composite_structs() {
    let status_json = r#"{
        "state": "hyper_speed",
        "last_profile_id": -1,
        "tun": false,
        "system_proxy": false,
        "kill_switch": "quantum_shield",
        "kill_switch_error": "solar_flare",
        "core": true,
        "inbound": {"enabled": true, "address": "127.0.0.1", "port": 2080},
        "version": "1.4.0",
        "capabilities": {"tun": true, "kill_switch": true, "headless": true},
        "restart_needed": false,
        "degraded": "power_outage",
        "api": "sleeping"
    }"#;
    let status: Status =
        serde_json::from_str(status_json).expect("deserialize status with unknown enums");
    assert_eq!(status.state, ConnectionState::Unknown);
    assert_eq!(status.kill_switch, KillSwitchState::Unknown);
    assert_eq!(status.kill_switch_error, Some(KillSwitchError::Unknown));
    assert_eq!(status.degraded, Some(DegradedReason::Unknown));
    assert_eq!(status.api, Some(ApiListenerState::Unknown));

    let job_json = r#"{
        "id": 99,
        "kind": "bandwidth_burst",
        "state": "waiting_for_peer",
        "profile_ids": [1],
        "started_at": 1760000000
    }"#;
    let job: Job = serde_json::from_str(job_json).expect("deserialize job with unknown enums");
    assert_eq!(job.kind, JobKind::Unknown);
    assert_eq!(job.state, JobState::Unknown);
}

/// Verifies that unknown extra fields are tolerated when deserializing `Profile`.
#[test]
fn profile_tolerates_unknown_fields() {
    let json = r#"{
        "id": 42,
        "group": 1,
        "name": "Custom Server",
        "type": "vless",
        "address": "custom.example.com:443",
        "unknown_future_field_string": "extra_value",
        "unknown_future_field_number": 9999,
        "unknown_future_field_object": {"nested": true}
    }"#;
    let profile: Profile =
        serde_json::from_str(json).expect("failed to deserialize Profile with unknown fields");
    assert_eq!(profile.id, 42);
    assert_eq!(profile.name, "Custom Server");
    assert_eq!(profile.r#type, "vless");
    assert_eq!(profile.address, "custom.example.com:443");
}

/// Verifies that unknown extra fields are tolerated when deserializing `Status`.
#[test]
fn status_tolerates_unknown_fields() {
    let json = r#"{
        "state": "idle",
        "last_profile_id": -1,
        "tun": false,
        "system_proxy": false,
        "kill_switch": "off",
        "core": false,
        "inbound": {"enabled": false, "address": "127.0.0.1", "port": 2080},
        "version": "1.4.0",
        "capabilities": {"tun": false, "kill_switch": false, "headless": true},
        "restart_needed": false,
        "unknown_status_flag": true,
        "extra_debug_payload": [1, 2, 3]
    }"#;
    let status: Status =
        serde_json::from_str(json).expect("failed to deserialize Status with unknown fields");
    assert_eq!(status.state, ConnectionState::Idle);
    assert!(!status.core);
    assert_eq!(status.version, "1.4.0");
    assert!(!status.tun);
}
