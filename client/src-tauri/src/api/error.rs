//! The errors the webview gets from an API call, and the removal of the key from them.
//!
//! The kinds that also exist for the connection screen (`unreachable`, `forbidden`,
//! `unauthorized`, `bad_response`, `invalid_input`, `storage`, `internal`) keep their names, so one
//! describer on the screen side serves both. The others tell apart what the server's GraphQL
//! error codes tell apart.

use serde::Serialize;
use serde_json::Value;

use crate::server::{ConnectError, ErrorKind};

/// Longest `detail` handed to the screen, in characters.
const DETAIL_LIMIT: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorKind {
    /// The operation name or the arguments are not accepted here. Nothing was sent.
    InvalidInput,
    /// No answer from the server, or no connection could be made. A mutation that was not sent.
    Unreachable,
    /// 403: this machine is not allowed (key issue), or the server refuses the request.
    Forbidden,
    /// 401, and the key could not be replaced by a new one that works.
    Unauthorized,
    /// The server answered, but not in the agreed shape (HTTP status, JSON, GraphQL shape).
    BadResponse,
    /// The saved connection could not be read or written on this machine.
    Storage,
    /// The request could not be prepared on this machine. Nothing was sent.
    Internal,
    /// No connection is saved yet.
    NotConnected,
    /// The server says the target does not exist (`NOT_FOUND`).
    NotFound,
    /// The server says the state or the version moved on (`CONFLICT`).
    Conflict,
    /// The server could not use bantou, memory or openspec (`EXTERNAL_UNAVAILABLE`). Not accepted.
    ExternalUnavailable,
    /// A change may or may not have been accepted: the request was sent and no answer came back,
    /// or the server says so (`EXTERNAL_OUTCOME_UNKNOWN`). Never resent.
    OutcomeUnknown,
    /// The server failed inside (`INTERNAL`).
    ServerError,
    /// The server does not know this operation or field (an older server).
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiError {
    pub kind: ApiErrorKind,
    pub detail: String,
    /// The server's `extensions.reason`, when it gave one (`unsupported_by_openspec_mcp` …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The server's `extensions.missing`: the tools (or inputs) of openspec-mcp that are lacking.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

impl ApiError {
    pub fn new(kind: ApiErrorKind, detail: impl Into<String>) -> Self {
        Self { kind, detail: truncate(detail.into()), reason: None, missing: Vec::new() }
    }

    pub fn with_extras(mut self, reason: Option<String>, missing: Vec<String>) -> Self {
        self.reason = reason;
        self.missing = missing;
        self
    }
}

fn truncate(text: String) -> String {
    if text.chars().count() <= DETAIL_LIMIT {
        return text;
    }
    let mut short: String = text.chars().take(DETAIL_LIMIT).collect();
    short.push('…');
    short
}

impl From<ConnectError> for ApiError {
    fn from(e: ConnectError) -> Self {
        let kind = match e.kind {
            ErrorKind::InvalidInput => ApiErrorKind::InvalidInput,
            ErrorKind::Unreachable => ApiErrorKind::Unreachable,
            ErrorKind::Forbidden => ApiErrorKind::Forbidden,
            ErrorKind::Unauthorized => ApiErrorKind::Unauthorized,
            ErrorKind::BadResponse => ApiErrorKind::BadResponse,
            ErrorKind::Storage => ApiErrorKind::Storage,
            ErrorKind::Internal => ApiErrorKind::Internal,
        };
        ApiError::new(kind, e.detail)
    }
}

const MASK: &str = "[removed]";

/// Replaces every occurrence of the key in a text.
pub fn redact(text: &str, key: &str) -> String {
    if key.is_empty() {
        text.to_owned()
    } else {
        text.replace(key, MASK)
    }
}

/// Replaces the key in every string of a value, object keys included.
pub fn redact_value(value: Value, key: &str) -> Value {
    if key.is_empty() {
        return value;
    }
    match value {
        Value::String(s) => Value::String(redact(&s, key)),
        Value::Array(items) => Value::Array(items.into_iter().map(|v| redact_value(v, key)).collect()),
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (redact(&k, key), redact_value(v, key))).collect())
        }
        other => other,
    }
}

pub fn redact_error(error: ApiError, key: &str) -> ApiError {
    ApiError {
        kind: error.kind,
        detail: redact(&error.detail, key),
        reason: error.reason.map(|r| redact(&r, key)),
        missing: error.missing.iter().map(|m| redact(m, key)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_have_the_names_the_screens_use() {
        let name = |kind| serde_json::to_string(&kind).unwrap();
        assert_eq!(name(ApiErrorKind::InvalidInput), r#""invalid_input""#);
        assert_eq!(name(ApiErrorKind::BadResponse), r#""bad_response""#);
        assert_eq!(name(ApiErrorKind::NotConnected), r#""not_connected""#);
        assert_eq!(name(ApiErrorKind::ExternalUnavailable), r#""external_unavailable""#);
        assert_eq!(name(ApiErrorKind::OutcomeUnknown), r#""outcome_unknown""#);
        assert_eq!(name(ApiErrorKind::ServerError), r#""server_error""#);
    }

    #[test]
    fn the_key_is_removed_from_texts_and_values() {
        assert_eq!(redact("a k1 b k1", "k1"), "a [removed] b [removed]");
        assert_eq!(redact("nothing", ""), "nothing");
        let value = serde_json::json!({"k1": ["x k1", 3, {"n": "k1"}], "ok": null});
        let cleaned = redact_value(value, "k1").to_string();
        assert!(!cleaned.contains("k1"), "{cleaned}");
        assert!(cleaned.contains("[removed]"));
    }

    #[test]
    fn a_long_detail_is_cut() {
        let error = ApiError::new(ApiErrorKind::BadResponse, "x".repeat(1000));
        assert_eq!(error.detail.chars().count(), DETAIL_LIMIT + 1);
    }
}
