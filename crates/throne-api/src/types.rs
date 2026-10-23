//! Serde data types for the Throne API (version 1).
//!
//! Specifications:
//! - Request/response definitions: `docs/throne-api/api.md`
//! - Transport and framing: `docs/throne-api/protocol.md`

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ============================================================================
// Conventions & Helpers (api.md: Conventions)
// ============================================================================

const fn default_none_id() -> i64 {
    -1
}

/// Helper deserializer for integer query parameters that may arrive as either
/// a JSON number or a string.
fn deserialize_opt_i64_lenient<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(i64),
        Str(String),
    }

    match Option::<NumOrStr>::deserialize(deserializer)? {
        None => Ok(None),
        Some(NumOrStr::Num(n)) => Ok(Some(n)),
        Some(NumOrStr::Str(s)) => {
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse::<i64>().map(Some).map_err(serde::de::Error::custom)
            }
        }
    }
}

/// Waiting parameters for requests that start, stop, or restart operations.
///
/// See api.md section "Conventions -> Waiting".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WaitParams {
    /// Respond when the change has finished or failed (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Seconds to wait before responding with 202 (1 to 300).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Selection criteria for requests acting on multiple profiles.
///
/// See api.md section "Conventions -> Selecting profiles".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProfileSelection {
    /// Selected profile IDs in exact order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<i64>>,

    /// Group ID whose profiles should be selected in Throne's order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<i64>,

    /// Optional case-insensitive regular expression (PCRE) matching profile names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// ============================================================================
// Core API Objects (api.md: Objects)
// ============================================================================

/// Profile object representing a VPN/proxy profile configuration and its metrics.
///
/// See api.md section "Objects -> Profile".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Profile {
    /// Profile ID (-1 means none).
    #[serde(default = "default_none_id")]
    pub id: i64,

    /// Group ID this profile belongs to.
    #[serde(default = "default_none_id")]
    pub group: i64,

    /// Name shown by Throne.
    #[serde(default)]
    pub name: String,

    /// Throne profile type, e.g. "vless", "shadowsocks", "chain", "autoselector".
    #[serde(default, rename = "type")]
    pub r#type: String,

    /// Address shown by Throne (or tracked group name for auto selectors).
    #[serde(default)]
    pub address: String,

    /// Last URL test: >0 latency in ms, 0 not tested, -1 failed, -2 VPN tunnel up but test failed.
    #[serde(default)]
    pub latency_ms: i64,

    /// Unix timestamp in seconds when `latency_ms` was measured (0 = unknown/never).
    #[serde(default)]
    pub latency_at: i64,

    /// Last speed test download rate in bits per second (0 not tested, -1 failed).
    #[serde(default)]
    pub download_bps: i64,

    /// Last speed test upload rate in bits per second (0 not tested, -1 failed).
    #[serde(default)]
    pub upload_bps: i64,

    /// ISO 3166-1 alpha-2 country code, or "" if not tested.
    #[serde(default)]
    pub country: String,

    /// Exit IP address from the last IP test, or "" if not tested.
    #[serde(default)]
    pub ip_out: String,

    /// Total bytes sent through this profile.
    #[serde(default)]
    pub traffic_up: u64,

    /// Total bytes received through this profile.
    #[serde(default)]
    pub traffic_down: u64,

    /// Whether this is the profile currently running.
    #[serde(default)]
    pub running: bool,
}

/// Group object representing a collection of profiles or a subscription.
///
/// See api.md section "Objects -> Group".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// Group ID.
    pub id: i64,

    /// Group name shown in the UI.
    pub name: String,

    /// Whether this group was created from a subscription URL.
    pub subscription: bool,

    /// Whether this group is archived.
    pub archived: bool,

    /// Number of profiles in this group.
    pub profile_count: u64,

    /// Unix timestamp in seconds of last subscription update (0 for plain groups).
    pub updated_at: i64,
}

/// Routing profile object.
///
/// The first three fields are in api.md. The rest come from the routing-profile
/// proposal (`docs/throne-api/routes-proposal.md`) and default when an older
/// Throne omits them.
///
/// See api.md section "Objects -> Route".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// Route profile ID.
    pub id: i64,

    /// Route profile name.
    pub name: String,

    /// Whether this is the currently active route profile.
    pub active: bool,

    /// The profile follows `remote_url`; its rules are replaced on every update.
    #[serde(default)]
    pub remote: bool,

    /// Source URL of a remote profile, or `""` when not remote.
    #[serde(default)]
    pub remote_url: String,

    /// A remote profile is updated by the `route_auto_update` timer.
    #[serde(default)]
    pub auto_update: bool,

    /// Last successful remote update in epoch seconds, `0` when never or not remote.
    #[serde(default)]
    pub updated_at: i64,

    /// The profile is a raw sing-box `route` object rather than a list of rules.
    #[serde(default)]
    pub raw: bool,

    /// Number of rules (`0` for a raw profile).
    #[serde(default)]
    pub rule_count: u64,
}

/// Connection and running state of Throne.
///
/// See api.md section "Objects -> Status" and "Headless Throne".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Idle,
    Connecting,
    Running,
    Stopping,
    Starting,
    #[serde(other)]
    Unknown,
}

/// Kill switch state.
///
/// See api.md section "Objects -> Status" and "Settings -> Kill switch".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillSwitchState {
    Off,
    Arming,
    Armed,
    Failed,
    #[serde(other)]
    Unknown,
}

/// Error reason when kill switch fails.
///
/// See api.md section "Objects -> Status" and "Settings -> Kill switch".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillSwitchError {
    Privilege,
    Unsupported,
    Config,
    Overridden,
    Tampered,
    Crash,
    Timeout,
    Internal,
    #[serde(other)]
    Unknown,
}

/// Reason why Throne cannot work as set up.
///
/// See api.md section "Objects -> Status" and "Headless Throne".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradedReason {
    CoreGaveUp,
    CoreUnavailable,
    KillSwitchFailed,
    ApiUnavailable,
    TunUnavailable,
    ResumeSuppressed,
    VpnAuthFailed,
    VpnChallengePending,
    #[serde(other)]
    Unknown,
}

/// State of the Throne API listener.
///
/// See api.md section "Headless Throne".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiListenerState {
    Off,
    Listening,
    KeyInvalid,
    Failed,
    Retrying,
    AddressMissing,
    #[serde(other)]
    Unknown,
}

/// Inbound proxy listener status.
///
/// See api.md section "Objects -> Status".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundStatus {
    /// Whether the mixed inbound is enabled.
    pub enabled: bool,

    /// Listen IP address.
    pub address: String,

    /// Listen port.
    pub port: u16,
}

