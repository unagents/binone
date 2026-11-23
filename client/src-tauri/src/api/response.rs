//! Reads the server's answer to a GraphQL call and sorts a failure into an [`ApiErrorKind`].
//! Pure: it gets the status and the body, and touches neither the network nor Tauri.

use serde_json::Value;

use super::error::{ApiError, ApiErrorKind};
use super::ops::Operation;

/// Turns an HTTP status and body into the operation's value, or the error it stands for.
pub fn interpret(operation: Operation, status: u16, body: &[u8]) -> Result<Value, ApiError> {
    match status {
        200 => {}
        401 => return Err(ApiError::new(ApiErrorKind::Unauthorized, "POST /graphql: 401")),
        403 => return Err(ApiError::new(ApiErrorKind::Forbidden, "POST /graphql: 403")),
        300..=399 => {
            return Err(ApiError::new(
                ApiErrorKind::BadResponse,
                format!("POST /graphql: HTTP {status} (a redirect; it is not followed)"),
            ))
        }
        other => return Err(ApiError::new(ApiErrorKind::BadResponse, format!("POST /graphql: HTTP {other}"))),
    }
    let mut answer: Value = serde_json::from_slice(body)
        .map_err(|_| ApiError::new(ApiErrorKind::BadResponse, "POST /graphql: the body is not a GraphQL response"))?;
    if let Some(errors) = answer.get("errors").and_then(Value::as_array).filter(|e| !e.is_empty()) {
        return Err(graphql_error(&errors[0]));
    }
    let name = operation.name();
    let value = answer
        .get_mut("data")
        .and_then(|data| data.get_mut(name))
        .map(Value::take)
        .ok_or_else(|| ApiError::new(ApiErrorKind::BadResponse, format!("POST /graphql: no data.{name}")))?;
    if !operation.shape().accepts(&value) {
        return Err(ApiError::new(
            ApiErrorKind::BadResponse,
            format!("POST /graphql: data.{name} is not the expected shape"),
        ));
    }
    Ok(value)
}

/// The server puts a code in `extensions.code`. An error without one comes from the GraphQL
/// layer itself (a field or type this server does not have, a malformed document).
fn graphql_error(error: &Value) -> ApiError {
    let message = error.get("message").and_then(Value::as_str).unwrap_or("");
    let code = error.pointer("/extensions/code").and_then(Value::as_str);
    let kind = match code {
        Some("INVALID_INPUT") => ApiErrorKind::InvalidInput,
        Some("NOT_FOUND") => ApiErrorKind::NotFound,
        Some("CONFLICT") => ApiErrorKind::Conflict,
        Some("EXTERNAL_UNAVAILABLE") => ApiErrorKind::ExternalUnavailable,
        Some("EXTERNAL_OUTCOME_UNKNOWN") => ApiErrorKind::OutcomeUnknown,
        Some("INTERNAL") => ApiErrorKind::ServerError,
        Some(_) => ApiErrorKind::BadResponse,
        None if is_unknown_schema_item(message) => ApiErrorKind::Unsupported,
        None => ApiErrorKind::BadResponse,
    };
    let detail = match (kind, code) {
        (ApiErrorKind::BadResponse, Some(code)) => format!("POST /graphql: unknown error code {code}"),
        (ApiErrorKind::BadResponse, None) => "POST /graphql: a GraphQL error without a code".to_owned(),
        _ => message.to_owned(),
    };
    ApiError::new(kind, detail)
}

