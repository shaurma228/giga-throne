//! JSON messages carried inside frames.
//!
//! See `docs/throne-api/protocol.md`, section "Messages".

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// HTTP-like request method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    /// Read.
    #[serde(rename = "GET")]
    Get,
    /// Create or act.
    #[serde(rename = "POST")]
    Post,
    /// Replace.
    #[serde(rename = "PUT")]
    Put,
    /// Delete.
    #[serde(rename = "DELETE")]
    Delete,
}

/// A request sent from the client to Throne.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Request method.
    pub method: Method,
    /// Path starting with `/`, without query string or fragment.
    pub path: String,
    /// Optional query parameters. Every value is a string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<BTreeMap<String, String>>,
    /// Optional JSON body. A missing body and `null` mean the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

/// A response sent from Throne to the client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// HTTP-like status code.
    pub status: u16,
    /// Result body for 2xx responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    /// Error details for 4xx and 5xx responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiErrorBody>,
}

/// Error details of a failed response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorBody {
    /// Stable, machine-readable code such as `busy`.
    pub code: String,
    /// Human-readable message, may change between versions.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_skips_missing_fields() {
        let req = Request {
            method: Method::Get,
            path: "/v1/status".to_owned(),
            query: None,
            body: None,
        };
        let text = serde_json::to_string(&req).expect("encode");
        assert_eq!(text, r#"{"method":"GET","path":"/v1/status"}"#);
    }

    #[test]
    fn request_with_query_and_body() {
        let mut query = BTreeMap::new();
        query.insert("group".to_owned(), "3".to_owned());
        let req = Request {
            method: Method::Post,
            path: "/v1/x".to_owned(),
            query: Some(query),
            body: Some(json!({"wait": false})),
        };
        let value = serde_json::to_value(&req).expect("encode");
        assert_eq!(
            value,
            json!({"method": "POST", "path": "/v1/x", "query": {"group": "3"}, "body": {"wait": false}})
        );
    }

    #[test]
    fn response_parses_ok_and_error() {
        let ok: Response =
            serde_json::from_str(r#"{"status":200,"body":{"state":"idle"}}"#).expect("decode ok");
        assert_eq!(ok.status, 200);
        assert_eq!(ok.body, Some(json!({"state": "idle"})));
        let err: Response = serde_json::from_str(
            r#"{"status":409,"error":{"code":"busy","message":"Another profile is starting..."}}"#,
        )
        .expect("decode error");
        assert_eq!(err.status, 409);
        assert_eq!(err.error.map(|e| e.code), Some("busy".to_owned()));
    }
}
