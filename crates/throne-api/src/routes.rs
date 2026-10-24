//! Routing-profile requests and `throne://` link codecs.
//!
//! The requests are the ones of `docs/throne-api/routes-proposal.md`: typed
//! helpers on [`Client`] that build the body, send it and decode the response.
//! Errors come back as [`Error::Api`] with Throne's code (`remote_route`,
//! `route_active`, `last_route`, `not_remote`, ...).
//!
//! The codecs turn a share object into a `throne://route/<base64url>` link and
//! back, and split a `throne://remoteroute/<base64url>` link into its entries.
//! Links carry base64url without padding; a padded link is accepted too.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::message::Method;
use crate::types::{
    AddRouteRequest, AddRouteResponse, GetRouteResponse, Route, RouteFetchResult, RouteShare,
    RoutesResponse, RuleSetsResponse, SetActiveRouteRequest, Status, UpdateRouteRequest,
    UpdateRouteResponse, WaitParams,
};

/// Seconds the waiting requests of this module ask Throne to wait (proposal default).
const WAIT_TIMEOUT: u64 = 30;

const ROUTE_LINK_PREFIX: &str = "throne://route/";
const REMOTE_ROUTE_LINK_PREFIX: &str = "throne://remoteroute/";

fn route_path(id: i64) -> String {
    format!("/v1/routes/{id}")
}

impl Client {
    /// Lists the routing profiles (`GET /v1/routes`).
    pub async fn routes(&self) -> Result<Vec<Route>> {
        let resp: RoutesResponse = self
            .request_json(Method::Get, "/v1/routes", None, None)
            .await?;
        Ok(resp.routes)
    }

    /// Returns one routing profile with its share object and link (`GET /v1/routes/{id}`).
    pub async fn route(&self, id: i64) -> Result<GetRouteResponse> {
        self.request_json(Method::Get, &route_path(id), None, None)
            .await
    }

    /// Adds a routing profile from a `throne://route/` link or any other text
    /// `FromShareInput` reads (`POST /v1/routes` with `link`).
    pub async fn add_route_link(&self, link: &str) -> Result<AddRouteResponse> {
        self.add_routes(&AddRouteRequest {
            link: Some(link.to_owned()),
            ..Default::default()
        })
        .await
    }

    /// Adds a routing profile from a share object (`POST /v1/routes` with `profile`).
    pub async fn add_route_profile(&self, profile: &RouteShare) -> Result<AddRouteResponse> {
        self.add_routes(&AddRouteRequest {
            profile: Some(profile.clone()),
            ..Default::default()
        })
        .await
    }

    /// Adds one remote profile per entry of `remote`, a `throne://remoteroute/`
    /// link or a single https URL, and fetches each once (`POST /v1/routes` with
    /// `remote`). Waits up to 30 s for the fetches.
    pub async fn add_remote_routes(
        &self,
        remote: &str,
        auto_update: bool,
    ) -> Result<AddRouteResponse> {
        self.add_routes(&AddRouteRequest {
            remote: Some(remote.to_owned()),
            auto_update: Some(auto_update),
            timeout: Some(WAIT_TIMEOUT),
            ..Default::default()
        })
        .await
    }

    /// Replaces the name, default outbound, rules and DNS of a local profile
    /// (`PUT /v1/routes/{id}` with `profile`). Waits up to 30 s for a restart of
    /// the running profile.
    pub async fn update_route_profile(
        &self,
        id: i64,
        profile: &RouteShare,
    ) -> Result<UpdateRouteResponse> {
        self.update_route(
            id,
            &UpdateRouteRequest {
                profile: Some(profile.clone()),
                timeout: Some(WAIT_TIMEOUT),
                ..Default::default()
            },
        )
        .await
    }

    /// Renames a routing profile (`PUT /v1/routes/{id}` with `name`).
    pub async fn rename_route(&self, id: i64, name: &str) -> Result<UpdateRouteResponse> {
        self.update_route(
            id,
            &UpdateRouteRequest {
                name: Some(name.to_owned()),
                ..Default::default()
            },
        )
        .await
    }

    /// Turns automatic updates of a remote profile on or off
    /// (`PUT /v1/routes/{id}` with `auto_update`).
    pub async fn set_route_auto_update(
        &self,
        id: i64,
        auto_update: bool,
    ) -> Result<UpdateRouteResponse> {
        self.update_route(
            id,
            &UpdateRouteRequest {
                auto_update: Some(auto_update),
                ..Default::default()
            },
        )
        .await
    }

    /// Deletes a routing profile (`DELETE /v1/routes/{id}`).
    pub async fn delete_route(&self, id: i64) -> Result<()> {
        self.request(Method::Delete, &route_path(id), None, None)
            .await?;
        Ok(())
    }

    /// Fetches a remote profile again now (`POST /v1/routes/{id}/update`). Waits
    /// up to 30 s; restarts the running profile if this one is active and its
    /// rules changed.
    pub async fn refresh_route(&self, id: i64) -> Result<RouteFetchResult> {
        let body = WaitParams {
            timeout: Some(WAIT_TIMEOUT),
            ..Default::default()
        };
        self.request_json(
            Method::Post,
            &format!("{}/update", route_path(id)),
            None,
            Some(serde_json::to_value(body)?),
        )
        .await
    }

