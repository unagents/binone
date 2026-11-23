//! HTTP calls to the binone server.
//!
//! The requests leave from here (the Rust side), never from the webview, so the client does
//! not depend on the server's CORS settings and the key never enters the page.
//!
//! Contract with the server:
//! - key acquisition: `POST /key` (no body) -> 200 `{"key":"<string>"}`; 403 when the client's
//!   address is outside the range the server allows.
//! - everything else carries `Authorization: Bearer <key>`; 401 when it is missing or invalid.
//! - GraphQL: `POST /graphql`. The connection check is `query { serverInfo { version } }`.
//!
//! Redirects are not followed: the key is only ever sent to the address the user entered, and a
//! 3xx answer counts as "not the agreed shape".

use std::net::Ipv4Addr;
use std::time::Duration;

use reqwest::header::{HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};

/// Default port of the binone server. The only place the number is written; the screen asks
/// for it through the `default_port` command.
pub const DEFAULT_SERVER_PORT: u16 = 47610;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Whole request, connection included. Short under test so the timeout test does not wait long.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 });
const SERVER_INFO_QUERY: &str = "query { serverInfo { version } }";

/// Where the server is. `port: None` means the default port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub host: Ipv4Addr,
    pub port: Option<u16>,
}

impl Endpoint {
    pub fn port(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_SERVER_PORT)
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("http://{}:{}{}", self.host, self.port(), path)
    }
}

/// Why a connection attempt failed. The screen turns `kind` into a sentence the user can act on;
/// `detail` is a technical note shown beside it. Neither ever contains the key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectError {
    pub kind: ErrorKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The address or port the user typed is not usable.
    InvalidInput,
    /// No answer from the server (wrong address, server not running, network).
    Unreachable,
    /// `POST /key` answered 403: this machine is outside the allowed IP range.
    Forbidden,
    /// The server answered 401: it does not accept the key.
    Unauthorized,
    /// The server answered, but not in the agreed shape (not a binone server, or another version).
    BadResponse,
    /// The settings could not be read or written on this machine.
    Storage,
    /// The request could not be prepared on this machine; nothing was sent.
    Internal,
}

impl ConnectError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into() }
    }
}

/// The time a request may take: `REQUEST_TIMEOUT`, or (under test, on this thread only) what
/// `testing::allow_slow_requests` set.
fn request_timeout() -> Duration {
    #[cfg(test)]
    if let Some(limit) = testing::REQUEST_TIMEOUT_OVERRIDE.with(std::cell::Cell::get) {
        return limit;
    }
    REQUEST_TIMEOUT
}

pub(crate) fn client() -> Result<reqwest::Client, ConnectError> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(request_timeout())
        // The server is addressed by IP on the user's network; a system proxy must not sit between.
        .no_proxy()
        // Never follow a redirect: the key must not travel to an address the user did not enter.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        // Nothing was sent yet, so this is a fault on this machine, not an unreachable server.
        .map_err(|e| ConnectError::new(ErrorKind::Internal, format!("HTTP client: {e}")))
}

/// Classifies a failed `send()`. Only the kind of failure goes into the detail, never the
/// error's own text (it can quote the URL or a header).
fn send_error(what: &str, e: &reqwest::Error) -> ConnectError {
    if e.is_timeout() {
        ConnectError::new(ErrorKind::Unreachable, format!("{what}: timed out"))
    } else if e.is_connect() {
        ConnectError::new(ErrorKind::Unreachable, format!("{what}: could not connect"))
    } else if e.is_builder() {
        // The request could not be put together (e.g. a header value with a forbidden character),
        // so nothing left this machine: this says nothing about whether the server is reachable.
        ConnectError::new(ErrorKind::Internal, format!("{what}: the request could not be built"))
    } else if e.is_body() || e.is_decode() {
        ConnectError::new(ErrorKind::BadResponse, format!("{what}: the answer could not be read"))
    } else {
        ConnectError::new(ErrorKind::Unreachable, format!("{what}: request failed"))
    }
}

/// Answers other than the ones the contract names. A redirect is reported as such.
fn unexpected_status(what: &str, status: u16) -> ConnectError {
    let detail = if (300..400).contains(&status) {
        format!("{what}: HTTP {status} (a redirect; it is not followed)")
    } else {
        format!("{what}: HTTP {status}")
    };
    ConnectError::new(ErrorKind::BadResponse, detail)
}

