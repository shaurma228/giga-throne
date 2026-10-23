# Routing profiles over the Throne API — proposal

Status: **draft**, implemented in our fork (`~/git/Throne`, branch `feat/route-api`, based on tag 1.4.0-beta.4)
and meant as an upstream PR to throneproj/Throne + throneproj/Throne-api. Until it is merged, giga-throne's
sidecar must be built from the fork.

Why: headless Throne can only list routing profiles and pick the active one (`GET /v1/routes`,
`PUT /v1/routes/active`). Importing the routeprofiles presets (`throne://route/…`, `throne://remoteroute/…`),
creating, editing and deleting profiles exist only in Throne's window. Everything below is a thin wrapper over
code Throne already has: `RouteProfile::FromShareInput`, `RouteProfile::FromRemoteRoutesLink`,
`RouteProfile::ToShareObject`, `RouteUpdate::UpdateProfile`, `RoutesRepo::{AddRouteProfile, Save, DeleteRouteProfile}`.

Conventions are the ones of `api.md` (envelope, errors, [Waiting](api.md#waiting), admin scope). All new requests
except the two GETs are **admin** requests.

## The Route object, extended

`GET /v1/routes` and every response below return Route objects with these fields. The first three are the
existing ones; the rest are new and always present:

```json
{"id": 2, "name": "Bypass Russia", "active": false, "remote": true,
 "remote_url": "https://raw.githubusercontent.com/throneproj/routeprofiles/profile/Bypass_Russia",
 "auto_update": true, "updated_at": 1760000000, "raw": false, "rule_count": 3}
```

| Field | |
|---|---|
| `remote` | The profile follows `remote_url`; its rules are replaced on every update |
| `remote_url` | `""` unless `remote` |
| `auto_update` | A remote profile is updated by the `route_auto_update` timer |
| `updated_at` | Last successful remote update, epoch seconds, `0` never or not remote |
| `raw` | The profile is a raw sing-box `route` object rather than rules (not editable through `rules`) |
| `rule_count` | Number of rules (`0` for a raw profile) |

## The share object

The body that carries a profile's content is Throne's share object, the same JSON a `throne://route/` link holds
(base64url of it), produced by `ToShareObject` and read by `FromShareInput`:

```json
{
  "kind": "throne-route-profile", "v": 1, "name": "Bypass Russia",
  "default_outbound": "proxy",
  "rules": [
    {"name": "dns", "type": "custom", "action": "hijack-dns", "protocol": "dns"},
    {"name": "private", "type": "custom", "action": "route", "ip_is_private": true, "outbound": "direct"},
    {"name": "ru", "type": "custom", "action": "route", "outbound": "direct",
     "rule_set": ["geoip-ru", "geosite-ru", "geosite-category-ru"]}
  ]
}
```

- `default_outbound` and a rule's `outbound`: `proxy`, `direct`, `block`, `warp-bypass`.
- A rule holds sing-box route-rule conditions (`domain`, `domain_suffix`, `domain_keyword`, `domain_regex`,
  `ip_cidr`, `ip_is_private`, `source_ip_cidr`, `port`, `port_range`, `process_name`, `process_path`,
  `network`, `protocol`, `rule_set`, `invert`, …), plus `name`, `type` (`custom` or one of Throne's simple types)
  and `disabled`. Throne validates them exactly as when importing a link.
- Optional `dns` (the profile's own DNS settings), `raw` + `route` for raw profiles, `endpoints`. A client that
  edits a profile sends back what it read, with its changes.

## `GET /v1/routes/{id}`

Returns `{"route": Route, "profile": <share object>, "link": "throne://route/…"}`. Errors: `not_found`.

## `POST /v1/routes`

Admin. Adds routing profiles. The body names the source in one of three ways:

```json
{"link": "throne://route/eyJ…"}
{"profile": {"kind": "throne-route-profile", "v": 1, "name": "Mine", "default_outbound": "proxy", "rules": []}}
{"remote": "throne://remoteroute/aHR0…", "auto_update": true}
```

| Field | |
|---|---|
| `link` | Anything `FromShareInput` reads: a `throne://route/` link, a share object as text, or an old rules array |
| `profile` | A share object |
| `remote` | A `throne://remoteroute/` link (one or more `URL#name` lines) or a single https URL. Every entry becomes a remote profile and is fetched once right away |
| `auto_update` | Remote only, default `true` |
| `name` | Optional: overrides the name (for `link`/`profile`, or a single `remote` URL) |

`remote` takes the [waiting](api.md#waiting) fields (default timeout 30) for the first fetch; on 202 the fetches
carry on. Server profiles inside a shared profile's `endpoints` are never created through the API (unmaterialized,
as for remote profiles), and a raw profile from an untrusted source is refused like a restore would refuse it.

Returns `{"routes": [Route…], "results": [{"id": 3, "ok": true}, {"id": 4, "ok": false, "error": "…"}], "warnings": "…"}`:
`results` only for `remote` (one per fetched profile; a failed fetch keeps the profile, empty, so it can be updated
later), `warnings` what `FromShareInput` reported (may be `""`).

Errors: `bad_request` (none or more than one of `link`/`profile`/`remote`, or the input does not parse — the message
says why), `editing` (the routing profiles are open in an editor in Throne).

## `PUT /v1/routes/{id}`

Admin. Changes a profile:

```json
{"profile": {"kind": "throne-route-profile", "v": 1, "name": "Mine", "default_outbound": "direct", "rules": [ … ]}}
{"name": "Renamed"}
{"auto_update": false}
```

`profile` replaces the name, default outbound, rules and DNS (a remote profile refuses it with `remote_route`: its
rules come from its URL; send `"remote": false` together with `profile` to turn it into a local copy). `name` and
`auto_update` change just that. If this is the active profile and a profile runs, the running profile restarts so the
change takes effect, so the request takes the [waiting](api.md#waiting) fields.

Returns `{"route": Route, "warnings": "…"}`. Errors: `not_found`, `bad_request`, `remote_route`, `editing`, plus the
errors of `PUT /v1/connection` for the restart.

## `DELETE /v1/routes/{id}`

Admin. Deletes a profile. Errors: `not_found`, `route_active` (activate another profile first), `last_route`
(Throne keeps at least one), `editing`.

## `POST /v1/routes/{id}/update`

Admin. Fetches a remote profile again now. Takes the [waiting](api.md#waiting) fields; restarts the running profile
if this one is active and its rules changed. Returns an [Update result](api.md#update-result)-like
`{"id": 2, "ok": true, "updated_at": 1760000000}` or `{"id": 2, "ok": false, "error": "…"}`.
Errors: `not_found`, `not_remote`.

## `GET /v1/rule-sets`

Returns the rule-set names Throne knows (its built-in list from routeprofiles' `srslist.h`), sorted, for
autocompletion in an editor: `{"rule_sets": ["geoip-ru", "geoip-ru-blocked", "geosite-ru", …]}`. A rule may still
name a rule-set by URL.