/// System capabilities reported by Throne.
///
/// See api.md section "Objects -> Status".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Whether Throne can run Tun (`None` while privileged check is in progress).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun: Option<bool>,

    /// Whether the kill switch is supported and functional on this system.
    pub kill_switch: bool,

    /// Whether Throne is running headless without a window.
    pub headless: bool,
}

/// Status of the Throne engine, connection, modes, and capabilities.
///
/// See api.md section "Objects -> Status".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Overall connection state: idle, connecting, running, stopping.
    pub state: ConnectionState,

    /// The currently running profile, or None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,

    /// Profile ID that {"select": "last"} would start (-1 if none).
    pub last_profile_id: i64,

    /// Whether Tun mode is enabled.
    pub tun: bool,

    /// Human-readable explanation if Tun cannot be turned on due to a conflict, or None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun_conflict: Option<String>,

    /// Whether system proxy mode is enabled.
    pub system_proxy: bool,

    /// The active routing profile, or None if deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<Route>,

    /// Kill switch state: off, arming, armed, failed.
    pub kill_switch: KillSwitchState,

    /// Why kill switch failed (None unless kill_switch is failed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kill_switch_error: Option<KillSwitchError>,

    /// Whether Throne's core process is up.
    pub core: bool,

    /// Configured mixed proxy (SOCKS and HTTP) inbound.
    pub inbound: InboundStatus,

    /// Throne version string.
    pub version: String,

    /// Platform capabilities.
    pub capabilities: Capabilities,

    /// True if pending changes (e.g. settings) require profile restart.
    pub restart_needed: bool,

    /// Reason why Throne is operating in degraded mode, or None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded: Option<DegradedReason>,

    /// API listener status.
    ///
    /// NOTE: api.md describes `api` under "Headless Throne" health checks, and the
    /// user prompt requests an "api block" in Status. Kept optional for compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiListenerState>,
}

/// Status response alias for `GET /v1/status`.
///
/// See api.md section "Status -> GET /v1/status".
pub type StatusResponse = Status;

/// Kind of test job.
///
/// See api.md section "Objects -> Job" and "Tests -> POST /v1/tests".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Latency,
    Ip,
    Speed,
    #[serde(other)]
    Unknown,
}

/// State of a background test job.
///
/// See api.md section "Objects -> Job".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Done,
    Cancelled,
    #[serde(other)]
    Unknown,
}

/// Test job object.
///
/// See api.md section "Objects -> Job".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// Job ID.
    pub id: i64,

    /// Kind of test: latency, ip, speed.
    pub kind: JobKind,

    /// Current job execution state.
    pub state: JobState,

    /// Profile IDs being tested, in test order.
    #[serde(default)]
    pub profile_ids: Vec<i64>,

    /// Unix timestamp in seconds when the job started.
    pub started_at: i64,

    /// Unix timestamp in seconds when the job finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,

    /// Tested profile objects as they are after the test (only present when state is not running).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<Profile>>,
}

/// Outcome of a subscription update.
///
/// See api.md section "Objects -> Update result" and "POST /v1/groups/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateResult {
    /// Whether the subscription update succeeded.
    pub ok: bool,

    /// Group ID (present when returned as part of `POST /v1/groups/update` results).
    ///
    /// NOTE: api.md "POST /v1/groups/update" specifies: "Returns an update result with the group id for each group".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,

    /// Count of newly added profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,

    /// Count of updated profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<u64>,

    /// Count of removed profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<u64>,

    /// Profiles no longer in subscription kept because they are in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept: Option<u64>,

    /// Error description if update failed (`ok` is false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Groups and Profiles (api.md: Groups and profiles)
// ============================================================================

/// Response body for `GET /v1/groups`.
///
/// See api.md section "Groups and profiles -> GET /v1/groups".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupsResponse {
    /// Groups in Throne's tab order.
    #[serde(default)]
    pub groups: Vec<Group>,
}

/// Query parameters for `GET /v1/profiles`.
///
/// See api.md section "Groups and profiles -> GET /v1/profiles".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProfilesQuery {
    /// Only this group's profiles, in Throne's order.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_i64_lenient"
    )]
    pub group: Option<i64>,

    /// Only profile names matching this regular expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Only these profiles, comma-separated (e.g. `"12,15"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<String>,
}

/// Response body for `GET /v1/profiles`.
///
/// See api.md section "Groups and profiles -> GET /v1/profiles".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilesResponse {
    /// Profiles matching the query, in tab and group order.
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

/// Import type for `POST /v1/import`.
///
/// See api.md section "Groups and profiles -> POST /v1/import".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportAs {
    Profiles,
    Url,
    #[serde(other)]
    Unknown,
}

/// Severity level of an import issue.
///
/// See api.md section "Groups and profiles -> POST /v1/import".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportIssueSeverity {
    Error,
    Warning,
    #[serde(other)]
    Unknown,
}

/// Problem detail for entries in an import request.
///
/// See api.md section "Groups and profiles -> POST /v1/import".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportIssue {
    /// Severity: error (not imported) or warning.
    pub severity: ImportIssueSeverity,

    /// Concerned field.
    pub field: String,

    /// Human-readable message.
    pub message: String,

    /// Number of entries with this issue.
    pub count: u64,

    /// Example inputs exhibiting the problem (credentials sanitized).
    #[serde(default)]
    pub examples: Vec<String>,
}

/// Request body for `POST /v1/import`.
///
/// See api.md section "Groups and profiles -> POST /v1/import".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRequest {
    /// Import mode: "profiles" (import `text`) or "url" (download `url` once).
    pub r#as: ImportAs,

    /// Share links, subscription body, or config file content (required when `as` is "profiles").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// HTTP or HTTPS URL to download and import (required when `as` is "url").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Group ID to import into (default: currently active group in Throne).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<i64>,

    /// Wait for import to complete (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `POST /v1/import`.