/// The `Authorization` value for a key, or `None` when the key has a character that cannot be
/// sent in a header (a line break or another control character).
pub(crate) fn bearer(key: &str) -> Option<HeaderValue> {
    let mut value = HeaderValue::from_str(&format!("Bearer {key}")).ok()?;
    value.set_sensitive(true);
    Some(value)
}

#[derive(Deserialize)]
struct KeyBody {
    key: String,
}

/// The key-acquisition connection.
pub async fn acquire_key(endpoint: &Endpoint) -> Result<String, ConnectError> {
    let response = client()?
        .post(endpoint.url("/key"))
        .send()
        .await
        .map_err(|e| send_error("POST /key", &e))?;
    match response.status().as_u16() {
        200 => {}
        403 => return Err(ConnectError::new(ErrorKind::Forbidden, "POST /key: 403")),
        401 => return Err(ConnectError::new(ErrorKind::Unauthorized, "POST /key: 401")),
        other => return Err(unexpected_status("POST /key", other)),
    }
    // The body is not echoed into the error: it may hold the key in an unexpected shape.
    let body: KeyBody = response
        .json()
        .await
        .map_err(|_| ConnectError::new(ErrorKind::BadResponse, "POST /key: the body is not {\"key\": \"...\"}"))?;
    if body.key.is_empty() {
        return Err(ConnectError::new(ErrorKind::BadResponse, "POST /key: the key is empty"));
    }
    if bearer(&body.key).is_none() {
        return Err(ConnectError::new(
            ErrorKind::BadResponse,
            "POST /key: the key has a character that cannot be sent in a header",
        ));
    }
    Ok(body.key)
}

#[derive(Deserialize)]
struct GraphqlBody {
    data: Option<ServerInfoData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfoData {
    server_info: Option<ServerInfo>,
}

#[derive(Deserialize)]
struct ServerInfo {
    version: String,
}

/// The connection check: asks the server for its version with the key.
pub async fn server_version(endpoint: &Endpoint, key: &str) -> Result<String, ConnectError> {
    // Checked before anything is sent; `resume` replaces a saved key that fails this.
    let authorization = bearer(key).ok_or_else(|| {
        ConnectError::new(ErrorKind::Unauthorized, "POST /graphql: the key cannot be sent in a header")
    })?;
    let response = client()?
        .post(endpoint.url("/graphql"))
        .header(AUTHORIZATION, authorization)
        .json(&serde_json::json!({ "query": SERVER_INFO_QUERY }))
        .send()
        .await
        .map_err(|e| send_error("POST /graphql", &e))?;
    match response.status().as_u16() {
        200 => {}
        401 => return Err(ConnectError::new(ErrorKind::Unauthorized, "POST /graphql: 401")),
        other => return Err(unexpected_status("POST /graphql", other)),
    }
    let body: GraphqlBody = response
        .json()
        .await
        .map_err(|_| ConnectError::new(ErrorKind::BadResponse, "POST /graphql: the body is not a GraphQL response"))?;
    body.data
        .and_then(|data| data.server_info)
        .map(|info| info.version)
        .ok_or_else(|| ConnectError::new(ErrorKind::BadResponse, "POST /graphql: no data.serverInfo.version"))
}

/// First connection to an endpoint: get a key, then check it. Returns the key and the server's version.
pub async fn connect(endpoint: &Endpoint) -> Result<(String, String), ConnectError> {
    let key = acquire_key(endpoint).await?;
    let version = server_version(endpoint, &key).await?;
    Ok((key, version))
}

/// Connection with a saved key. When the server no longer accepts the key (401), or the saved
/// key cannot be sent at all, a new key is acquired once. Returns the key now in use (new or unchanged) and the server's version.
pub async fn resume(endpoint: &Endpoint, key: &str) -> Result<(String, String), ConnectError> {
    match server_version(endpoint, key).await {
        Ok(version) => Ok((key.to_owned(), version)),
        Err(e) if e.kind == ErrorKind::Unauthorized => connect(endpoint).await,
        Err(e) => Err(e),
    }
}

/// Stand-ins for the server, shared by the tests of this crate.
#[cfg(test)]
pub(crate) mod testing {
    use std::cell::Cell;
    use std::time::Duration;