    /// Makes a routing profile active and restarts a running profile so it takes
    /// effect (`PUT /v1/routes/active`). Waits up to 30 s.
    pub async fn set_active_route(&self, id: i64) -> Result<Status> {
        let body = SetActiveRouteRequest {
            id,
            wait: None,
            timeout: Some(WAIT_TIMEOUT),
        };
        self.request_json(
            Method::Put,
            "/v1/routes/active",
            None,
            Some(serde_json::to_value(body)?),
        )
        .await
    }

    /// Lists the rule-set names Throne knows, for autocompletion (`GET /v1/rule-sets`).
    pub async fn rule_sets(&self) -> Result<RuleSetsResponse> {
        self.request_json(Method::Get, "/v1/rule-sets", None, None)
            .await
    }

    async fn add_routes(&self, body: &AddRouteRequest) -> Result<AddRouteResponse> {
        self.request_json(
            Method::Post,
            "/v1/routes",
            None,
            Some(serde_json::to_value(body)?),
        )
        .await
    }

    /// Sends any `PUT /v1/routes/{id}` body. Use it for the cases the helpers do not
    /// cover, such as `remote: false` together with `profile`, which turns a remote
    /// profile into a local copy.
    pub async fn update_route(
        &self,
        id: i64,
        body: &UpdateRouteRequest,
    ) -> Result<UpdateRouteResponse> {
        self.request_json(
            Method::Put,
            &route_path(id),
            None,
            Some(serde_json::to_value(body)?),
        )
        .await
    }
}

/// Encodes a share object as a `throne://route/<base64url>` link (no padding).
pub fn encode_route_link(profile: &RouteShare) -> String {
    // A RouteShare only has string keys, so serializing it cannot fail.
    let json = serde_json::to_vec(profile).expect("route profile serializes to JSON");
    format!("{ROUTE_LINK_PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
}

/// Decodes a `throne://route/<base64url>` link into its share object.
pub fn decode_route_link(link: &str) -> Result<RouteShare> {
    let payload = strip_prefix(link, ROUTE_LINK_PREFIX)?;
    let bytes = decode_payload(payload)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Decodes a `throne://remoteroute/<base64url>` link into `(URL, name)` pairs.
///
/// The decoded text holds one `URL#name` per line. Blank lines are skipped and
/// the name is whatever follows the first `#`.
pub fn decode_remote_route_link(link: &str) -> Result<Vec<(String, String)>> {
    let payload = strip_prefix(link, REMOTE_ROUTE_LINK_PREFIX)?;
    let bytes = decode_payload(payload)?;
    let text = String::from_utf8(bytes).map_err(|_| invalid_link("entries are not UTF-8"))?;

    let mut entries = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((url, name)) = line.split_once('#') else {
            return Err(invalid_link(format!("entry {line:?} has no #name")));
        };
        entries.push((url.to_owned(), name.to_owned()));
    }
    Ok(entries)
}

/// Encodes `(URL, name)` pairs as a `throne://remoteroute/<base64url>` link (no
/// padding), one `URL#name` per line: the form [`decode_remote_route_link`] reads.
pub fn encode_remote_route_link(entries: &[(String, String)]) -> String {
    let text: Vec<String> = entries
        .iter()
        .map(|(url, name)| format!("{url}#{name}"))
        .collect();
    format!(
        "{REMOTE_ROUTE_LINK_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(text.join("\n"))
    )
}

/// Strips `prefix` and surrounding whitespace, and returns the payload after it.
fn strip_prefix<'a>(link: &'a str, prefix: &str) -> Result<&'a str> {
    link.trim()
        .strip_prefix(prefix)
        .ok_or_else(|| invalid_link(format!("expected the prefix {prefix}")))
}

/// Decodes base64url, with or without padding.
fn decode_payload(payload: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|err| invalid_link(format!("payload is not base64url: {err}")))
}

fn invalid_link(reason: impl std::fmt::Display) -> Error {
    Error::Protocol(format!("invalid route link: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(url, name)| ((*url).to_owned(), (*name).to_owned()))
            .collect()
    }

    #[test]
    fn remote_link_round_trips_with_spaces_in_names() {
        let entries = pairs(&[
            (
                "https://example.com/Proxy_Russia_Blocked",
                "Proxy Russia Blocked",
            ),
            ("https://example.com/Bypass_Russia", "Bypass Russia"),
        ]);
        let link = encode_remote_route_link(&entries);
        assert!(link.starts_with("throne://remoteroute/"), "{link}");
        assert!(!link.ends_with('='), "no padding: {link}");
        assert_eq!(decode_remote_route_link(&link).expect("decode"), entries);
    }

    #[test]
    fn single_entry_is_one_line_without_newline() {
        let link = encode_remote_route_link(&pairs(&[("https://e.com/A", "Name")]));
        let text = URL_SAFE_NO_PAD
            .decode(link.trim_start_matches("throne://remoteroute/"))
            .expect("base64url");
        assert_eq!(text, b"https://e.com/A#Name");
    }
}