///
/// NOTE: On 202 timeout, Throne returns `{"group": 3, "state": "queued"}`.
///
/// See api.md section "Groups and profiles -> POST /v1/import".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportResponse {
    /// Target group ID.
    pub group: i64,

    /// Queued state for 202 responses (e.g. "queued").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,

    /// How many profiles were successfully imported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported: Option<u64>,

    /// IDs of the newly added profiles.
    #[serde(default)]
    pub profile_ids: Vec<i64>,

    /// Entries that could not be imported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<u64>,

    /// Entries imported with warnings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warned: Option<u64>,

    /// Issues grouped by problem.
    #[serde(default)]
    pub issues: Vec<ImportIssue>,

    /// Document-level failures (e.g. parse errors).
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Request body for `POST /v1/groups`.
///
/// See api.md section "Groups and profiles -> POST /v1/groups".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateGroupRequest {
    /// Group name (required, non-empty).
    pub name: String,

    /// Subscription URL (makes this a subscription group).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Whether this subscription group participates in automatic updates (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,

    /// Download the subscription immediately upon creation (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<bool>,

    /// Wait for first update to complete (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `POST /v1/groups`.
///
/// See api.md section "Groups and profiles -> POST /v1/groups".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateGroupResponse {
    /// The created group.
    pub group: Group,

    /// Update result if an update ran (`None` on 202 or if `update` was false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdateResult>,
}

/// Request body for `POST /v1/groups/{id}/update`.
///
/// See api.md section "Groups and profiles -> POST /v1/groups/{id}/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GroupUpdateRequest {
    /// Wait for update to complete (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `POST /v1/groups/{id}/update`.
///
/// See api.md section "Groups and profiles -> POST /v1/groups/{id}/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdateResponse {
    /// The updated group.
    pub group: Group,

    /// Update result (`None` on 202 timeout).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdateResult>,
}

/// Request body for `POST /v1/groups/update` (update all subscriptions).
///
/// See api.md section "Groups and profiles -> POST /v1/groups/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UpdateAllGroupsRequest {
    /// Wait for updates to complete (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 120).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `POST /v1/groups/update`.
///
/// NOTE: On 202 timeout, Throne returns `{"queued": [3, 4]}`.
///
/// See api.md section "Groups and profiles -> POST /v1/groups/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UpdateAllGroupsResponse {
    /// Results for each subscription group updated.
    #[serde(default)]
    pub results: Vec<UpdateResult>,

    /// Group IDs queued for background update on 202 timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued: Option<Vec<i64>>,
}

/// Request body for `DELETE /v1/groups/{id}`.
///
/// See api.md section "Groups and profiles -> DELETE /v1/groups/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DeleteGroupRequest {
    /// Wait for running profile to stop before deleting (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `DELETE /v1/groups/{id}`.
///
/// See api.md section "Groups and profiles -> DELETE /v1/groups/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteGroupResponse {
    /// ID of the deleted group.
    pub deleted: i64,

    /// State if 202 timeout occurred while stopping ("stopping").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Request body for `DELETE /v1/profiles`.
///
/// See api.md section "Groups and profiles -> DELETE /v1/profiles".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DeleteProfilesRequest {
    /// Explicit profile IDs to delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<i64>>,

    /// Group ID whose profiles should be deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<i64>,

    /// Regular expression filter for profile names in `group`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Wait for running profile to stop before deleting (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `DELETE /v1/profiles`.
///
/// See api.md section "Groups and profiles -> DELETE /v1/profiles".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteProfilesResponse {
    /// Number of profiles deleted.
    pub deleted: u64,

    /// State if 202 timeout occurred while stopping ("stopping").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

// ============================================================================
// Connection (api.md: Connection)
// ============================================================================

/// Profile selection strategy for starting a connection.
///
/// See api.md section "Connection -> PUT /v1/connection".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSelectMode {
    Last,
    LowestLatency,
    #[serde(other)]
    Unknown,
}

/// Request body for `PUT /v1/connection`.
///
/// See api.md section "Connection -> PUT /v1/connection".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StartConnectionRequest {
    /// Specific profile ID to start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<i64>,

    /// Selection strategy: "last" or "lowest_latency".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<ProfileSelectMode>,

    /// Group ID for `lowest_latency` selection (default: currently active group).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<i64>,

    /// Regex filter for `lowest_latency` selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Max age in seconds of cached test results for `lowest_latency` (default: 3600).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age: Option<u64>,

    /// Restart even if already running (default: false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<bool>,

    /// Wait for connection to start (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Request body for `DELETE /v1/connection`.
///
/// See api.md section "Connection -> DELETE /v1/connection".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StopConnectionRequest {
    /// Wait for profile to stop (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

// ============================================================================
// Modes and Routing (api.md: Modes and routing)
// ============================================================================

/// Request body for `PUT /v1/modes`.
///
/// See api.md section "Modes and routing -> PUT /v1/modes".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SetModesRequest {
    /// Enable or disable Tun mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun: Option<bool>,

    /// Enable or disable system proxy mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_proxy: Option<bool>,

    /// Wait for running profile restart (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `GET /v1/routes`.
///
/// See api.md section "Modes and routing -> GET /v1/routes".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutesResponse {
    /// List of routing profiles.
    #[serde(default)]
    pub routes: Vec<Route>,
}

/// Request body for `PUT /v1/routes/active`.
///
/// See api.md section "Modes and routing -> PUT /v1/routes/active".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetActiveRouteRequest {
    /// ID of the routing profile to activate.
    pub id: i64,

    /// Wait for running profile restart (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

// ============================================================================
// Tests (api.md: Tests)
// ============================================================================

/// Request body for `POST /v1/tests`.
///
/// See api.md section "Tests -> POST /v1/tests".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartTestRequest {
    /// Kind of test: latency, ip, speed.
    pub kind: JobKind,

    /// Explicit profile IDs to test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<i64>>,

    /// Group ID whose profiles should be tested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<i64>,

    /// Regex filter for profile names in `group`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Wait for test to finish before responding (default: false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds when `wait` is true (1 to 300, default: 300).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `GET /v1/tests`.
///
/// See api.md section "Tests -> GET /v1/tests".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobsResponse {
    /// Recent test jobs, newest first, without profile `results`.
    #[serde(default)]
    pub jobs: Vec<Job>,
}

/// Request body for `DELETE /v1/results`.
///
/// See api.md section "Tests -> DELETE /v1/results".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClearResultsRequest {
    /// Profiles whose results should be cleared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<i64>>,

    /// Group ID whose profiles should be cleared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<i64>,

    /// Regex filter for profile names in `group`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Response body for `DELETE /v1/results`.
///
/// See api.md section "Tests -> DELETE /v1/results".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClearResultsResponse {
    /// Number of profiles whose test results were cleared.
    pub cleared: u64,
}

// ============================================================================
// Statistics (api.md: Statistics)
// ============================================================================

