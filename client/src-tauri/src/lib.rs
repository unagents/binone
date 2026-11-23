//! The binone client's native side: it stores the server's address and the key, and talks to
//! the server. The webview only calls the commands below; it never sees the key.

mod api;
mod server;
mod storage;
mod update;

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use server::{ConnectError, Endpoint, ErrorKind, DEFAULT_SERVER_PORT};

/// A working connection, as shown on the screen.
#[derive(Debug, PartialEq, Eq, Serialize)]
struct Connected {
    host: String,
    /// `None` when the default port is in use.
    port: Option<u16>,
    version: String,
}

/// Result of trying the saved connection at startup.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum Restored {
    /// Nothing is saved yet.
    None,
    Connected(Connected),
    /// A connection is saved but did not work; the screen goes back to the form with the reason.
    Failed { host: String, port: Option<u16>, error: ConnectError },
}

/// The settings file in the app's data directory. It holds the endpoint and the key and is not
/// exposed to the webview (the capability grants no file permission).
fn settings_path(app: &AppHandle) -> Result<PathBuf, ConnectError> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join(storage::FILE_NAME))
        .map_err(|e| ConnectError::new(ErrorKind::Storage, format!("locating the settings directory: {e}")))
}

fn connected(endpoint: &Endpoint, version: String) -> Connected {
    Connected { host: endpoint.host.to_string(), port: endpoint.port, version }
}

/// The address as typed on the screen.
fn parse_endpoint(host: &str, port: Option<u16>) -> Result<Endpoint, ConnectError> {
    let host: Ipv4Addr = host
        .trim()
        .parse()
        .map_err(|_| ConnectError::new(ErrorKind::InvalidInput, "the address is not an IPv4 address"))?;
    if port == Some(0) {
        return Err(ConnectError::new(ErrorKind::InvalidInput, "the port must be 1 to 65535"));
    }
    Ok(Endpoint { host, port })
}

/// The key-acquisition connection: the first connection, or "get the key again". Gets a key from
/// the address, saves the address and the key (replacing any key held), then checks the connection.
/// The address and the key are saved as soon as the key is acquired, before the check.
async fn connect_and_save(settings: &Path, host: &str, port: Option<u16>) -> Result<Connected, ConnectError> {
    let endpoint = parse_endpoint(host, port)?;
    let key = server::acquire_key(&endpoint).await?;
    storage::save(settings, &endpoint, &key)?;
    let version = server::server_version(&endpoint, &key).await?;
    Ok(connected(&endpoint, version))
}

/// The usual connection: another address, the same key. Nothing is saved unless the server there
/// accepts the key, so a failed change leaves the saved connection as it was. A 401 is reported
/// as it is; no key is acquired here.
async fn change_address_and_save(settings: &Path, host: &str, port: Option<u16>) -> Result<Connected, ConnectError> {
    let endpoint = parse_endpoint(host, port)?;
    let Some((_, Some(key))) = storage::load(settings)? else {
        return Err(ConnectError::new(ErrorKind::Unauthorized, "no key is saved"));
    };
    let version = server::server_version(&endpoint, &key).await?;
    storage::save(settings, &endpoint, &key)?;
    Ok(connected(&endpoint, version))
}

/// Startup: try the saved address with the saved key. A key is never acquired here: without one
/// the screen asks for the first connection, and a rejected key is shown as such.
async fn restore_saved(settings: &Path) -> Restored {
    let (endpoint, key) = match storage::load(settings) {
        // An address without a key counts as nothing saved.
        Ok(Some((endpoint, Some(key)))) => (endpoint, key),
        Ok(_) => return Restored::None,
        Err(error) => return Restored::Failed { host: String::new(), port: None, error },
    };
    match server::server_version(&endpoint, &key).await {
        Ok(version) => Restored::Connected(connected(&endpoint, version)),
        Err(error) => Restored::Failed { host: endpoint.host.to_string(), port: endpoint.port, error },
    }
}

#[tauri::command]
fn default_port() -> u16 {
    DEFAULT_SERVER_PORT
}

