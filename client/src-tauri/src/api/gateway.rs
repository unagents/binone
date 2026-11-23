//! Sends a checked operation to the server with the saved address and key.
//!
//! The key is read from the settings file here and goes only into the `Authorization` header.
//! When the server answers 401 (or the saved key cannot be sent at all) a new key is acquired
//! once, saved, and the call is made again; this is the same replacement the connection screen
//! does at startup. A change (mutation) is never sent a second time because of an unclear outcome:
//! only a 401 repeats it, and a 401 means the server refused before doing anything.

use std::path::Path;

use reqwest::header::AUTHORIZATION;
use serde_json::Value;
use tauri::async_runtime::Mutex;

use super::error::{redact_error, redact_value, ApiError, ApiErrorKind};
use super::ops::{Operation, Prepared};
use super::response::interpret;
use crate::server::{self, Endpoint};
use crate::storage;

/// State of the API calls: key replacements are taken one at a time, so calls that fail with 401
/// together do not each acquire a key and write the settings file over one another.
#[derive(Default)]
pub struct Gateway {
    replacing_key: Mutex<()>,
}

impl Gateway {
    /// Runs one operation. `name` and `args` come from the webview and are checked first; nothing is
    /// sent when they are not accepted. The key is removed from whatever is returned.
    pub async fn call(&self, settings: &Path, name: &str, args: Option<Value>) -> Result<Value, ApiError> {
        let prepared = Operation::prepare(name, args)?;
        let (endpoint, saved_key) = storage::load(settings)?.ok_or_else(|| {
            ApiError::new(ApiErrorKind::NotConnected, "no server address is saved yet")
        })?;

        let mut keys: Vec<String> = Vec::new();
        let outcome = self.attempt(settings, endpoint, saved_key, &prepared, &mut keys).await;
        // Whatever key was used or issued during the call, it is not in the result.
        match outcome {
            Ok(value) => Ok(keys.iter().fold(value, |value, key| redact_value(value, key))),
            Err(error) => Err(keys.iter().fold(error, |error, key| redact_error(error, key))),
        }
    }

    async fn attempt(
        &self,
        settings: &Path,
        endpoint: Endpoint,
        saved_key: Option<String>,
        prepared: &Prepared,
        keys: &mut Vec<String>,
    ) -> Result<Value, ApiError> {
        let rejected = match &saved_key {
            Some(key) => {
                keys.push(key.clone());
                match send(&endpoint, key, prepared).await {
                    Err(e) if e.kind == ApiErrorKind::Unauthorized => Some(key.clone()),
                    other => return other,
                }
            }
            // An address without a key (should not happen): acquire one as on the first connection.
            None => None,
        };

        let _replacing = self.replacing_key.lock().await;
        // Another call may have replaced the key while this one waited for its turn.
        let (endpoint, current) = match storage::load(settings)? {
            Some(saved) => saved,
            None => (endpoint, None),
        };
        let key = match current {
            Some(current) if Some(&current) != rejected.as_ref() => current,
            _ => {
                let fresh = server::acquire_key(&endpoint).await?;
                keys.push(fresh.clone());
                storage::save(settings, &endpoint, &fresh)?;
                fresh
            }
        };
        keys.push(key.clone());
        send(&endpoint, &key, prepared).await
    }
}

/// One `POST /graphql` with the key.
async fn send(endpoint: &Endpoint, key: &str, prepared: &Prepared) -> Result<Value, ApiError> {
    // Checked before anything is sent: a saved key that cannot go into a header is replaced.
    let authorization = server::bearer(key)
        .ok_or_else(|| ApiError::new(ApiErrorKind::Unauthorized, "POST /graphql: the key cannot be sent in a header"))?;
    let mutation = prepared.operation.is_mutation();
    let response = server::client()?
        .post(endpoint.url("/graphql"))
        .header(AUTHORIZATION, authorization)
        .json(&prepared.body())
        .send()
        .await
        .map_err(|e| send_failure(&e, mutation))?;
    let status = response.status().as_u16();
    let bytes = response.bytes().await.map_err(|e| read_failure(&e, mutation))?;
    interpret(prepared.operation, status, &bytes)
}