/// Real-time transfer rate in bytes per second.
///
/// See api.md section "Statistics -> GET /v1/stats/live".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveRate {
    /// Upload rate in bytes per second over the last second.
    pub up: u64,

    /// Download rate in bytes per second over the last second.
    pub down: u64,
}

/// Response body for `GET /v1/stats/live`.
///
/// See api.md section "Statistics -> GET /v1/stats/live".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveStatsResponse {
    /// Traffic through the proxy inbound.
    pub proxy: LiveRate,

    /// Direct traffic bypassing the proxy.
    pub direct: LiveRate,
}

/// Grouping dimension for traffic statistics.
///
/// See api.md section "Statistics -> GET /v1/stats/traffic".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficBy {
    Profile,
    App,
    #[serde(other)]
    Unknown,
}

/// Query parameters for `GET /v1/stats/traffic`.
///
/// See api.md section "Statistics -> GET /v1/stats/traffic".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TrafficStatsQuery {
    /// Group by "profile" (default) or "app".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<TrafficBy>,

    /// Start timestamp in Unix seconds (default: 24 hours ago).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_i64_lenient"
    )]
    pub from: Option<i64>,

    /// End timestamp in Unix seconds (default: now).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_i64_lenient"
    )]
    pub to: Option<i64>,
}

/// Individual traffic summary item.
///
/// See api.md section "Statistics -> GET /v1/stats/traffic".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficStatsItem {
    /// Profile ID (omitted when `by=app`, -101 for direct traffic).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,

    /// Profile name or process/application name.
    pub name: String,

    /// Total bytes sent.
    pub up: u64,

    /// Total bytes received.
    pub down: u64,
}

/// Response body for `GET /v1/stats/traffic`.
///
/// See api.md section "Statistics -> GET /v1/stats/traffic".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficStatsResponse {
    /// Aggregation dimension.
    pub by: TrafficBy,

    /// Start timestamp in Unix seconds.
    pub from: i64,

    /// End timestamp in Unix seconds.
    pub to: i64,

    /// Traffic items sorted by total traffic descending.
    #[serde(default)]
    pub items: Vec<TrafficStatsItem>,
}

// ============================================================================
// Settings (api.md: Settings)
// ============================================================================

/// Raw key-value representation of Throne settings.
///
/// See api.md section "Settings".
pub type SettingsMap = BTreeMap<String, serde_json::Value>;

/// Strongly-typed representation of known Throne settings.
///
/// NOTE: api.md notes that absent settings depend on platform, and stored values
/// are returned as-is (even invalid ones). Unrecognized or extra settings are
/// captured in `extra`.
///
/// See api.md section "Settings".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Remember the running profile and resume it on startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remember_enable: Option<bool>,

    /// Core log level: trace, debug, info, warn, error, fatal, panic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,

    /// Xray log level: debug, info, warning, error, none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xray_log_level: Option<String>,

    /// Mixed (SOCKS/HTTP) inbound enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound_enabled: Option<bool>,

    /// Listen IP address of mixed inbound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound_address: Option<String>,

    /// Port of mixed inbound (1 to 65535).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound_port: Option<u16>,

    /// URL for URL latency tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_url: Option<String>,

    /// Profiles tested simultaneously (1 to 499).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_concurrency: Option<u32>,

    /// URL test timeout in milliseconds (0 = core default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_timeout_ms: Option<u64>,

    /// Minutes between automatic subscription updates (0 = off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_auto_update: Option<u32>,

    /// Respect server-requested update interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_respect_server_interval: Option<bool>,

    /// Minutes between automatic route updates (0 = off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_auto_update: Option<u32>,

    /// Kill switch enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kill_switch: Option<bool>,

    /// Tun strict routing enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun_strict_route: Option<bool>,

    /// Tun auto redirect (Linux only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun_auto_redirect: Option<bool>,

    /// Tun MTU (1000 to 10000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun_mtu: Option<u32>,

    /// Tun handles IPv6 traffic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun_ipv6: Option<bool>,

    /// Tun IPv4 CIDR (e.g. "172.19.0.1/24").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun_ipv4_cidr: Option<String>,

    /// Tun IPv6 CIDR (e.g. "fdfe:dcba:9876::1/96").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun_ipv6_cidr: Option<String>,

    /// DNS inbound server port on 127.0.0.1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_in_port: Option<u16>,

    /// Clash API port (0 = off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clash_api_port: Option<u16>,

    /// Clash API listen address ("" for 127.0.0.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clash_listen_addr: Option<String>,

    /// sing-box API port (0 = off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub singbox_api_port: Option<u16>,

    /// Remote DNS server for proxied traffic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_dns: Option<String>,

    /// Direct DNS server for non-proxied traffic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_dns: Option<String>,

    /// Core keeps time via NTP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ntp_enabled: Option<bool>,

    /// NTP server host ("" for default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ntp_server: Option<String>,

    /// NTP check interval (e.g. "30m").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ntp_interval: Option<String>,

    /// Platform-specific or unrecognized settings.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Response body for `GET /v1/settings`.
///
/// Can be used as `GetSettingsResponse<Settings>` or `GetSettingsResponse<SettingsMap>`.
///
/// See api.md section "Settings -> GET /v1/settings".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetSettingsResponse<T = serde_json::Value> {
    /// Settings dictionary.
    pub settings: T,
}

/// Convenience alias for `GET /v1/settings` with raw JSON values.
pub type SettingsResponse = GetSettingsResponse<serde_json::Value>;

/// Request body for `PUT /v1/settings`.
///
/// See api.md section "Settings -> PUT /v1/settings".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateSettingsRequest<T = serde_json::Value> {
    /// Map of setting names and their new values. `null` leaves setting untouched.
    pub settings: T,
}

/// Response body for `PUT /v1/settings`.
///
/// See api.md section "Settings -> PUT /v1/settings".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateSettingsResponse<T = serde_json::Value> {
    /// Every setting after the change.
    pub settings: T,

    /// Sorted list of setting names that actually changed.
    #[serde(default)]
    pub changed: Vec<String>,

    /// True if profile restart is required for changes to take effect.
    pub restart_needed: bool,
}

// ============================================================================
// Rule-Sets and Geo Files (api.md: Rule-sets and geo files)
// ============================================================================

/// Request body for `POST /v1/rule-sets/update`.
///
/// See api.md section "Rule-sets and geo files -> POST /v1/rule-sets/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RuleSetsUpdateRequest {
    /// Wait for download to finish (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 90).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Rule-set update failure record.
///
/// See api.md section "Rule-sets and geo files -> POST /v1/rule-sets/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSetFailure {
    /// Rule-set tag that failed.
    pub tag: String,

    /// Error message.
    pub error: String,
}