#[tauri::command]
async fn connect(app: AppHandle, host: String, port: Option<u16>) -> Result<Connected, ConnectError> {
    connect_and_save(&settings_path(&app)?, &host, port).await
}

#[tauri::command]
async fn change_address(app: AppHandle, host: String, port: Option<u16>) -> Result<Connected, ConnectError> {
    change_address_and_save(&settings_path(&app)?, &host, port).await
}

/// Drops the saved address and key: the next start (and this one, once the screen is told) is a first connection.
#[tauri::command]
fn forget_connection(app: AppHandle) -> Result<(), ConnectError> {
    storage::forget(&settings_path(&app)?)
}

#[tauri::command]
async fn restore(app: AppHandle) -> Restored {
    match settings_path(&app) {
        Ok(settings) => restore_saved(&settings).await,
        Err(error) => Restored::Failed { host: String::new(), port: None, error },
    }
}

/// The webview's only way to the server's GraphQL: an operation name from the allowlist in
/// `api::ops` and its arguments. The address, the key and the document stay on this side.
#[tauri::command]
async fn api_call(
    app: AppHandle,
    gateway: State<'_, api::Gateway>,
    op: String,
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, api::ApiError> {
    let settings = settings_path(&app)?;
    gateway.call(&settings, &op, args).await
}

/// The `.app` bundle this program runs from, if it does.
fn app_bundle() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok().and_then(|exe| update::app_bundle(&exe))
}

/// How an update would be done from here: Homebrew, or by hand. Runs `brew list` once per brew found.
fn detect_plan() -> update::Plan {
    update::plan(app_bundle().as_deref(), &|path| path.is_file(), &update::SystemRunner, update::LIST_TIME_LIMIT)
}

/// The version check: the running version against the latest published release, and (only when
/// there is a newer one) how it could be installed. It never fails as a command; a failure is an
/// `unknown` answer with its reason.
#[tauri::command]
async fn check_update(app: AppHandle) -> update::UpdateCheck {
    let method = || async {
        // Runs a program and waits for it: off the async threads.
        let found = tauri::async_runtime::spawn_blocking(|| detect_plan().method()).await;
        found.unwrap_or(update::UpdateMethod::Manual { reason: update::ManualReason::NotInstalledByCask })
    };
    update::check(&app.package_info().version.to_string(), update::fetch_latest, method).await
}

/// The update through Homebrew, or `manual` when it cannot be run from here (no Homebrew, or
/// this app was not installed by the cask). `latest` is the version the check found: the update
/// counts as done only when that version is the one installed afterwards. One at a time, and
/// within a time limit.
#[tauri::command]
async fn run_update(gate: State<'_, update::UpdateGate>, latest: String) -> Result<update::UpdateOutcome, String> {
    update::run_exclusive(&gate, move || {
        update::detect_and_update(app_bundle().as_deref(), &latest, &|path| path.is_file(), &update::SystemRunner, update::UPDATE_TIME_LIMIT)
    })
    .await
}

/// Starts the app again (after an update the new version starts).
#[tauri::command]
fn restart_app(app: AppHandle) {
    app.restart()
}

/// Opens the release page in the default browser. The address is fixed here; the webview
/// cannot choose it.
#[tauri::command]
fn open_release_page() -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(opener)
        .arg(update::RELEASES_PAGE_URL)
        .spawn()
        .map(drop)
        .map_err(|e| format!("ブラウザを開けませんでした（{}）", e.kind()))
}

pub fn run() {
    tauri::Builder::default()
        .manage(api::Gateway)
        .manage(update::UpdateGate::default())
        .invoke_handler(tauri::generate_handler![
            default_port,
            connect,
            change_address,
            forget_connection,
            restore,
            api_call,
            check_update,
            run_update,
            restart_app,
            open_release_page
        ])
        .run(tauri::generate_context!())
        .expect("error while running the binone client");
}

#[cfg(test)]
mod tests {
    use super::*;
    use server::testing::{good_server, redirect_to, run, silent, stub};
    use std::os::unix::fs::PermissionsExt;
    use storage::testing::TestDir;

    /// A key no other text in a response or an error could contain by accident.
    const KEY: &str = "key-5f0c2d-not-for-the-webview";

