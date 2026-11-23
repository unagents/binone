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

use std::net::Ipv4Addr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default port of the binone server. The only place the number is written; the screen asks
/// for it through the `default_port` command.
pub const DEFAULT_SERVER_PORT: u16 = 47610;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
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

    fn url(&self, path: &str) -> String {
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
}

impl ConnectError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into() }
    }
}

fn client() -> Result<reqwest::Client, ConnectError> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        // The server is addressed by IP on the user's network; a system proxy must not sit between.
        .no_proxy()
        .build()
        .map_err(|e| ConnectError::new(ErrorKind::Unreachable, format!("HTTP client: {e}")))
}

fn unreachable(what: &str, e: &reqwest::Error) -> ConnectError {
    let why = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "could not connect"
    } else {
        "request failed"
    };
    ConnectError::new(ErrorKind::Unreachable, format!("{what}: {why}"))
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
        .map_err(|e| unreachable("POST /key", &e))?;
    match response.status().as_u16() {
        200 => {}
        403 => return Err(ConnectError::new(ErrorKind::Forbidden, "POST /key: 403")),
        401 => return Err(ConnectError::new(ErrorKind::Unauthorized, "POST /key: 401")),
        other => return Err(ConnectError::new(ErrorKind::BadResponse, format!("POST /key: HTTP {other}"))),
    }
    // The body is not echoed into the error: it may hold the key in an unexpected shape.
    let body: KeyBody = response
        .json()
        .await
        .map_err(|_| ConnectError::new(ErrorKind::BadResponse, "POST /key: the body is not {\"key\": \"...\"}"))?;
    if body.key.is_empty() {
        return Err(ConnectError::new(ErrorKind::BadResponse, "POST /key: the key is empty"));
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
    let response = client()?
        .post(endpoint.url("/graphql"))
        .bearer_auth(key)
        .json(&serde_json::json!({ "query": SERVER_INFO_QUERY }))
        .send()
        .await
        .map_err(|e| unreachable("POST /graphql", &e))?;
    match response.status().as_u16() {
        200 => {}
        401 => return Err(ConnectError::new(ErrorKind::Unauthorized, "POST /graphql: 401")),
        other => return Err(ConnectError::new(ErrorKind::BadResponse, format!("POST /graphql: HTTP {other}"))),
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

/// Connection with a saved key. When the server no longer accepts the key (401), a new key is
/// acquired once. Returns the key now in use (new or unchanged) and the server's version.
pub async fn resume(endpoint: &Endpoint, key: &str) -> Result<(String, String), ConnectError> {
    match server_version(endpoint, key).await {
        Ok(version) => Ok((key.to_owned(), version)),
        Err(e) if e.kind == ErrorKind::Unauthorized => connect(endpoint).await,
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// What the stub saw: one `"<METHOD> <path> <authorization or ->"` line per request.
    type Seen = Arc<Mutex<Vec<String>>>;

    /// A one-thread HTTP stub. `answer` maps (path, authorization header) to (status, body).
    fn stub(answer: impl Fn(&str, Option<&str>) -> (u16, String) + Send + 'static) -> (Endpoint, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Seen = Arc::default();
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                // Read the head, then as much body as Content-Length announces.
                let head_end = loop {
                    let n = stream.read(&mut buffer).unwrap();
                    if n == 0 {
                        break request.len();
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
                    let n = stream.read(&mut buffer).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..n]);
                }
                let mut first = head.lines().next().unwrap_or("").split(' ');
                let method = first.next().unwrap_or("").to_owned();
                let path = first.next().unwrap_or("").to_owned();
                let authorization = head
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
                    .map(|l| l["authorization:".len()..].trim().to_owned());
                log.lock().unwrap().push(format!("{method} {path} {}", authorization.as_deref().unwrap_or("-")));
                let (status, body) = answer(&path, authorization.as_deref());
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (Endpoint { host: Ipv4Addr::LOCALHOST, port: Some(port) }, seen)
    }

    /// A server that follows the contract and accepts only `valid_key`.
    fn good_server(valid_key: &'static str) -> impl Fn(&str, Option<&str>) -> (u16, String) + Send + 'static {
        move |path, authorization| match path {
            "/key" => (200, format!(r#"{{"key":"{valid_key}"}}"#)),
            "/graphql" if authorization == Some(format!("Bearer {valid_key}").as_str()) => {
                (200, r#"{"data":{"serverInfo":{"version":"9.9.9"}}}"#.to_owned())
            }
            "/graphql" => (401, String::new()),
            _ => (404, String::new()),
        }
    }

    fn run<T>(future: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(future)
    }

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
        let (endpoint, _) = stub(|_, _| (403, String::new()));
        assert_eq!(run(connect(&endpoint)).unwrap_err().kind, ErrorKind::Forbidden);
    }

    #[test]
    fn unauthorized_when_the_new_key_is_rejected_too() {
        let (endpoint, _) = stub(|path, _| match path {
            "/key" => (200, r#"{"key":"k"}"#.to_owned()),
            _ => (401, String::new()),
        });
        assert_eq!(run(resume(&endpoint, "old")).unwrap_err().kind, ErrorKind::Unauthorized);
    }

    #[test]
    fn bad_response_when_the_shape_differs() {
        let (endpoint, _) = stub(|_, _| (200, r#"{"token":"secret-value"}"#.to_owned()));
        let error = run(connect(&endpoint)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(!error.detail.contains("secret-value"));

        let (endpoint, _) = stub(|path, _| match path {
            "/key" => (200, r#"{"key":"k"}"#.to_owned()),
            _ => (200, r#"{"errors":[{"message":"unknown field"}]}"#.to_owned()),
        });
        assert_eq!(run(connect(&endpoint)).unwrap_err().kind, ErrorKind::BadResponse);

        let (endpoint, _) = stub(|_, _| (500, "oops".to_owned()));
        assert_eq!(run(connect(&endpoint)).unwrap_err().kind, ErrorKind::BadResponse);
    }

    #[test]
    fn unreachable_when_nothing_listens() {
        // Bind and drop to get a port that is closed.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let endpoint = Endpoint { host: Ipv4Addr::LOCALHOST, port: Some(port) };
        assert_eq!(run(connect(&endpoint)).unwrap_err().kind, ErrorKind::Unreachable);
        assert_eq!(run(resume(&endpoint, "k")).unwrap_err().kind, ErrorKind::Unreachable);
    }

    #[test]
    fn the_default_port_is_used_when_none_is_given() {
        let endpoint = Endpoint { host: Ipv4Addr::new(10, 0, 0, 1), port: None };
        assert_eq!(endpoint.url("/key"), format!("http://10.0.0.1:{DEFAULT_SERVER_PORT}/key"));
    }
}