/// Response body for `POST /v1/rule-sets/update`.
///
/// See api.md section "Rule-sets and geo files -> POST /v1/rule-sets/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RuleSetsUpdateResponse {
    /// Number of successfully updated rule-sets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<u64>,

    /// Rule-sets that failed to update.
    #[serde(default)]
    pub failures: Vec<RuleSetFailure>,

    /// Status if 202 timeout occurred ("updating").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Geo asset file information.
///
/// See api.md section "Rule-sets and geo files -> GET /v1/geo-assets".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeoAssetFile {
    /// Asset filename (e.g. "geoip.dat", "geosite.dat").
    pub name: String,

    /// Download URL.
    pub url: String,

    /// Whether the file is present on disk.
    pub present: bool,

    /// File size in bytes (0 if not present).
    pub size: u64,

    /// Unix timestamp of last update (0 if not present).
    pub updated_at: i64,
}

/// Response body for `GET /v1/geo-assets`.
///
/// See api.md section "Rule-sets and geo files -> GET /v1/geo-assets".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeoAssetsResponse {
    /// Available geo asset files.
    #[serde(default)]
    pub files: Vec<GeoAssetFile>,
}

/// Request body for `POST /v1/geo-assets/update`.
///
/// See api.md section "Rule-sets and geo files -> POST /v1/geo-assets/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GeoAssetsUpdateRequest {
    /// Files to download (defaults to `["geoip", "geosite"]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,

    /// Skip files already present on disk (default: false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_missing: Option<bool>,

    /// Wait for download to finish (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 300).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Result of downloading an individual geo asset file.
///
/// See api.md section "Rule-sets and geo files -> POST /v1/geo-assets/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeoAssetUpdateResult {
    /// Asset filename.
    pub name: String,

    /// Whether the update was successful.
    pub ok: bool,

    /// Whether download was skipped because file is already present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<bool>,

    /// Error message if download failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response body for `POST /v1/geo-assets/update`.
///
/// See api.md section "Rule-sets and geo files -> POST /v1/geo-assets/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GeoAssetsUpdateResponse {
    /// Outcome for each file.
    #[serde(default)]
    pub files: Vec<GeoAssetUpdateResult>,

    /// State if 202 timeout occurred ("downloading").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

// ============================================================================
// Backup and Restore (api.md: Backup and restore)
// ============================================================================

/// Request body for `POST /v1/backups`.
///
/// See api.md section "Backup and restore -> POST /v1/backups".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BackupRequest {
    /// Parts to include: profiles, routes, settings, otp, ip_lists, icons (default: all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<String>>,

    /// Wait for archive build to finish (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 120).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `POST /v1/backups`.
///
/// See api.md section "Backup and restore -> POST /v1/backups".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupResponse {
    /// Backup ID.
    pub id: i64,

    /// Archive size in bytes (None if 202 building).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// Hexadecimal SHA-256 digest of the archive (None if 202 building).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,

    /// Parts included in the archive.
    #[serde(default)]
    pub parts: Vec<String>,

    /// State if 202 timeout occurred ("building").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Query parameters for `GET /v1/backups/{id}`.
///
/// See api.md section "Backup and restore -> GET /v1/backups/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BackupChunkQuery {
    /// Byte offset into the archive (default: 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,

    /// Chunk length in bytes (default: 4194304, max: 16777216).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
}

/// Response body for `GET /v1/backups/{id}`.
///
/// See api.md section "Backup and restore -> GET /v1/backups/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupChunkResponse {
    /// Backup ID.
    pub id: i64,

    /// Total archive size in bytes.
    pub size: u64,

    /// Hex SHA-256 checksum of the entire archive.
    pub sha256: String,

    /// Parts included in the backup.
    #[serde(default)]
    pub parts: Vec<String>,

    /// Offset of this chunk.
    pub offset: u64,

    /// Base64-encoded chunk data.
    pub data: String,

    /// True if this chunk reaches the end of the archive.
    pub eof: bool,
}

/// Response body for `DELETE /v1/backups/{id}`.
///
/// See api.md section "Backup and restore -> DELETE /v1/backups/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteBackupResponse {
    /// ID of the deleted backup.
    pub deleted: i64,
}

/// Request body for `POST /v1/restores`.
///
/// See api.md section "Backup and restore -> POST /v1/restores".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreOpenRequest {
    /// Archive size in bytes.
    pub size: u64,

    /// 64-character hexadecimal SHA-256 checksum.
    pub sha256: String,
}

/// Response body for `POST /v1/restores`.
///
/// See api.md section "Backup and restore -> POST /v1/restores".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreOpenResponse {
    /// Restore session ID.
    pub id: i64,

    /// Maximum bytes allowed per chunk.
    pub chunk_limit: u64,
}

/// Request body for `PUT /v1/restores/{id}`.
///
/// See api.md section "Backup and restore -> PUT /v1/restores/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreChunkRequest {
    /// Byte offset where this chunk begins.
    pub offset: u64,

    /// Base64-encoded chunk data.
    pub data: String,

    /// Wait for inspection when upload finishes (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 120).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Metadata describing an archive once verified.
///
/// See api.md section "Backup and restore -> PUT /v1/restores/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveMetadata {
    /// Human-readable creation timestamp.
    pub created_at: String,

    /// Origin platform: "winnt", "linux", "darwin".
    pub platform: String,

    /// Parts present in the archive.
    #[serde(default)]
    pub parts: Vec<String>,
}

/// Response body for `PUT /v1/restores/{id}`.
///
/// See api.md section "Backup and restore -> PUT /v1/restores/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreChunkResponse {
    /// Restore session ID.
    pub id: i64,

    /// Expected total size in bytes.
    pub size: u64,

    /// Bytes received so far.
    pub received: u64,

    /// Archive metadata (present on the final chunk when inspection succeeds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<ArchiveMetadata>,

    /// State if final chunk timed out during inspection ("inspecting").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// State of an upload restore session.
///
/// See api.md section "Backup and restore -> GET /v1/restores/{id}".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreState {
    Uploading,
    Inspecting,
    Complete,
    Failed,
    #[serde(other)]
    Unknown,
}

/// Response body for `GET /v1/restores/{id}`.
///
/// See api.md section "Backup and restore -> GET /v1/restores/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreStatusResponse {
    /// Restore session ID.
    pub id: i64,

    /// Expected total archive size in bytes.
    pub size: u64,

    /// Bytes received so far.
    pub received: u64,

    /// Current session state.
    pub state: RestoreState,

    /// Archive description (present in state "complete").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<ArchiveMetadata>,

    /// Reason for failure (present in state "failed").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Request body for `POST /v1/restores/{id}/apply`.