    thread_local! {
        pub(super) static REQUEST_TIMEOUT_OVERRIDE: Cell<Option<Duration>> = const { Cell::new(None) };
    }

    /// For a test whose result must not depend on how fast the machine is: requests made from this
    /// thread get a long time-out instead of the short one used under test (other tests keep theirs).
    pub fn allow_slow_requests() {
        REQUEST_TIMEOUT_OVERRIDE.with(|limit| limit.set(Some(Duration::from_secs(120))));
    }
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// What the stub saw: one `"<METHOD> <path> <authorization or ->"` line per request.
    pub type Seen = Arc<Mutex<Vec<String>>>;

    /// What the stub answers with.
    pub struct Answer {
        status: u16,
        body: String,
        location: Option<String>,
    }

    impl From<(u16, String)> for Answer {
        fn from((status, body): (u16, String)) -> Self {
            Self { status, body, location: None }
        }
    }

    /// What the stub received as bodies: one entry per request, in order.
    pub type Bodies = Arc<Mutex<Vec<String>>>;

    /// Reads one request. Returns its head and body, or `None` when the client went away.
    fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        // Read the head, then as much body as Content-Length announces.
        let head_end = loop {
            let n = stream.read(&mut buffer).ok()?;
            if n == 0 {
                return None;
            }
            request.extend_from_slice(&buffer[..n]);
            if let Some(i) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let head = String::from_utf8_lossy(&request[..head_end]).to_string();
        let length: usize = head
            .lines()
            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
            .unwrap_or(0);
        while request.len() < head_end + length {
            let n = stream.read(&mut buffer).ok()?;
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..n]);
        }
        let body = String::from_utf8_lossy(&request[head_end..]).to_string();
        Some((head, body))
    }

    /// A one-thread HTTP stub. `answer` maps (path, authorization header) to the answer.
    pub fn stub<A: Into<Answer>>(answer: impl Fn(&str, Option<&str>) -> A + Send + 'static) -> (Endpoint, Seen) {
        let (endpoint, seen, _) = stub_with_bodies(move |path, authorization, _| answer(path, authorization));
        (endpoint, seen)
    }

    /// Like `stub`, and `answer` also gets the request's body; the bodies are returned too.
    pub fn stub_with_bodies<A: Into<Answer>>(
        answer: impl Fn(&str, Option<&str>, &str) -> A + Send + 'static,
    ) -> (Endpoint, Seen, Bodies) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Seen = Arc::default();
        let bodies: Bodies = Arc::default();
        let log = seen.clone();
        let body_log = bodies.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let Some((head, body)) = read_request(&mut stream) else { continue };
                let mut first = head.lines().next().unwrap_or("").split(' ');
                let method = first.next().unwrap_or("").to_owned();
                let path = first.next().unwrap_or("").to_owned();
                let authorization = head
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
                    .map(|l| l["authorization:".len()..].trim().to_owned());
                log.lock().unwrap().push(format!("{method} {path} {}", authorization.as_deref().unwrap_or("-")));
                body_log.lock().unwrap().push(body.clone());
                let Answer { status, body, location } = answer(&path, authorization.as_deref(), &body).into();
                let location = location.map(|l| format!("Location: {l}\r\n")).unwrap_or_default();
                let response = format!(
                    "HTTP/1.1 {status} X\r\n{location}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (Endpoint { host: Ipv4Addr::LOCALHOST, port: Some(port) }, seen, bodies)
    }

    /// A server that follows the contract and accepts only `valid_key`.
    pub fn good_server(valid_key: &'static str) -> impl Fn(&str, Option<&str>) -> (u16, String) + Send + 'static {
        move |path, authorization| match path {
            "/key" => (200, format!(r#"{{"key":"{valid_key}"}}"#)),
            "/graphql" if authorization == Some(format!("Bearer {valid_key}").as_str()) => {
                (200, r#"{"data":{"serverInfo":{"version":"9.9.9"}}}"#.to_owned())
            }
            "/graphql" => (401, String::new()),
            _ => (404, String::new()),
        }
    }

    /// A server that answers every request with a redirect to the same path on `target`.
    pub fn redirect_to(status: u16, target: &Endpoint) -> impl Fn(&str, Option<&str>) -> Answer + Send + 'static {
        let target = target.clone();
        move |path, _| Answer { status, body: String::new(), location: Some(target.url(path)) }
    }

    /// A server that accepts connections and never answers. The count is of the connections.
    pub fn silent() -> (Endpoint, Arc<Mutex<usize>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let connections: Arc<Mutex<usize>> = Arc::default();
        let count = connections.clone();
        std::thread::spawn(move || {
            // The streams are kept so the connections stay open without an answer.
            let mut open = Vec::new();
            for stream in listener.incoming().flatten() {
                *count.lock().unwrap() += 1;
                open.push(stream);
            }
        });
        (Endpoint { host: Ipv4Addr::LOCALHOST, port: Some(port) }, connections)
    }

    pub fn run<T>(future: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(future)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{good_server, redirect_to, run, silent, stub};
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn connect_gets_a_key_and_the_version() {
        let (endpoint, seen) = stub(good_server("k1"));
        assert_eq!(run(connect(&endpoint)), Ok(("k1".to_owned(), "9.9.9".to_owned())));
        assert_eq!(*seen.lock().unwrap(), ["POST /key -", "POST /graphql Bearer k1"]);
    }

    #[test]
    fn resume_uses_the_saved_key() {
        let (endpoint, seen) = stub(good_server("k1"));
        assert_eq!(run(resume(&endpoint, "k1")), Ok(("k1".to_owned(), "9.9.9".to_owned())));
        assert_eq!(*seen.lock().unwrap(), ["POST /graphql Bearer k1"]);
    }

    #[test]
    fn resume_gets_a_new_key_after_401() {
        let (endpoint, seen) = stub(good_server("k2"));
        assert_eq!(run(resume(&endpoint, "old")), Ok(("k2".to_owned(), "9.9.9".to_owned())));
        assert_eq!(
            *seen.lock().unwrap(),
            ["POST /graphql Bearer old", "POST /key -", "POST /graphql Bearer k2"]
        );
    }

    #[test]
    fn forbidden_when_outside_the_allowed_range() {
        let (endpoint, _) = stub(|_: &str, _: Option<&str>| (403, String::new()));
        assert_eq!(run(connect(&endpoint)).unwrap_err().kind, ErrorKind::Forbidden);
    }

    #[test]
    fn unauthorized_when_the_new_key_is_rejected_too() {
        let (endpoint, _) = stub(|path: &str, _: Option<&str>| match path {
            "/key" => (200, r#"{"key":"k"}"#.to_owned()),
            _ => (401, String::new()),
        });
        assert_eq!(run(resume(&endpoint, "old")).unwrap_err().kind, ErrorKind::Unauthorized);
    }

    #[test]
    fn bad_response_when_the_shape_differs() {
        let (endpoint, _) = stub(|_: &str, _: Option<&str>| (200, r#"{"token":"secret-value"}"#.to_owned()));
        let error = run(connect(&endpoint)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(!error.detail.contains("secret-value"));

        let (endpoint, _) = stub(|path: &str, _: Option<&str>| match path {
            "/key" => (200, r#"{"key":"k"}"#.to_owned()),
            _ => (200, r#"{"errors":[{"message":"unknown field"}]}"#.to_owned()),
        });
        assert_eq!(run(connect(&endpoint)).unwrap_err().kind, ErrorKind::BadResponse);

        let (endpoint, _) = stub(|_: &str, _: Option<&str>| (500, "oops".to_owned()));
        assert_eq!(run(connect(&endpoint)).unwrap_err().kind, ErrorKind::BadResponse);
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let (target, target_seen) = stub(good_server("k1"));
        for status in [301, 302, 303, 307, 308] {
            // The key acquisition is redirected: no key comes out of it.
            let (endpoint, seen) = stub(redirect_to(status, &target));
            let error = run(connect(&endpoint)).unwrap_err();
            assert_eq!(error.kind, ErrorKind::BadResponse, "{status}");
            assert_eq!(error.detail, format!("POST /key: HTTP {status} (a redirect; it is not followed)"));
            assert_eq!(*seen.lock().unwrap(), ["POST /key -"]);

            // A request with the key is redirected: the key does not go to the other address,
            // and the answer is not taken for a rejected key (no new key is asked for).
            let (endpoint, seen) = stub(redirect_to(status, &target));
            let error = run(resume(&endpoint, "k1")).unwrap_err();
            assert_eq!(error.kind, ErrorKind::BadResponse, "{status}");
            assert_eq!(*seen.lock().unwrap(), ["POST /graphql Bearer k1"]);
        }
        assert!(target_seen.lock().unwrap().is_empty());
    }

    #[test]
    fn unreachable_when_nothing_listens() {
        // Bind and drop to get a port that is closed.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let endpoint = Endpoint { host: Ipv4Addr::LOCALHOST, port: Some(port) };
        let error = run(connect(&endpoint)).unwrap_err();
        assert_eq!(error, ConnectError::new(ErrorKind::Unreachable, "POST /key: could not connect"));
        assert_eq!(run(resume(&endpoint, "k")).unwrap_err().kind, ErrorKind::Unreachable);
    }

    #[test]
    fn unreachable_when_the_server_does_not_answer() {
        let (endpoint, connections) = silent();
        let started = std::time::Instant::now();
        let error = run(connect(&endpoint)).unwrap_err();
        assert_eq!(error, ConnectError::new(ErrorKind::Unreachable, "POST /key: timed out"));
        assert!(started.elapsed() >= REQUEST_TIMEOUT);
        assert!(started.elapsed() < REQUEST_TIMEOUT * 5);
        assert_eq!(*connections.lock().unwrap(), 1);

        let error = run(resume(&endpoint, "k")).unwrap_err();
        assert_eq!(error, ConnectError::new(ErrorKind::Unreachable, "POST /graphql: timed out"));
    }

    #[test]
    fn a_key_that_cannot_be_sent_is_not_reported_as_unreachable() {
        // From the server: refused as an answer of the wrong shape, before it is used or saved.
        let (endpoint, seen) = stub(|_: &str, _: Option<&str>| (200, r#"{"key":"line1\nline2"}"#.to_owned()));
        let error = run(connect(&endpoint)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(!error.detail.contains("line1"));
        assert_eq!(*seen.lock().unwrap(), ["POST /key -"]);

        // Given directly: nothing is sent, even when nothing listens at the address.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let closed = Endpoint { host: Ipv4Addr::LOCALHOST, port: Some(port) };
        for key in ["line1\nline2", "nul\0", "del\x7f"] {
            let error = run(server_version(&closed, key)).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Unauthorized, "{key:?}");
            assert!(!error.detail.contains(key));
        }

        // Saved: replaced by a new key, like a key the server no longer accepts.
        let (endpoint, seen) = stub(good_server("k2"));
        assert_eq!(run(resume(&endpoint, "line1\nline2")), Ok(("k2".to_owned(), "9.9.9".to_owned())));
        assert_eq!(*seen.lock().unwrap(), ["POST /key -", "POST /graphql Bearer k2"]);
    }

    #[test]
    fn a_request_that_cannot_be_built_is_internal() {
        // A URL without a host cannot be made into a request: the error is reqwest's "builder"
        // kind, the one a header or a URL that cannot be sent produces. Nothing listens anywhere
        // here, and nothing is sent.
        let error = run(reqwest::Client::new().post("http://").send()).unwrap_err();
        assert!(error.is_builder(), "{error:?}");
        assert_eq!(
            send_error("POST /key", &error),
            ConnectError::new(ErrorKind::Internal, "POST /key: the request could not be built")
        );
        // The screen tells the kinds apart by this name.
        let text = serde_json::to_string(&send_error("POST /graphql", &error)).unwrap();
        assert_eq!(text, r#"{"kind":"internal","detail":"POST /graphql: the request could not be built"}"#);
    }

    #[test]
    fn the_default_port_is_used_when_none_is_given() {
        let endpoint = Endpoint { host: Ipv4Addr::new(10, 0, 0, 1), port: None };
        assert_eq!(endpoint.url("/key"), format!("http://10.0.0.1:{DEFAULT_SERVER_PORT}/key"));
    }
}