    fn json(value: &impl Serialize) -> String {
        serde_json::to_string(value).unwrap()
    }

    fn settings(dir: &TestDir) -> PathBuf {
        dir.path().join(storage::FILE_NAME)
    }

    fn port_of(endpoint: &Endpoint) -> Option<u16> {
        endpoint.port
    }

    #[test]
    fn connect_saves_the_address_and_the_key() {
        let dir = TestDir::new();
        let (endpoint, _) = stub(good_server(KEY));
        let result = run(connect_and_save(&settings(&dir), " 127.0.0.1 ", port_of(&endpoint))).unwrap();
        assert_eq!(result, connected(&endpoint, "9.9.9".to_owned()));
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((endpoint, Some(KEY.to_owned())))));
        let mode = std::fs::metadata(settings(&dir)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn restore_uses_what_connect_saved() {
        let dir = TestDir::new();
        let (endpoint, seen) = stub(good_server(KEY));
        run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&endpoint))).unwrap();
        seen.lock().unwrap().clear();

        let restored = run(restore_saved(&settings(&dir)));
        assert_eq!(restored, Restored::Connected(connected(&endpoint, "9.9.9".to_owned())));
        // The saved key was used as it is: no new key was asked for.
        assert_eq!(*seen.lock().unwrap(), [format!("POST /graphql Bearer {KEY}")]);
    }

    #[test]
    fn restore_without_a_saved_connection_is_none() {
        let dir = TestDir::new();
        assert_eq!(run(restore_saved(&settings(&dir))), Restored::None);
        assert_eq!(json(&Restored::None), r#"{"state":"none"}"#);
    }

    #[test]
    fn a_rejected_saved_key_is_reported_and_no_key_is_asked_for() {
        let dir = TestDir::new();
        let (endpoint, seen) = stub(good_server(KEY));
        storage::save(&settings(&dir), &endpoint, "old-key").unwrap();
        let restored = run(restore_saved(&settings(&dir)));
        let Restored::Failed { error, .. } = &restored else { panic!("{restored:?}") };
        assert_eq!(error.kind, ErrorKind::Unauthorized);
        assert_eq!(*seen.lock().unwrap(), ["POST /graphql Bearer old-key"]);
        // The key is still the one the user holds.
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((endpoint, Some("old-key".to_owned())))));
    }

    #[test]
    fn a_saved_key_that_cannot_be_sent_is_reported_and_nothing_is_sent() {
        let dir = TestDir::new();
        let (endpoint, seen) = stub(good_server(KEY));
        storage::save(&settings(&dir), &endpoint, "broken\nkey").unwrap();
        let restored = run(restore_saved(&settings(&dir)));
        let Restored::Failed { error, .. } = &restored else { panic!("{restored:?}") };
        assert_eq!(error.kind, ErrorKind::Unauthorized);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn an_address_without_a_key_is_the_first_connection() {
        let dir = TestDir::new();
        std::fs::write(settings(&dir), r#"{"endpoint":{"host":"127.0.0.1","port":1}}"#).unwrap();
        assert_eq!(run(restore_saved(&settings(&dir))), Restored::None);
    }

    #[test]
    fn changing_the_address_keeps_the_key() {
        let dir = TestDir::new();
        let (first, _) = stub(good_server(KEY));
        run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&first))).unwrap();

        let (second, seen) = stub(good_server(KEY));
        let result = run(change_address_and_save(&settings(&dir), "127.0.0.1", port_of(&second))).unwrap();
        assert_eq!(result, connected(&second, "9.9.9".to_owned()));
        // The same key went to the new address; no key was asked for there.
        assert_eq!(*seen.lock().unwrap(), [format!("POST /graphql Bearer {KEY}")]);
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((second, Some(KEY.to_owned())))));
    }

    #[test]
    fn a_change_the_server_refuses_is_not_saved_and_asks_for_no_key() {
        let dir = TestDir::new();
        let (first, _) = stub(good_server(KEY));
        run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&first))).unwrap();

        // Another server that does not know this key (it would hand out a key at `/key`).
        let (other, seen) = stub(good_server("another-key"));
        let error = run(change_address_and_save(&settings(&dir), "127.0.0.1", port_of(&other))).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unauthorized);
        assert_eq!(*seen.lock().unwrap(), [format!("POST /graphql Bearer {KEY}")]);
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((first, Some(KEY.to_owned())))));
    }

    #[test]
    fn getting_the_key_again_replaces_it() {
        let dir = TestDir::new();
        let (first, _) = stub(good_server("old-key"));
        run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&first))).unwrap();
        let (other, _) = stub(good_server(KEY));
        run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&other))).unwrap();
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((other, Some(KEY.to_owned())))));
    }

    #[test]
    fn a_key_refused_by_range_is_forbidden_and_the_held_key_stays() {
        let dir = TestDir::new();
        let (first, _) = stub(good_server(KEY));
        run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&first))).unwrap();
        let (outside, _) = stub(|_: &str, _: Option<&str>| (403, String::new()));
        let error = run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&outside))).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Forbidden);
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((first, Some(KEY.to_owned())))));
    }

    #[test]
    fn forgetting_the_connection_goes_back_to_the_first_connection() {
        let dir = TestDir::new();
        let (endpoint, _) = stub(good_server(KEY));
        run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&endpoint))).unwrap();
        assert_eq!(storage::forget(&settings(&dir)), Ok(()));
        assert_eq!(run(restore_saved(&settings(&dir))), Restored::None);
        // Nothing saved is not an error either.
        assert_eq!(storage::forget(&settings(&dir)), Ok(()));
    }

    #[test]
    fn what_the_webview_gets_never_holds_the_key() {
        let dir = TestDir::new();
        let (endpoint, _) = stub(good_server(KEY));

        let result = run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&endpoint))).unwrap();
        let text = json(&result);
        assert_eq!(text, format!(r#"{{"host":"127.0.0.1","port":{},"version":"9.9.9"}}"#, endpoint.port()));
        assert!(!text.contains(KEY));

        let text = json(&run(restore_saved(&settings(&dir))));
        assert!(text.starts_with(r#"{"state":"connected","host":"127.0.0.1","#), "{text}");
        assert!(!text.contains(KEY));

        // A failed restore: the saved key is rejected and so is the new one.
        let (rejecting, _) = stub(|path: &str, _: Option<&str>| match path {
            "/key" => (200, format!(r#"{{"key":"{KEY}"}}"#)),
            _ => (401, String::new()),
        });
        storage::save(&settings(&dir), &rejecting, KEY).unwrap();
        let text = json(&run(restore_saved(&settings(&dir))));
        assert!(text.starts_with(r#"{"state":"failed","host":"127.0.0.1","#), "{text}");
        assert!(text.contains(r#""kind":"unauthorized""#), "{text}");
        assert!(!text.contains(KEY));
    }

    #[test]
    fn a_redirect_is_an_error_and_nothing_is_saved() {
        let dir = TestDir::new();
        // The place the redirect points to would hand out a key.
        let (target, target_seen) = stub(good_server(KEY));
        for status in [301, 302, 303, 307, 308] {
            let (endpoint, seen) = stub(redirect_to(status, &target));
            let error = run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&endpoint))).unwrap_err();
            assert_eq!(error.kind, ErrorKind::BadResponse, "{status}");
            assert!(error.detail.contains(&format!("HTTP {status}")), "{}", error.detail);
            assert_eq!(*seen.lock().unwrap(), ["POST /key -"], "{status}");
        }
        assert!(target_seen.lock().unwrap().is_empty());
        assert!(!settings(&dir).exists());
    }

    #[test]
    fn a_saved_key_is_not_sent_along_a_redirect() {
        let dir = TestDir::new();
        let (target, target_seen) = stub(good_server(KEY));
        let (endpoint, seen) = stub(redirect_to(307, &target));
        storage::save(&settings(&dir), &endpoint, KEY).unwrap();

        let restored = run(restore_saved(&settings(&dir)));
        let Restored::Failed { error, .. } = &restored else { panic!("{restored:?}") };
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(!json(&restored).contains(KEY));
        assert_eq!(*seen.lock().unwrap(), [format!("POST /graphql Bearer {KEY}")]);
        assert!(target_seen.lock().unwrap().is_empty());
        // The saved connection is left as it was.
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((endpoint, Some(KEY.to_owned())))));
    }

    #[test]
    fn a_server_that_does_not_answer_times_out() {
        let dir = TestDir::new();
        let (endpoint, connections) = silent();
        let error = run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&endpoint))).unwrap_err();
        assert_eq!(error, ConnectError::new(ErrorKind::Unreachable, "POST /key: timed out"));
        assert_eq!(*connections.lock().unwrap(), 1);
        assert!(!settings(&dir).exists());

        storage::save(&settings(&dir), &endpoint, KEY).unwrap();
        let restored = run(restore_saved(&settings(&dir)));
        let Restored::Failed { error, .. } = &restored else { panic!("{restored:?}") };
        assert_eq!(*error, ConnectError::new(ErrorKind::Unreachable, "POST /graphql: timed out"));
        assert!(!json(&restored).contains(KEY));
    }

    #[test]
    fn the_key_is_saved_before_the_connection_check() {
        // The key is issued but the check then fails: the address and the key are already saved,
        // so the next start tries them again.
        let dir = TestDir::new();
        let (endpoint, _) = stub(|path: &str, _: Option<&str>| match path {
            "/key" => (200, format!(r#"{{"key":"{KEY}"}}"#)),
            _ => (500, String::new()),
        });
        let error = run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&endpoint))).unwrap_err();
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(!json(&error).contains(KEY));
        assert_eq!(storage::load(&settings(&dir)), Ok(Some((endpoint, Some(KEY.to_owned())))));
    }

    #[test]
    fn errors_never_quote_the_key() {
        let dir = TestDir::new();
        type Answer = Box<dyn Fn(&str, Option<&str>) -> (u16, String) + Send>;
        let answers: Vec<Answer> = vec![
            // The key in a body of the wrong shape, at each step.
            Box::new(|_, _| (200, format!(r#"{{"token":"{KEY}"}}"#))),
            Box::new(|_, _| (200, format!(r#"{{"key":["{KEY}"]}}"#))),
            Box::new(|_, _| (403, format!(r#"{{"key":"{KEY}"}}"#))),
            Box::new(|_, _| (500, KEY.to_owned())),
            Box::new(|path, _| match path {
                "/key" => (200, format!(r#"{{"key":"{KEY}"}}"#)),
                _ => (200, format!(r#"{{"errors":[{{"message":"{KEY}"}}]}}"#)),
            }),
            Box::new(|path, authorization| match path {
                "/key" => (200, format!(r#"{{"key":"{KEY}"}}"#)),
                // Echoes the header it was sent.
                _ => (401, authorization.unwrap_or("").to_owned()),
            }),
            // A key that cannot be sent in a header.
            Box::new(|_, _| (200, format!(r#"{{"key":"{KEY}\n{KEY}"}}"#))),
        ];
        for (i, answer) in answers.into_iter().enumerate() {
            let (endpoint, _) = stub(answer);
            let error = run(connect_and_save(&settings(&dir), "127.0.0.1", port_of(&endpoint))).unwrap_err();
            assert!(!json(&error).contains(KEY), "case {i}: {error:?}");
            assert!(!format!("{error:?}").contains(KEY), "case {i}");

            storage::save(&settings(&dir), &endpoint, KEY).unwrap();
            let restored = run(restore_saved(&settings(&dir)));
            assert!(matches!(restored, Restored::Failed { .. }), "case {i}: {restored:?}");
            assert!(!json(&restored).contains(KEY), "case {i}: {restored:?}");
            assert!(!format!("{restored:?}").contains(KEY), "case {i}");
        }
    }

    #[test]
    fn input_that_is_not_an_address_is_refused_before_anything_is_sent() {
        let dir = TestDir::new();
        for (host, port) in [("example.local", None), ("", None), ("10.0.0.256", None), ("127.0.0.1", Some(0))] {
            let error = run(connect_and_save(&settings(&dir), host, port)).unwrap_err();
            assert_eq!(error.kind, ErrorKind::InvalidInput, "{host}");
        }
        assert!(!settings(&dir).exists());
    }
}