///
/// See api.md section "Backup and restore -> POST /v1/restores/{id}/apply".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RestoreApplyRequest {
    /// Parts to restore (defaults to all parts in archive except "settings" and "icons").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<String>>,

    /// Wait for application and shutdown before responding (default: true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Timeout in seconds (default: 60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `POST /v1/restores/{id}/apply`.
///
/// See api.md section "Backup and restore -> POST /v1/restores/{id}/apply".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RestoreApplyResponse {
    /// Restored parts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored: Option<Vec<String>>,

    /// Routing rules from the archive that could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_rules: Option<u64>,

    /// Whether Throne is restarting to apply changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restarting: Option<bool>,

    /// Restore ID on 202 timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,

    /// State if 202 timeout occurred ("applying").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Response body for `DELETE /v1/restores/{id}`.
///
/// See api.md section "Backup and restore -> DELETE /v1/restores/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteRestoreResponse {
    /// ID of the discarded upload session.
    pub deleted: i64,
}

// ============================================================================
// Program (api.md: Program & Headless Throne)
// ============================================================================

/// Response body for `POST /v1/restart`.
///
/// See api.md section "Program -> POST /v1/restart".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartResponse {
    /// True indicating Throne is restarting.
    pub restarting: bool,

    /// Whether Throne resumes the running profile after restart.
    pub resume: bool,
}

/// Response body for `POST /v1/shutdown`.
///
/// See api.md section "Program -> POST /v1/shutdown".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownResponse {
    /// True indicating Throne is shutting down.
    pub stopping: bool,
}

/// Previous exit information reported by `--health`.
///
/// See api.md section "Headless Throne".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviousExit {
    /// Exit code.
    pub code: i32,

    /// Human-readable explanation.
    pub reason: String,

    /// Unix timestamp in seconds when the process exited.
    pub at: i64,
}

/// JSON object output by `Throne --health`.
///
/// See api.md section "Headless Throne".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthStatus {
    /// True if Throne is healthy.
    pub ok: bool,

    /// True if Throne has finished starting.
    pub ready: bool,

    /// Current connection state ("starting" until fully started).
    pub state: ConnectionState,

    /// Degraded reason if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded: Option<DegradedReason>,

    /// Seconds profile has been stuck starting or stopping.
    pub stuck_s: u64,

    /// Count of core restarts.
    pub core_restarts: u64,

    /// Count of dialogs skipped because Throne is headless.
    pub headless_dialogs: u64,

    /// Whether previous run crashed.
    pub previous_crash: bool,

    /// Previous exit details, or None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_exit: Option<PreviousExit>,

    /// Status of the API listener.
    pub api: ApiListenerState,
}

// ============================================================================
// Routing profiles (docs/throne-api/routes-proposal.md)
// ============================================================================

fn default_route_kind() -> String {
    "throne-route-profile".to_owned()
}

const fn default_route_version() -> u32 {
    1
}

fn default_rule_type() -> String {
    "custom".to_owned()
}

fn default_rule_action() -> String {
    "route".to_owned()
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Deserializes an optional list of strings. Numbers are accepted too and kept
/// as their decimal text, since sing-box writes some lists (such as ports) as
/// numbers.
fn deserialize_opt_string_list<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(items) = Option::<Vec<serde_json::Value>>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let mut texts = Vec::with_capacity(items.len());
    for item in items {
        match item {
            serde_json::Value::String(text) => texts.push(text),
            serde_json::Value::Number(number) => texts.push(number.to_string()),
            other => {
                return Err(serde::de::Error::custom(format!(
                    "expected a string or a number, found {other}"
                )))
            }
        }
    }
    Ok(Some(texts))
}

/// Serializes a list of ports: each entry that parses as an integer is written
/// as a JSON number, anything else as a string. Throne reads both.
fn serialize_opt_port_list<S>(ports: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match ports {
        None => serializer.serialize_none(),
        Some(ports) => ports
            .iter()
            .map(|port| match port.parse::<u32>() {
                Ok(number) => serde_json::Value::from(number),
                Err(_) => serde_json::Value::from(port.as_str()),
            })
            .collect::<Vec<serde_json::Value>>()
            .serialize(serializer),
    }
}

/// Outbound of a route rule or of a profile's default.
///
/// Serialized as its name. A name this crate does not know is kept in
/// [`Outbound::Unknown`] and written back unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Outbound {
    /// Through the running profile.
    #[default]
    Proxy,
    /// Straight out of this computer.
    Direct,
    /// Dropped.
    Block,
    /// Bypasses Cloudflare WARP.
    WarpBypass,
    /// Any other outbound name, kept as written.
    Unknown(String),
}

impl From<String> for Outbound {
    fn from(name: String) -> Self {
        match name.as_str() {
            "proxy" => Self::Proxy,
            "direct" => Self::Direct,
            "block" => Self::Block,
            "warp-bypass" => Self::WarpBypass,
            _ => Self::Unknown(name),
        }
    }
}

impl From<Outbound> for String {
    fn from(outbound: Outbound) -> Self {
        match outbound {
            Outbound::Proxy => "proxy".to_owned(),
            Outbound::Direct => "direct".to_owned(),
            Outbound::Block => "block".to_owned(),
            Outbound::WarpBypass => "warp-bypass".to_owned(),
            Outbound::Unknown(name) => name,
        }
    }
}

