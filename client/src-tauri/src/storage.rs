//! The settings file: the server's address and the key, as JSON in the app's data directory.
//!
//! The file holds the key, so only the user may read it (mode 0600). The webview has no access
//! to it; everything here is plain file I/O on a path, so it is tested without the app.

use std::fs::{self, OpenOptions, Permissions};
use std::io::{ErrorKind as IoErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use serde_json::{json, Value};

use crate::server::{ConnectError, Endpoint, ErrorKind};

/// File name inside the app's data directory.
pub const FILE_NAME: &str = "connection.json";
const ENDPOINT_KEY: &str = "endpoint";
const KEY_KEY: &str = "key";
/// Read and write for the user, nothing for anyone else.
const MODE: u32 = 0o600;

/// The errors here name the step and the OS's reason. They never quote the file's contents.
fn storage_error(step: &str, e: impl std::fmt::Display) -> ConnectError {
    ConnectError::new(ErrorKind::Storage, format!("{step}: {e}"))
}

/// What is saved: the endpoint and, normally, the key. `None` when nothing usable is saved
/// (no file, or a file that does not hold an endpoint).
pub fn load(path: &Path) -> Result<Option<(Endpoint, Option<String>)>, ConnectError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == IoErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(storage_error("reading the settings file", e)),
    };
    // A value that does not parse is treated as "nothing saved".
    let Ok(Value::Object(mut saved)) = serde_json::from_str::<Value>(&text) else {
        return Ok(None);
    };
    let endpoint = saved.remove(ENDPOINT_KEY).and_then(|v| serde_json::from_value::<Endpoint>(v).ok());
    let key = saved.get(KEY_KEY).and_then(Value::as_str).map(str::to_owned);
    Ok(endpoint.map(|endpoint| (endpoint, key)))
}

/// Removes the saved address and key (the "start the connection over" action). Nothing saved is
/// not an error.
pub fn forget(path: &Path) -> Result<(), ConnectError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == IoErrorKind::NotFound => Ok(()),
        Err(e) => Err(storage_error("removing the settings file", e)),
    }
}

