//! The binone client's native side: it stores the server's address and the key, and talks to
//! the server. The webview only calls the commands below; it never sees the key.

mod server;

use std::net::Ipv4Addr;

use serde::Serialize;
use serde_json::json;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use server::{ConnectError, Endpoint, ErrorKind, DEFAULT_SERVER_PORT};

/// Settings file in the app's data directory. It holds the endpoint and the key and is not
/// exposed to the webview (the capability grants no store permission).
const STORE_FILE: &str = "connection.json";
const ENDPOINT_KEY: &str = "endpoint";
const KEY_KEY: &str = "key";

/// A working connection, as shown on the screen.
#[derive(Debug, Serialize)]
struct Connected {
    host: String,
    /// `None` when the default port is in use.
    port: Option<u16>,
    version: String,
}

/// Result of trying the saved connection at startup.
#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum Restored {
    /// Nothing is saved yet.
    None,
    Connected(Connected),
    /// A connection is saved but did not work; the screen goes back to the form with the reason.
    Failed { host: String, port: Option<u16>, error: ConnectError },
}

fn storage_error(e: impl std::fmt::Display) -> ConnectError {
    ConnectError::new(ErrorKind::Storage, e.to_string())
}

fn load_saved(app: &AppHandle) -> Result<Option<(Endpoint, Option<String>)>, ConnectError> {
    let store = app.store(STORE_FILE).map_err(storage_error)?;
    // A value that does not parse is treated as "nothing saved".
    let endpoint = store.get(ENDPOINT_KEY).and_then(|v| serde_json::from_value::<Endpoint>(v).ok());
    let key = store.get(KEY_KEY).and_then(|v| v.as_str().map(str::to_owned));
    Ok(endpoint.map(|endpoint| (endpoint, key)))
}

fn save(app: &AppHandle, endpoint: &Endpoint, key: &str) -> Result<(), ConnectError> {
    let store = app.store(STORE_FILE).map_err(storage_error)?;
    store.set(ENDPOINT_KEY, json!(endpoint));
    store.set(KEY_KEY, json!(key));
    store.save().map_err(storage_error)
}

fn connected(endpoint: &Endpoint, version: String) -> Connected {
    Connected { host: endpoint.host.to_string(), port: endpoint.port, version }
}

#[tauri::command]
fn default_port() -> u16 {
    DEFAULT_SERVER_PORT
}

/// The user confirmed an address: get a key, save the address and the key, check the connection.
#[tauri::command]
async fn connect(app: AppHandle, host: String, port: Option<u16>) -> Result<Connected, ConnectError> {
    let host: Ipv4Addr = host
        .trim()
        .parse()
        .map_err(|_| ConnectError::new(ErrorKind::InvalidInput, "the address is not an IPv4 address"))?;
    if port == Some(0) {
        return Err(ConnectError::new(ErrorKind::InvalidInput, "the port must be 1 to 65535"));
    }
    let endpoint = Endpoint { host, port };
    let key = server::acquire_key(&endpoint).await?;
    save(&app, &endpoint, &key)?;
    let version = server::server_version(&endpoint, &key).await?;
    Ok(connected(&endpoint, version))
}

/// Startup: try the saved address and key.
#[tauri::command]
async fn restore(app: AppHandle) -> Restored {
    let (endpoint, key) = match load_saved(&app) {
        Ok(Some(saved)) => saved,
        Ok(None) => return Restored::None,
        Err(error) => return Restored::Failed { host: String::new(), port: None, error },
    };
    let result = match &key {
        Some(key) => server::resume(&endpoint, key).await,
        // An address without a key (should not happen): acquire one as on the first connection.
        None => server::connect(&endpoint).await,
    };
    let result = result.and_then(|(new_key, version)| {
        if key.as_deref() != Some(new_key.as_str()) {
            save(&app, &endpoint, &new_key)?;
        }
        Ok(version)
    });
    match result {
        Ok(version) => Restored::Connected(connected(&endpoint, version)),
        Err(error) => Restored::Failed { host: endpoint.host.to_string(), port: endpoint.port, error },
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![default_port, connect, restore])
        .run(tauri::generate_context!())
        .expect("error while running the binone client");
}