/// One rule of a routing profile: sing-box route-rule conditions plus Throne's
/// own `name`, `type`, `action` and `disabled`.
///
/// Conditions this struct does not name are kept in `extra`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteRule {
    /// Rule name shown by Throne.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,

    /// `custom`, or one of Throne's simple rule types.
    #[serde(default = "default_rule_type", rename = "type")]
    pub r#type: String,

    /// Whether the rule is switched off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,

    /// What the rule does, such as `route` or `hijack-dns`.
    #[serde(default = "default_rule_action")]
    pub action: String,

    /// Outbound of a `route` rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outbound: Option<Outbound>,

    /// Exact domain names.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub domain: Option<Vec<String>>,

    /// Domain suffixes.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub domain_suffix: Option<Vec<String>>,

    /// Domain keywords.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub domain_keyword: Option<Vec<String>>,

    /// Domain regular expressions.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub domain_regex: Option<Vec<String>>,

    /// Destination IP CIDR ranges.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub ip_cidr: Option<Vec<String>>,

    /// Whether the destination is a private address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_is_private: Option<bool>,

    /// Source IP CIDR ranges.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_ip_cidr: Option<Vec<String>>,

    /// Destination ports. Written as numbers when they are integers, as Throne does.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        serialize_with = "serialize_opt_port_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub port: Option<Vec<String>>,

    /// Destination port ranges such as `"1000:2000"`.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub port_range: Option<Vec<String>>,

    /// Source ports. Written as numbers when they are integers, as Throne does.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        serialize_with = "serialize_opt_port_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_port: Option<Vec<String>>,

    /// Source port ranges such as `"1000:2000"`.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_port_range: Option<Vec<String>>,

    /// Process names.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub process_name: Option<Vec<String>>,

    /// Process paths.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub process_path: Option<Vec<String>>,

    /// Rule-set names (see `GET /v1/rule-sets`) or rule-set URLs.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_list",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_set: Option<Vec<String>>,

    /// Inverts the match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invert: Option<bool>,

    /// Network: `tcp` or `udp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Sniffed protocol, such as `dns`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,

    /// Every other field, kept as written.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// The share object of a routing profile: the JSON a `throne://route/` link
/// holds (base64url of it), and the body of `POST /v1/routes` and
/// `PUT /v1/routes/{id}`.
///
/// Fields this struct does not name (`dns`, `raw`, `route`, `endpoints`,
/// `outbound_names`, ...) are kept in `extra`, so reading a profile, editing it
/// and writing it back loses nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteShare {
    /// Always `throne-route-profile`.
    #[serde(default = "default_route_kind")]
    pub kind: String,

    /// Format version, `1`.
    #[serde(default = "default_route_version")]
    pub v: u32,

    /// Profile name.
    #[serde(default)]
    pub name: String,

    /// Outbound for traffic that no rule matches.
    #[serde(default)]
    pub default_outbound: Outbound,

    /// Rules, in order.
    #[serde(default)]
    pub rules: Vec<RouteRule>,

    /// Every other field, kept as written.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Response body for `GET /v1/routes/{id}`.
///
/// See routes-proposal.md, section "GET /v1/routes/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetRouteResponse {
    /// The routing profile.
    pub route: Route,

    /// Its share object.
    pub profile: RouteShare,

    /// Its `throne://route/` link.
    #[serde(default)]
    pub link: String,
}

/// Request body for `POST /v1/routes`. Exactly one of `link`, `profile` and
/// `remote` is set.
///
/// The waiting fields apply to `remote`, whose first fetch may take a while.
///
/// See routes-proposal.md, section "POST /v1/routes".
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AddRouteRequest {
    /// Anything `FromShareInput` reads: a `throne://route/` link, a share object as text,
    /// or an old rules array.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,

    /// A share object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<RouteShare>,

    /// A `throne://remoteroute/` link or a single https URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,

    /// Remote only: whether the new profiles update automatically (default `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,

    /// Overrides the name of a `link` or `profile`, or of a single `remote` URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Respond when the first fetch has finished or failed (default `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Seconds to wait before responding with 202 (default `30`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Outcome of one remote fetch: `POST /v1/routes` (per profile) and
/// `POST /v1/routes/{id}/update`.
///
/// See routes-proposal.md, sections "POST /v1/routes" and "POST /v1/routes/{id}/update".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteFetchResult {
    /// Routing profile ID.
    pub id: i64,

    /// Whether the fetch succeeded.
    pub ok: bool,

    /// Why the fetch failed, when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Epoch seconds of the successful update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

/// Response body for `POST /v1/routes`.
///
/// See routes-proposal.md, section "POST /v1/routes".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddRouteResponse {
    /// The routing profiles that were added.
    #[serde(default)]
    pub routes: Vec<Route>,

    /// One entry per fetched profile, for `remote` requests only.
    #[serde(default)]
    pub results: Vec<RouteFetchResult>,

    /// What `FromShareInput` reported. May be empty.
    #[serde(default)]
    pub warnings: String,
}

/// Request body for `PUT /v1/routes/{id}`. Set exactly the fields to change.
///
/// The waiting fields apply to `profile`, which restarts the running profile when
/// this one is active.
///
/// See routes-proposal.md, section "PUT /v1/routes/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UpdateRouteRequest {
    /// Replaces the name, default outbound, rules and DNS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<RouteShare>,

    /// New name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Whether a remote profile updates automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,

    /// `false` with `profile` turns a remote profile into a local copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<bool>,

    /// Respond when the change has finished or failed (default `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,

    /// Seconds to wait before responding with 202 (default `30`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Response body for `PUT /v1/routes/{id}`.
///
/// See routes-proposal.md, section "PUT /v1/routes/{id}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateRouteResponse {
    /// The routing profile after the change.
    pub route: Route,

    /// Warnings about the change. May be empty.
    #[serde(default)]
    pub warnings: String,
}

/// Response body for `GET /v1/rule-sets`.
///
/// See routes-proposal.md, section "GET /v1/rule-sets".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSetsResponse {
    /// Rule-set names Throne knows, sorted.
    #[serde(default)]
    pub rule_sets: Vec<String>,
}