/// A failed `send()`. Only the kind of failure is reported, never the error's own text (it can
/// quote the URL or a header).
///
/// `is_connect()` is tested before `is_timeout()`: a timeout while connecting sets both, and
/// nothing was sent then. After the connection exists, a change that got no answer may have been
/// accepted, so it is reported as an unknown outcome.
fn send_failure(e: &reqwest::Error, mutation: bool) -> ApiError {
    if e.is_builder() {
        ApiError::new(ApiErrorKind::Internal, "POST /graphql: the request could not be built")
    } else if e.is_connect() {
        ApiError::new(ApiErrorKind::Unreachable, "POST /graphql: could not connect")
    } else if mutation {
        ApiError::new(
            ApiErrorKind::OutcomeUnknown,
            "POST /graphql: the request was sent and no answer came back; it may have been accepted",
        )
    } else if e.is_timeout() {
        ApiError::new(ApiErrorKind::Unreachable, "POST /graphql: timed out")
    } else {
        ApiError::new(ApiErrorKind::Unreachable, "POST /graphql: request failed")
    }
}

/// The answer's body could not be read to the end.
fn read_failure(_: &reqwest::Error, mutation: bool) -> ApiError {
    if mutation {
        ApiError::new(
            ApiErrorKind::OutcomeUnknown,
            "POST /graphql: the answer could not be read; the change may have been accepted",
        )
    } else {
        ApiError::new(ApiErrorKind::BadResponse, "POST /graphql: the answer could not be read")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::testing::{redirect_to, run, silent, stub, stub_with_bodies};
    use crate::storage::testing::TestDir;
    use serde_json::json;
    use std::net::TcpListener;
    use std::path::PathBuf;

    const KEY: &str = "key-5f0c2d-not-for-the-webview";
    const NEW_KEY: &str = "key-91ab77-issued-later";

    fn settings(dir: &TestDir) -> PathBuf {
        dir.path().join(storage::FILE_NAME)
    }

    fn call(settings: &Path, name: &str, args: Value) -> Result<Value, ApiError> {
        run(Gateway::default().call(settings, name, Some(args)))
    }

    fn data(field: &str, value: Value) -> (u16, String) {
        (200, json!({ "data": { field: value } }).to_string())
    }

    fn gql_errors(code: &str, message: &str) -> (u16, String) {
        (200, json!({ "data": null, "errors": [{ "message": message, "extensions": { "code": code } }] }).to_string())
    }

    #[test]
    fn a_checked_operation_is_sent_with_the_saved_key() {
        let dir = TestDir::new();
        let (endpoint, seen, bodies) = stub_with_bodies(|_, _, _| data("projects", json!(["a", "b"])));
        storage::save(&settings(&dir), &endpoint, KEY).unwrap();

        let value = call(&settings(&dir), "projects", json!({})).unwrap();
        assert_eq!(value, json!(["a", "b"]));
        assert_eq!(*seen.lock().unwrap(), [format!("POST /graphql Bearer {KEY}")]);
        let sent: Value = serde_json::from_str(&bodies.lock().unwrap()[0]).unwrap();
        assert_eq!(sent, json!({ "query": "query { projects }", "variables": {} }));
    }

    #[test]
    fn what_is_not_on_the_list_or_not_accepted_sends_nothing() {
        let dir = TestDir::new();
        let (endpoint, seen) = stub(|_: &str, _: Option<&str>| data("projects", json!([])));
        storage::save(&settings(&dir), &endpoint, KEY).unwrap();

        let attempts = [
            ("introspect", json!({})),
            ("projects { id }", json!({})),
            ("projects", json!({ "url": "http://127.0.0.1:1/key" })),
            ("projects", json!({ "headers": { "Authorization": "x" } })),
            ("request", json!({ "id": "r", "query": "{ x }" })),
            ("request", json!({ "id": 1 })),
            ("saveDraft", json!({ "title": "t", "document": "mutation { x }" })),
        ];
        for (name, args) in attempts {
            let error = call(&settings(&dir), name, args).unwrap_err();
            assert_eq!(error.kind, ApiErrorKind::InvalidInput, "{name}");
        }
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn without_a_saved_connection_nothing_is_sent() {
        let dir = TestDir::new();
        let error = call(&settings(&dir), "projects", json!({})).unwrap_err();
        assert_eq!(error.kind, ApiErrorKind::NotConnected);
    }

    #[test]
    fn a_rejected_key_is_replaced_once_and_the_call_is_made_again() {
        let dir = TestDir::new();
        let (endpoint, seen, bodies) = stub_with_bodies(|path, authorization, _| match path {
            "/key" => (200, format!(r#"{{"key":"{NEW_KEY}"}}"#)),
            _ if authorization == Some(format!("Bearer {NEW_KEY}").as_str()) => data("createProject", json!("p1")),
            _ => (401, String::new()),
        });
        storage::save(&settings(&dir), &endpoint, "old-key").unwrap();

        let value = call(&settings(&dir), "createProject", json!({ "name": "p1" })).unwrap();
        assert_eq!(value, json!("p1"));
        assert_eq!(
            *seen.lock().unwrap(),
            ["POST /graphql Bearer old-key".to_owned(), "POST /key -".to_owned(), format!("POST /graphql Bearer {NEW_KEY}")]
        );
        // The same document and variables went out both times.
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies[0], bodies[2]);
        // The new key was saved.
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((endpoint, Some(NEW_KEY.to_owned())))));
    }

    #[test]
    fn a_saved_key_that_cannot_be_sent_is_replaced_too() {
        let dir = TestDir::new();
        let (endpoint, seen) = stub(|path: &str, _: Option<&str>| match path {
            "/key" => (200, format!(r#"{{"key":"{NEW_KEY}"}}"#)),
            _ => data("projects", json!([])),
        });
        storage::save(&settings(&dir), &endpoint, "broken\nkey").unwrap();
        call(&settings(&dir), "projects", json!({})).unwrap();
        assert_eq!(*seen.lock().unwrap(), ["POST /key -".to_owned(), format!("POST /graphql Bearer {NEW_KEY}")]);
    }

    #[test]
    fn a_new_key_that_is_rejected_too_is_unauthorized_and_no_third_try_is_made() {
        let dir = TestDir::new();
        let (endpoint, seen) = stub(|path: &str, _: Option<&str>| match path {
            "/key" => (200, format!(r#"{{"key":"{NEW_KEY}"}}"#)),
            _ => (401, String::new()),
        });
        storage::save(&settings(&dir), &endpoint, "old-key").unwrap();
        let error = call(&settings(&dir), "projects", json!({})).unwrap_err();
        assert_eq!(error.kind, ApiErrorKind::Unauthorized);
        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn a_refused_new_key_is_forbidden_and_the_saved_key_stays() {
        let dir = TestDir::new();
        let (endpoint, seen) = stub(|path: &str, _: Option<&str>| match path {
            "/key" => (403, String::new()),
            _ => (401, String::new()),
        });
        storage::save(&settings(&dir), &endpoint, "old-key").unwrap();
        let error = call(&settings(&dir), "projects", json!({})).unwrap_err();
        assert_eq!(error.kind, ApiErrorKind::Forbidden);
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((endpoint, Some("old-key".to_owned())))));
    }

    #[test]
    fn a_403_on_the_call_itself_is_forbidden_and_the_key_is_not_replaced() {
        let dir = TestDir::new();
        let (endpoint, seen) = stub(|_: &str, _: Option<&str>| (403, String::new()));
        storage::save(&settings(&dir), &endpoint, KEY).unwrap();
        assert_eq!(call(&settings(&dir), "projects", json!({})).unwrap_err().kind, ApiErrorKind::Forbidden);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_redirect_is_not_followed_and_the_key_goes_nowhere_else() {
        let dir = TestDir::new();
        let (target, target_seen) = stub(|_: &str, _: Option<&str>| data("projects", json!([])));
        for status in [301, 302, 303, 307, 308] {
            let (endpoint, seen) = stub(redirect_to(status, &target));
            storage::save(&settings(&dir), &endpoint, KEY).unwrap();
            let error = call(&settings(&dir), "projects", json!({})).unwrap_err();
            assert_eq!(error.kind, ApiErrorKind::BadResponse, "{status}");
            // Not taken for a rejected key: no new key is asked for.
            assert_eq!(*seen.lock().unwrap(), [format!("POST /graphql Bearer {KEY}")], "{status}");
        }
        assert!(target_seen.lock().unwrap().is_empty());
    }

    #[test]
    fn each_failure_has_its_own_kind() {
        let dir = TestDir::new();
        let kind_of = |answer: (u16, String), name: &str, args: Value| {
            let answer = std::sync::Mutex::new(Some(answer));
            let (endpoint, _) = stub(move |_: &str, _: Option<&str>| {
                answer.lock().unwrap().clone().unwrap_or((500, String::new()))
            });
            storage::save(&settings(&dir), &endpoint, KEY).unwrap();
            call(&settings(&dir), name, args).unwrap_err().kind
        };
        let req = json!({ "id": "r1" });
        assert_eq!(kind_of(gql_errors("INVALID_INPUT", "invalid input: title"), "request", req.clone()), ApiErrorKind::InvalidInput);
        assert_eq!(kind_of(gql_errors("NOT_FOUND", "not found: request r1"), "request", req.clone()), ApiErrorKind::NotFound);
        assert_eq!(kind_of(gql_errors("CONFLICT", "conflict: version"), "request", req.clone()), ApiErrorKind::Conflict);
        assert_eq!(kind_of(gql_errors("EXTERNAL_UNAVAILABLE", "bantou"), "request", req.clone()), ApiErrorKind::ExternalUnavailable);
        assert_eq!(kind_of(gql_errors("EXTERNAL_OUTCOME_UNKNOWN", "openspec"), "request", req.clone()), ApiErrorKind::OutcomeUnknown);
        assert_eq!(kind_of(gql_errors("INTERNAL", "internal error"), "request", req.clone()), ApiErrorKind::ServerError);
        assert_eq!(kind_of((200, r#"{"token":"x"}"#.into()), "request", req.clone()), ApiErrorKind::BadResponse);
        assert_eq!(kind_of((500, "oops".into()), "request", req.clone()), ApiErrorKind::BadResponse);
        assert_eq!(
            kind_of((200, r#"{"errors":[{"message":"Unknown field \"agentMcpConfig\" on type \"Query\""}]}"#.into()), "agentMcpConfig", json!({})),
            ApiErrorKind::Unsupported
        );
    }

    #[test]
    fn nothing_listening_is_unreachable_for_a_query_and_for_a_change() {
        let dir = TestDir::new();
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let endpoint = Endpoint { host: std::net::Ipv4Addr::LOCALHOST, port: Some(port) };
        storage::save(&settings(&dir), &endpoint, KEY).unwrap();
        assert_eq!(call(&settings(&dir), "projects", json!({})).unwrap_err().kind, ApiErrorKind::Unreachable);
        // Nothing was sent, so even a change is plainly "not reached", not "unknown".
        let error = call(&settings(&dir), "createProject", json!({ "name": "p" })).unwrap_err();
        assert_eq!(error.kind, ApiErrorKind::Unreachable);
    }

    #[test]
    fn a_change_that_gets_no_answer_is_an_unknown_outcome_and_is_not_sent_again() {
        let dir = TestDir::new();
        let (endpoint, connections) = silent();
        storage::save(&settings(&dir), &endpoint, KEY).unwrap();

        let error = call(&settings(&dir), "submitRequest", json!({ "id": "r1", "version": 1 })).unwrap_err();
        assert_eq!(error.kind, ApiErrorKind::OutcomeUnknown);
        assert_eq!(*connections.lock().unwrap(), 1);

        // A query that gets no answer is only unreachable.
        let error = call(&settings(&dir), "requests", json!({})).unwrap_err();
        assert_eq!(error.kind, ApiErrorKind::Unreachable);
        assert_eq!(*connections.lock().unwrap(), 2);
    }

    #[test]
    fn the_key_is_in_no_value_and_no_error() {
        let dir = TestDir::new();
        type Answer = Box<dyn Fn(&str, Option<&str>) -> (u16, String) + Send>;
        let answers: Vec<(&str, Value, Answer)> = vec![
            // A result that echoes the header it was sent.
            ("request", json!({ "id": "r" }), Box::new(|_, auth| data("request", json!({ "id": auth.unwrap_or("") })))),
            ("projects", json!({}), Box::new(|_, auth| data("projects", json!([auth.unwrap_or("")])))),
            // An error that quotes it, in each place the server could.
            ("request", json!({ "id": "r" }), Box::new(|_, _| gql_errors("CONFLICT", &format!("conflict: {KEY}")))),
            ("request", json!({ "id": "r" }), Box::new(|_, _| (200, format!(r#"{{"errors":[{{"message":"{KEY}"}}]}}"#)))),
            ("request", json!({ "id": "r" }), Box::new(|_, _| (500, KEY.to_owned()))),
            ("request", json!({ "id": "r" }), Box::new(|_, auth| (401, auth.unwrap_or("").to_owned()))),
            ("request", json!({ "id": "r" }), Box::new(|_, _| (200, format!(r#"{{"data":{{"request":"{KEY}"}}}}"#)))),
            ("createProject", json!({ "name": "p" }), Box::new(|_, _| gql_errors("INVALID_INPUT", KEY))),
        ];
        for (i, (name, args, answer)) in answers.into_iter().enumerate() {
            let (endpoint, _) = stub(answer);
            storage::save(&settings(&dir), &endpoint, KEY).unwrap();
            let result = call(&settings(&dir), name, args);
            let shown = match &result {
                Ok(value) => format!("{value} {value:?}"),
                Err(error) => format!("{} {error:?}", serde_json::to_string(error).unwrap()),
            };
            assert!(!shown.contains(KEY), "case {i}: {shown}");
        }
    }

    #[test]
    fn a_new_key_is_not_in_what_comes_back_either() {
        let dir = TestDir::new();
        let (endpoint, _) = stub(|path: &str, auth: Option<&str>| match path {
            "/key" => (200, format!(r#"{{"key":"{NEW_KEY}"}}"#)),
            _ if auth == Some(format!("Bearer {NEW_KEY}").as_str()) => data("projects", json!([auth.unwrap_or("")])),
            _ => (401, String::new()),
        });
        storage::save(&settings(&dir), &endpoint, "old-key").unwrap();
        let value = call(&settings(&dir), "projects", json!({})).unwrap();
        assert!(!value.to_string().contains(NEW_KEY), "{value}");
    }

    #[test]
    fn parallel_calls_that_fail_with_401_share_one_new_key() {
        let dir = TestDir::new();
        let (endpoint, seen) = stub(|path: &str, auth: Option<&str>| match path {
            "/key" => (200, format!(r#"{{"key":"{NEW_KEY}"}}"#)),
            _ if auth == Some(format!("Bearer {NEW_KEY}").as_str()) => data("projects", json!([])),
            _ => (401, String::new()),
        });
        storage::save(&settings(&dir), &endpoint, "old-key").unwrap();
        let gateway = Gateway::default();
        let path = settings(&dir);
        let results = run(async {
            let a = gateway.call(&path, "projects", None);
            let b = gateway.call(&path, "projects", None);
            let c = gateway.call(&path, "projects", None);
            futures_join3(a, b, c).await
        });
        assert!(results.0.is_ok() && results.1.is_ok() && results.2.is_ok(), "{results:?}");
        let keys = seen.lock().unwrap().iter().filter(|line| line.starts_with("POST /key")).count();
        assert_eq!(keys, 1, "{:?}", seen.lock().unwrap());
    }

    /// Runs three futures to completion together (no extra crate needed for this).
    async fn futures_join3<A, B, C>(
        a: impl std::future::Future<Output = A>,
        b: impl std::future::Future<Output = B>,
        c: impl std::future::Future<Output = C>,
    ) -> (A, B, C) {
        let mut a = Box::pin(a);
        let mut b = Box::pin(b);
        let mut c = Box::pin(c);
        let (mut ra, mut rb, mut rc) = (None, None, None);
        std::future::poll_fn(|cx| {
            use std::task::Poll;
            if ra.is_none() {
                if let Poll::Ready(v) = a.as_mut().poll(cx) {
                    ra = Some(v);
                }
            }
            if rb.is_none() {
                if let Poll::Ready(v) = b.as_mut().poll(cx) {
                    rb = Some(v);
                }
            }
            if rc.is_none() {
                if let Poll::Ready(v) = c.as_mut().poll(cx) {
                    rc = Some(v);
                }
            }
            if ra.is_some() && rb.is_some() && rc.is_some() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        (ra.unwrap(), rb.unwrap(), rc.unwrap())
    }
}