/// async-graphql's wording for a document that names something the schema lacks.
fn is_unknown_schema_item(message: &str) -> bool {
    ["Unknown field", "Unknown argument", "Unknown type", "Unknown directive"]
        .iter()
        .any(|prefix| message.starts_with(prefix))
        || message.contains("Cannot query field")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body(value: Value) -> Vec<u8> {
        value.to_string().into_bytes()
    }

    fn failed(operation: Operation, value: Value) -> ApiError {
        interpret(operation, 200, &body(value)).unwrap_err()
    }

    #[test]
    fn the_operations_value_is_returned_as_it_is() {
        let ok = interpret(Operation::ServerInfo, 200, &body(json!({"data": {"serverInfo": {"version": "1.2.3"}}})));
        assert_eq!(ok.unwrap(), json!({"version": "1.2.3"}));
        let ok = interpret(Operation::Projects, 200, &body(json!({"data": {"projects": ["a", "b"]}})));
        assert_eq!(ok.unwrap(), json!(["a", "b"]));
        // A conversation that was never registered is `null`, not a failure.
        let ok = interpret(Operation::Conversation, 200, &body(json!({"data": {"conversation": null}})));
        assert_eq!(ok.unwrap(), Value::Null);
    }

    #[test]
    fn http_statuses_are_told_apart() {
        let kind = |status| interpret(Operation::Projects, status, b"").unwrap_err().kind;
        assert_eq!(kind(401), ApiErrorKind::Unauthorized);
        assert_eq!(kind(403), ApiErrorKind::Forbidden);
        for status in [301, 302, 307, 308] {
            assert_eq!(kind(status), ApiErrorKind::BadResponse, "{status}");
        }
        for status in [400, 404, 500, 502] {
            assert_eq!(kind(status), ApiErrorKind::BadResponse, "{status}");
        }
    }

    #[test]
    fn a_refusal_of_the_check_or_the_batch_is_data_not_an_error() {
        let check = json!({"id": "r1", "ok": false, "code": "incomplete", "missing": ["protocol"], "message": "m"});
        let ok = interpret(Operation::CheckSubmitRequest, 200, &body(json!({"data": {"checkSubmitRequest": check.clone()}})));
        assert_eq!(ok.unwrap(), check);
        let batch = json!({"ok": false, "results": [check], "requests": []});
        let ok = interpret(Operation::SubmitRequests, 200, &body(json!({"data": {"submitRequests": batch.clone()}})));
        assert_eq!(ok.unwrap(), batch);
        // An answer that is not an object is not taken for one.
        let error = failed(Operation::SubmitRequests, json!({"data": {"submitRequests": true}}));
        assert_eq!(error.kind, ApiErrorKind::BadResponse);
        // A server without the operation is told apart from a broken one.
        let error = failed(Operation::CheckSubmitRequest, json!({"errors": [{"message": r#"Unknown field "checkSubmitRequest" on type "Query""#}]}));
        assert_eq!(error.kind, ApiErrorKind::Unsupported);
    }

    #[test]
    fn each_server_error_code_has_its_own_kind() {
        let cases = [
            ("INVALID_INPUT", ApiErrorKind::InvalidInput),
            ("NOT_FOUND", ApiErrorKind::NotFound),
            ("CONFLICT", ApiErrorKind::Conflict),
            ("EXTERNAL_UNAVAILABLE", ApiErrorKind::ExternalUnavailable),
            ("EXTERNAL_OUTCOME_UNKNOWN", ApiErrorKind::OutcomeUnknown),
            ("INTERNAL", ApiErrorKind::ServerError),
            ("SOMETHING_NEW", ApiErrorKind::BadResponse),
        ];
        for (code, kind) in cases {
            let error = failed(
                Operation::SaveDraft,
                json!({"data": null, "errors": [{"message": "m", "extensions": {"code": code}}]}),
            );
            assert_eq!(error.kind, kind, "{code}");
        }
        // The server's own message is kept for the codes it defines.
        let error = failed(
            Operation::SubmitRequest,
            json!({"errors": [{"message": "conflict: version 2 is not 1", "extensions": {"code": "CONFLICT"}}]}),
        );
        assert_eq!(error.detail, "conflict: version 2 is not 1");
    }

    #[test]
    fn a_schema_error_without_a_code_means_the_server_lacks_the_field() {
        for message in [
            r#"Unknown field "notifications" on type "Query""#,
            r#"Unknown argument "limit" on field "Query.notifications""#,
            r#"Unknown type "SaveDraftInput""#,
        ] {
            let error = failed(Operation::Notifications, json!({"errors": [{"message": message}]}));
            assert_eq!(error.kind, ApiErrorKind::Unsupported, "{message}");
        }
        let error = failed(Operation::Notifications, json!({"errors": [{"message": "Syntax Error: Expected Name"}]}));
        assert_eq!(error.kind, ApiErrorKind::BadResponse);
        // An uncoded message is not repeated: it could quote anything.
        assert_eq!(error.detail, "POST /graphql: a GraphQL error without a code");
    }

    #[test]
    fn an_answer_of_another_shape_is_a_bad_response() {
        let cases = [
            (Operation::Projects, b"not json".to_vec()),
            (Operation::Projects, body(json!({"data": null}))),
            (Operation::Projects, body(json!({"data": {"other": []}}))),
            (Operation::Projects, body(json!({"data": {"projects": "a"}}))),
            (Operation::Settings, body(json!({"data": {"settings": null}}))),
            (Operation::AnswerQuestion, body(json!({"data": {"answerQuestion": "yes"}}))),
            (Operation::Projects, body(json!([]))),
        ];
        for (operation, bytes) in cases {
            let error = interpret(operation, 200, &bytes).unwrap_err();
            assert_eq!(error.kind, ApiErrorKind::BadResponse, "{}", String::from_utf8_lossy(&bytes));
        }
    }
}