// ============================================================================
// Unit Tests (Copied verbatim from api.md)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_verbatim() {
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
    }

    #[test]
    fn test_group_verbatim() {
        let json = r#"{"id": 3, "name": "Provider", "subscription": true, "archived": false, "profile_count": 120, "updated_at": 1760000000}"#;
        let group: Group = serde_json::from_str(json).expect("failed to deserialize Group");
        assert_eq!(group.id, 3);
        assert_eq!(group.name, "Provider");
        assert!(group.subscription);
        assert!(!group.archived);
        assert_eq!(group.profile_count, 120);
        assert_eq!(group.updated_at, 1760000000);
    }

    #[test]
    fn test_route_verbatim() {
        let json = r#"{"id": 1, "name": "Default", "active": true}"#;
        let route: Route = serde_json::from_str(json).expect("failed to deserialize Route");
        assert_eq!(route.id, 1);
        assert_eq!(route.name, "Default");
        assert!(route.active);
    }

    #[test]
    fn test_status_verbatim() {
        let json = r#"{
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
        let status: Status = serde_json::from_str(json).expect("failed to deserialize Status");
        assert_eq!(status.state, ConnectionState::Running);
        assert_eq!(status.last_profile_id, 12);
        assert!(status.tun);
        assert_eq!(status.tun_conflict, None);
        assert!(!status.system_proxy);
        assert_eq!(status.kill_switch, KillSwitchState::Off);
        assert_eq!(status.kill_switch_error, None);
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

        let prof = status.profile.expect("profile should be present");
        assert_eq!(prof.id, 12);
        assert_eq!(prof.name, "DE Frankfurt");

        let route = status.route.expect("route should be present");
        assert_eq!(route.id, 1);
        assert_eq!(route.name, "Default");
        assert!(route.active);
    }

    #[test]
    fn test_job_verbatim() {
        let json = r#"{
  "id": 7,
  "kind": "latency",
  "state": "done",
  "profile_ids": [12, 15, 16],
  "started_at": 1760000000,
  "finished_at": 1760000042,
  "results": [{"id": 12, "latency_ms": 85, "...": "Profile objects"}]
}"#;
        let job: Job = serde_json::from_str(json).expect("failed to deserialize Job");
        assert_eq!(job.id, 7);
        assert_eq!(job.kind, JobKind::Latency);
        assert_eq!(job.state, JobState::Done);
        assert_eq!(job.profile_ids, vec![12, 15, 16]);
        assert_eq!(job.started_at, 1760000000);
        assert_eq!(job.finished_at, Some(1760000042));
        let results = job.results.expect("results should be present");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, 12);
        assert_eq!(results[0].latency_ms, 85);
    }

    #[test]
    fn test_update_result_verbatim() {
        let ok_json = r#"{"ok": true, "added": 3, "updated": 10, "removed": 1, "kept": 0}"#;
        let res_ok: UpdateResult =
            serde_json::from_str(ok_json).expect("failed to deserialize ok UpdateResult");
        assert!(res_ok.ok);
        assert_eq!(res_ok.added, Some(3));
        assert_eq!(res_ok.updated, Some(10));
        assert_eq!(res_ok.removed, Some(1));
        assert_eq!(res_ok.kept, Some(0));
        assert_eq!(res_ok.error, None);

        let err_json = r#"{"ok": false, "error": "fetch failed"}"#;
        let res_err: UpdateResult =
            serde_json::from_str(err_json).expect("failed to deserialize error UpdateResult");
        assert!(!res_err.ok);
        assert_eq!(res_err.error.as_deref(), Some("fetch failed"));
    }

    #[test]
    fn test_import_response_verbatim() {
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
        assert_eq!(resp.issues[0].count, 1);
        assert_eq!(resp.issues[0].examples, vec!["vless://host:70000 #name"]);
        assert!(resp.errors.is_empty());
    }

    #[test]
    fn test_group_update_all_verbatim() {
        let json = r#"{"results": [{"id": 3, "ok": true, "added": 3, "updated": 10, "removed": 1, "kept": 0}, {"id": 4, "ok": false, "error": "..."}]}"#;
        let resp: UpdateAllGroupsResponse =
            serde_json::from_str(json).expect("failed to deserialize UpdateAllGroupsResponse");
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.results[0].id, Some(3));
        assert!(resp.results[0].ok);
        assert_eq!(resp.results[0].added, Some(3));
        assert_eq!(resp.results[1].id, Some(4));
        assert!(!resp.results[1].ok);
        assert_eq!(resp.results[1].error.as_deref(), Some("..."));
    }

    #[test]
    fn test_live_stats_verbatim() {
        let json = r#"{"proxy": {"up": 1200, "down": 56000}, "direct": {"up": 0, "down": 310}}"#;
        let stats: LiveStatsResponse =
            serde_json::from_str(json).expect("failed to deserialize LiveStatsResponse");
        assert_eq!(stats.proxy.up, 1200);
        assert_eq!(stats.proxy.down, 56000);
        assert_eq!(stats.direct.up, 0);
        assert_eq!(stats.direct.down, 310);
    }

    #[test]
    fn test_traffic_stats_verbatim() {
        let json = r#"{
  "by": "profile",
  "from": 1759913600,
  "to": 1760000000,
  "items": [
    {"id": 12, "name": "DE Frankfurt", "up": 10485760, "down": 734003200},
    {"id": -101, "name": "Direct", "up": 2048, "down": 409600}
  ]
}"#;
        let stats: TrafficStatsResponse =
            serde_json::from_str(json).expect("failed to deserialize TrafficStatsResponse");
        assert_eq!(stats.by, TrafficBy::Profile);
        assert_eq!(stats.from, 1759913600);
        assert_eq!(stats.to, 1760000000);
        assert_eq!(stats.items.len(), 2);
        assert_eq!(stats.items[0].id, Some(12));
        assert_eq!(stats.items[0].name, "DE Frankfurt");
        assert_eq!(stats.items[0].up, 10485760);
        assert_eq!(stats.items[0].down, 734003200);
        assert_eq!(stats.items[1].id, Some(-101));
        assert_eq!(stats.items[1].name, "Direct");
        assert_eq!(stats.items[1].up, 2048);
        assert_eq!(stats.items[1].down, 409600);
    }

    #[test]
    fn test_health_verbatim() {
        let json = r#"{"api":"listening","core_restarts":0,"degraded":null,"headless_dialogs":0,"ok":true,"previous_crash":false,"previous_exit":{"at":1760700000,"code":75,"reason":"restart requested"},"ready":true,"state":"running","stuck_s":0}"#;
        let health: HealthStatus =
            serde_json::from_str(json).expect("failed to deserialize HealthStatus");
        assert!(health.ok);
        assert!(health.ready);
        assert_eq!(health.state, ConnectionState::Running);
        assert_eq!(health.api, ApiListenerState::Listening);
        assert_eq!(health.degraded, None);
        let exit = health
            .previous_exit
            .expect("previous_exit should be present");
        assert_eq!(exit.code, 75);
        assert_eq!(exit.reason, "restart requested");
        assert_eq!(exit.at, 1760700000);
    }

    #[test]
    fn test_settings_open_ended() {
        let json = r#"{
  "settings": {
    "log_level": "debug",
    "kill_switch": true,
    "unknown_custom_setting": 42
  }
}"#;
        let resp: GetSettingsResponse<Settings> =
            serde_json::from_str(json).expect("failed to deserialize Settings");
        assert_eq!(resp.settings.log_level.as_deref(), Some("debug"));
        assert_eq!(resp.settings.kill_switch, Some(true));
        assert_eq!(
            resp.settings.extra.get("unknown_custom_setting"),
            Some(&serde_json::json!(42))
        );
    }

    #[test]
    fn test_unknown_enum_fallbacks() {
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
}