/// Writes the endpoint and the key, then makes sure the file is readable by the user only.
///
/// The contents go to a sibling file created with mode 0600 and are moved into place, so the
/// key is never in a file with wider permissions, even when an older file had them.
pub fn save(path: &Path, endpoint: &Endpoint, key: &str) -> Result<(), ConnectError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| storage_error("creating the settings directory", e))?;
    }
    let text = json!({ ENDPOINT_KEY: endpoint, KEY_KEY: key }).to_string();
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = Path::new(&partial);

    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(MODE)
            .open(partial)
            .map_err(|e| storage_error("creating the settings file", e))?;
        // `mode` only applies to a new file; a leftover one keeps its old permissions.
        file.set_permissions(Permissions::from_mode(MODE))
            .map_err(|e| storage_error("restricting the settings file to the user", e))?;
        file.write_all(text.as_bytes()).map_err(|e| storage_error("writing the settings file", e))?;
        file.sync_all().map_err(|e| storage_error("writing the settings file", e))?;
        fs::rename(partial, path).map_err(|e| storage_error("replacing the settings file", e))
    })();
    if written.is_err() {
        let _ = fs::remove_file(partial);
    }
    written?;

    // After the write: set the permissions on the file in place and confirm they took effect.
    fs::set_permissions(path, Permissions::from_mode(MODE))
        .map_err(|e| storage_error("restricting the settings file to the user", e))?;
    let mode = fs::metadata(path)
        .map_err(|e| storage_error("checking the settings file's permissions", e))?
        .permissions()
        .mode()
        & 0o777;
    if mode != MODE {
        return Err(storage_error(
            "restricting the settings file to the user",
            format!("the mode is {mode:o}, not {MODE:o}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory for one test, beside the test binary (inside cargo's target directory, not
    /// in a shared temporary area). Removed when dropped.
    pub struct TestDir(PathBuf);

    impl TestDir {
        pub fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let exe = std::env::current_exe().unwrap();
            let name = format!("test-tmp-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
            let dir = exe.parent().unwrap().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::TestDir;
    use super::*;
    use std::net::Ipv4Addr;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn endpoint() -> Endpoint {
        Endpoint { host: Ipv4Addr::new(10, 0, 0, 1), port: Some(4000) }
    }

    #[test]
    fn nothing_is_saved_at_first() {
        let dir = TestDir::new();
        assert_eq!(load(&dir.path().join(FILE_NAME)), Ok(None));
    }

    #[test]
    fn what_is_saved_comes_back() {
        let dir = TestDir::new();
        // The data directory does not exist before the first save.
        let path = dir.path().join("data").join(FILE_NAME);
        save(&path, &endpoint(), "k1").unwrap();
        assert_eq!(load(&path), Ok(Some((endpoint(), Some("k1".to_owned())))));

        // A later save replaces both values; the default port is saved as "no port".
        let other = Endpoint { host: Ipv4Addr::new(10, 0, 0, 2), port: None };
        save(&path, &other, "k2").unwrap();
        assert_eq!(load(&path), Ok(Some((other, Some("k2".to_owned())))));
        // Nothing is left beside the file.
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn the_file_is_readable_by_the_user_only() {
        let dir = TestDir::new();
        let path = dir.path().join(FILE_NAME);
        save(&path, &endpoint(), "k1").unwrap();
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn wider_permissions_of_an_older_file_are_narrowed() {
        let dir = TestDir::new();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
        // A leftover of an interrupted save, also too wide.
        let partial = dir.path().join(format!("{FILE_NAME}.partial"));
        fs::write(&partial, "x").unwrap();
        fs::set_permissions(&partial, Permissions::from_mode(0o644)).unwrap();

        save(&path, &endpoint(), "k1").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert!(!partial.exists());
    }

    #[test]
    fn a_file_that_does_not_parse_counts_as_nothing_saved() {
        let dir = TestDir::new();
        let path = dir.path().join(FILE_NAME);
        for text in ["", "not json", "[]", r#"{"key":"k1"}"#, r#"{"endpoint":{"host":"example"},"key":"k1"}"#] {
            fs::write(&path, text).unwrap();
            assert_eq!(load(&path), Ok(None), "{text}");
        }
        // An endpoint without a key is kept; the key is acquired again on restore.
        fs::write(&path, r#"{"endpoint":{"host":"10.0.0.1","port":4000}}"#).unwrap();
        assert_eq!(load(&path), Ok(Some((endpoint(), None))));
    }

    #[test]
    fn a_failed_save_leaves_the_saved_file_as_it_was() {
        let dir = TestDir::new();
        let data = dir.path().join("data");
        let path = data.join(FILE_NAME);
        save(&path, &endpoint(), "k1").unwrap();
        let before = fs::read(&path).unwrap();

        // The directory can no longer be written to, so the new contents cannot be put beside
        // the file.
        fs::set_permissions(&data, Permissions::from_mode(0o500)).unwrap();
        let other = Endpoint { host: Ipv4Addr::new(10, 0, 0, 2), port: None };
        let error = save(&path, &other, "k2").unwrap_err();
        fs::set_permissions(&data, Permissions::from_mode(0o700)).unwrap();
        assert_eq!(error.kind, ErrorKind::Storage);
        assert!(error.detail.starts_with("creating the settings file: "), "{}", error.detail);

        // The earlier address and key are still there, byte for byte, and nothing is left beside them.
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(load(&path), Ok(Some((endpoint(), Some("k1".to_owned())))));
        assert_eq!(mode(&path), 0o600);
        assert_eq!(fs::read_dir(&data).unwrap().count(), 1);

        // The new contents were written but cannot be moved into place (the place is taken by a
        // directory that is not empty): what is there stays, and the partial file is removed.
        let taken = dir.path().join("taken");
        fs::create_dir_all(taken.join(FILE_NAME).join("kept")).unwrap();
        let error = save(&taken.join(FILE_NAME), &other, "k2").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Storage);
        assert!(error.detail.starts_with("replacing the settings file: "), "{}", error.detail);
        assert!(taken.join(FILE_NAME).join("kept").is_dir());
        assert_eq!(fs::read_dir(&taken).unwrap().count(), 1);
    }

    #[test]
    fn a_failed_save_reports_why_without_the_key() {
        let dir = TestDir::new();
        // The "directory" is a file, so nothing can be written under it.
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "").unwrap();
        let error = save(&blocker.join(FILE_NAME), &endpoint(), "secret-key-value").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Storage);
        assert!(error.detail.starts_with("creating the settings directory: "), "{}", error.detail);
        assert!(!error.detail.contains("secret-key-value"));

        // The directory cannot be written to: the reason is reported and no file appears.
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, Permissions::from_mode(0o500)).unwrap();
        let error = save(&locked.join(FILE_NAME), &endpoint(), "secret-key-value").unwrap_err();
        fs::set_permissions(&locked, Permissions::from_mode(0o700)).unwrap();
        assert_eq!(error.kind, ErrorKind::Storage);
        assert!(error.detail.starts_with("creating the settings file: "), "{}", error.detail);
        assert!(!error.detail.contains("secret-key-value"));
        assert_eq!(fs::read_dir(&locked).unwrap().count(), 0);
    }
}
