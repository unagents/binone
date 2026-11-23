//! Checking for a newer client and updating it through Homebrew.
//!
//! Both happen on this side: the webview never reaches GitHub and never starts a program. The
//! latest release is read from GitHub's public API without credentials; the update runs
//! `brew update` and `brew upgrade --cask binone` with the program and its arguments given
//! separately (no shell). Nothing here touches the app's data directory, where the connection
//! settings and the key are kept.
//!
//! What touches the outside (the HTTP answer, running a program, the file system) is passed in
//! as an argument or a trait, so the decisions can be tested without a network or a real brew.

use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

/// The latest published release of the client (GitHub leaves out drafts and prereleases).
pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/unagents/binone/releases/latest";
/// Where a person can get the release by hand.
pub const RELEASES_PAGE_URL: &str = "https://github.com/unagents/binone/releases/latest";
/// The command to give to a person when the update cannot be run from here.
pub const MANUAL_COMMAND: &str = "brew upgrade --cask binone";

const CASK: &str = "binone";
/// Looked for in this order; `PATH` is not consulted (an app started from Finder has a short one).
const BREW_CANDIDATES: [&str; 2] = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"];
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// The whole update (`brew update` and `brew upgrade`) may take this long.
pub const UPDATE_TIME_LIMIT: Duration = Duration::from_secs(10 * 60);
/// The one brew call made to find out how to update (`brew list --cask binone`).
pub const LIST_TIME_LIMIT: Duration = Duration::from_secs(10);
/// After a time-out: how long the program gets to end on TERM before the whole group is killed.
const TERM_GRACE: Duration = Duration::from_millis(500);
/// After a stop: how long the readers of the output are waited for (a straggler holding a pipe must not hold us).
const DRAIN_LIMIT: Duration = Duration::from_secs(1);
const OUTPUT_TAIL_LINES: usize = 20;
const OUTPUT_TAIL_CHARS: usize = 2000;

// ---------------------------------------------------------------------------------------------
// Versions

/// `major.minor.patch`, compared as numbers (so 0.10.0 is newer than 0.9.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl Version {
    /// `1.2.3`, with one leading `v` allowed (a tag). A prerelease or build part, a missing or
    /// an extra number, or anything that is not a plain number is refused.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut number = || {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse::<u64>().ok()
        };
        let version = Version(number()?, number()?, number()?);
        parts.next().is_none().then_some(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

// ---------------------------------------------------------------------------------------------
// Checking

/// Why the latest version could not be found out. The text is shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckFailure(pub String);

/// What the check found. `Unknown` carries the reason; the screens stay usable in every case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateCheck {
    UpToDate { current: String, latest: String },
    /// `method` is how the update can be done from here, found out only for this state.
    Available { current: String, latest: String, method: UpdateMethod },
    Unknown { current: String, reason: String },
}

/// How a newer version can be installed from this app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateMethod {
    /// Homebrew can do it (the "update" button is offered).
    Brew,
    /// It cannot be run from here: the release page and the command are offered instead.
    Manual { reason: ManualReason },
}

/// The status and the body of an HTTP answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpAnswer {
    pub status: u16,
    pub body: String,
}

/// The version of the release in GitHub's answer for "latest release".
pub fn interpret_release(answer: &HttpAnswer) -> Result<Version, CheckFailure> {
    let fail = |text: String| Err(CheckFailure(text));
    match answer.status {
        200 => {}
        403 | 429 => return fail(format!("GitHub refused the request (rate limit?): HTTP {}", answer.status)),
        404 => return fail("no release has been published".to_owned()),
        status => return fail(format!("GitHub answered HTTP {status}")),
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&answer.body) else {
        return fail("the answer is not JSON".to_owned());
    };
    let flag = |name: &str| value.get(name).and_then(|v| v.as_bool()).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return fail("the latest release is a draft or a prerelease".to_owned());
    }
    let Some(tag) = value.get("tag_name").and_then(|v| v.as_str()) else {
        return fail("the answer has no tag_name".to_owned());
    };
    match Version::parse(tag) {
        Some(version) => Ok(version),
        None => fail(format!("the tag \"{}\" is not a version", tag.chars().take(40).collect::<String>())),
    }
}

/// Compares the running version with the latest one. `fetch` gets the answer of GitHub's API;
/// `method` finds out how an update could be done, and is called only when there is a newer
/// version (so nothing about Homebrew is looked at otherwise).
pub async fn check<F, Fut, M, MFut>(current: &str, fetch: F, method: M) -> UpdateCheck
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<HttpAnswer, CheckFailure>>,
    M: FnOnce() -> MFut,
    MFut: Future<Output = UpdateMethod>,
{
    let unknown = |reason: String| UpdateCheck::Unknown { current: current.to_owned(), reason };
    let Some(running) = Version::parse(current) else {
        return unknown(format!("this app's version \"{current}\" is not a plain version"));
    };
    let latest = match fetch().await.and_then(|answer| interpret_release(&answer)) {
        Ok(latest) => latest,
        Err(CheckFailure(reason)) => return unknown(reason),
    };
    let (current, latest) = (running.to_string(), latest.to_string());
    if Version::parse(&latest) > Some(running) {
        UpdateCheck::Available { current, latest, method: method().await }
    } else {
        UpdateCheck::UpToDate { current, latest }
    }
}

/// Asks GitHub for the latest release. Only `https` is used, and the answer's text is not
/// quoted in a failure (only the kind of failure is).
pub async fn fetch_latest() -> Result<HttpAnswer, CheckFailure> {
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent("binone-client")
        .build()
        .map_err(|_| CheckFailure("the HTTP client could not be prepared".to_owned()))?;
    let failure = |e: &reqwest::Error| {
        CheckFailure(
            if e.is_timeout() {
                "GitHub did not answer in time"
            } else if e.is_connect() {
                "could not connect to GitHub"
            } else {
                "the request to GitHub failed"
            }
            .to_owned(),
        )
    };
    let response = client
        .get(LATEST_RELEASE_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| failure(&e))?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(|e| failure(&e))?;
    Ok(HttpAnswer { status, body })
}

// ---------------------------------------------------------------------------------------------
// Updating

/// What a finished program left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub code: Option<i32>,
    /// Standard output, then standard error (what there was when the program was stopped, too).
    pub text: String,
    /// The program did not finish within its time limit and was killed.
    pub timed_out: bool,
}

/// Runs a program with its arguments (never through a shell). A program that is still running
/// after `limit` is killed and reaped, and the answer says so.
pub trait Runner {
    fn run(&self, program: &Path, args: &[&str], limit: Duration) -> Result<RunOutput, String>;
}

pub struct SystemRunner;

/// Collects what a pipe delivers, so that what came before a kill can still be read.
fn collect<R: Read + Send + 'static>(mut pipe: R) -> (Arc<Mutex<Vec<u8>>>, std::thread::JoinHandle<()>) {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buffer);
    let reader = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(&chunk[..n]);
        }
    });
    (buffer, reader)
}

fn text_of(buffer: &Mutex<Vec<u8>>) -> String {
    String::from_utf8_lossy(&buffer.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
}

/// Waits for the readers to reach the end of their pipes, but not longer than `limit` in all
/// (a program's child may hold a pipe open for good; its reader is then left behind).
fn drain(readers: Vec<std::thread::JoinHandle<()>>, limit: Duration) {
    let deadline = Instant::now() + limit;
    for reader in readers {
        while !reader.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if reader.is_finished() {
            let _ = reader.join();
        }
    }
}

#[cfg(unix)]
mod group {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    pub const SIGTERM: i32 = 15;
    pub const SIGKILL: i32 = 9;

    /// Signals every process of the group led by `leader` (the program was started as the leader of its own group).
    pub fn signal(leader: u32, sig: i32) {
        if let Ok(pid) = i32::try_from(leader) {
            // SAFETY: kill(2) only sends a signal; a negative pid names the process group.
            unsafe { kill(-pid, sig) };
        }
    }
}

/// Stops the program and its children (git, curl, installers...) and reaps it: TERM to the group,
/// a short grace, then KILL to the group. The leader is not reaped until then, so the group id is not reused.
fn stop_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        group::signal(child.id(), group::SIGTERM);
        std::thread::sleep(TERM_GRACE);
        group::signal(child.id(), group::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

impl Runner for SystemRunner {
    fn run(&self, program: &Path, args: &[&str], limit: Duration) -> Result<RunOutput, String> {
        use std::process::Stdio;
        let started = |e: std::io::Error| format!("could not start {}: {}", program.display(), e.kind());
        let mut command = std::process::Command::new(program);
        command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().map_err(started)?;
        let (stdout, out_reader) = collect(child.stdout.take().ok_or("no standard output")?);
        let (stderr, err_reader) = collect(child.stderr.take().ok_or("no standard error")?);
        let deadline = Instant::now() + limit;
        let (code, timed_out) = loop {
            match child.try_wait() {
                Ok(Some(status)) => break (status.code(), false),
                Ok(None) if Instant::now() >= deadline => {
                    stop_group(&mut child);
                    break (None, true);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => {
                    stop_group(&mut child);
                    return Err(format!("could not wait for {}: {}", program.display(), e.kind()));
                }
            }
        };
        // The pipes reach their end once the program (and its children) are gone; wait for the
        // tail of the output, but only a short while.
        drain(vec![out_reader, err_reader], DRAIN_LIMIT);
        let mut text = text_of(&stdout);
        text.push_str(&text_of(&stderr));
        Ok(RunOutput { code, text, timed_out })
    }
}

/// The first of the usual places that has a brew.
pub fn find_brew(exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    BREW_CANDIDATES.iter().map(PathBuf::from).find(|path| exists(path))
}

/// The arguments of each brew command of an update, in the order they are run.
pub fn update_steps() -> [Vec<&'static str>; 2] {
    [vec!["update"], vec!["upgrade", "--cask", CASK]]
}

/// Whether the running executable is inside an `.app` bundle (a development build is not).
pub fn is_app_bundle(exe: &Path) -> bool {
    exe.ancestors().any(|dir| {
        dir.extension().is_some_and(|e| e == "app") && exe.strip_prefix(dir).is_ok_and(|rest| rest.starts_with("Contents/MacOS"))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualReason {
    /// No Homebrew in the usual places.
    NoBrew,
    /// Homebrew is there, but this app was not installed by the cask `binone`.
    NotInstalledByCask,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Brew(PathBuf),
    Manual(ManualReason),
}

/// Whether the update can be run with Homebrew: there is a brew, this app is an app bundle, and
/// the cask `binone` is installed.
/// `list_limit` caps the one Homebrew call made (`brew list`).
pub fn plan(bundled: bool, exists: &dyn Fn(&Path) -> bool, runner: &dyn Runner, list_limit: Duration) -> Plan {
    let Some(brew) = find_brew(exists) else {
        return Plan::Manual(ManualReason::NoBrew);
    };
    let installed = bundled
        && matches!(runner.run(&brew, &["list", "--cask", CASK], list_limit), Ok(RunOutput { code: Some(0), timed_out: false, .. }));
    if installed {
        Plan::Brew(brew)
    } else {
        Plan::Manual(ManualReason::NotInstalledByCask)
    }
}

impl Plan {
    /// What the check tells the screen about this plan.
    pub fn method(&self) -> UpdateMethod {
        match self {
            Plan::Brew(_) => UpdateMethod::Brew,
            Plan::Manual(reason) => UpdateMethod::Manual { reason: *reason },
        }
    }
}

/// How an update attempt ended, as shown on the screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateOutcome {
    /// Nothing was run: the person is given the release page and the command.
    Manual { reason: ManualReason, command: &'static str, url: &'static str },
    /// Another update is running.
    Busy,
    /// Run to the end or to the first failing step. `output` is the end of its output.
    /// `timed_out`: the step did not end within the time limit and was killed.
    Finished { ok: bool, code: Option<i32>, step: String, output: String, timed_out: bool },
}

/// The last lines of `text`, limited in size.
pub fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let joined = lines[lines.len().saturating_sub(OUTPUT_TAIL_LINES)..].join("\n");
    let skip = joined.chars().count().saturating_sub(OUTPUT_TAIL_CHARS);
    joined.chars().skip(skip).collect()
}

/// Runs the update as planned. `limit` is for the whole update: each step gets what is left of
/// it, and a step that goes past it is killed.
pub fn run_update(plan: &Plan, runner: &dyn Runner, limit: Duration) -> UpdateOutcome {
    let brew = match plan {
        Plan::Manual(reason) => return UpdateOutcome::Manual { reason: *reason, command: MANUAL_COMMAND, url: RELEASES_PAGE_URL },
        Plan::Brew(brew) => brew,
    };
    let deadline = Instant::now() + limit;
    let mut last = UpdateOutcome::Busy;
    for args in update_steps() {
        let step = format!("brew {}", args.join(" "));
        let left = deadline.saturating_duration_since(Instant::now());
        last = match runner.run(brew, &args, left) {
            Err(text) => return UpdateOutcome::Finished { ok: false, code: None, step, output: text, timed_out: false },
            Ok(RunOutput { code, text, timed_out }) if timed_out || code != Some(0) => {
                return UpdateOutcome::Finished { ok: false, code, step, output: tail(&text), timed_out }
            }
            Ok(RunOutput { code, text, .. }) => UpdateOutcome::Finished { ok: true, code, step, output: tail(&text), timed_out: false },
        };
    }
    last
}

/// What "更新" runs: the judgment of the way (`brew list`, at most `LIST_TIME_LIMIT`) and then the
/// update, all within the one `limit`: the judgment is part of it, not extra.
pub fn detect_and_update(bundled: bool, exists: &dyn Fn(&Path) -> bool, runner: &dyn Runner, limit: Duration) -> UpdateOutcome {
    let deadline = Instant::now() + limit;
    let plan = plan(bundled, exists, runner, LIST_TIME_LIMIT.min(limit));
    run_update(&plan, runner, deadline.saturating_duration_since(Instant::now()))
}

/// Lets one update run at a time.
#[derive(Default)]
pub struct UpdateGate(AtomicBool);

pub struct UpdateRunning<'a>(&'a UpdateGate);

impl UpdateGate {
    pub fn enter(&self) -> Option<UpdateRunning<'_>> {
        (!self.0.swap(true, Ordering::SeqCst)).then_some(UpdateRunning(self))
    }
}

/// Runs `work` (which waits for programs) off the async threads, one at a time. However it ends
/// (a failure, a timeout, a panic), the gate is open again afterwards.
pub async fn run_exclusive<W>(gate: &UpdateGate, work: W) -> Result<UpdateOutcome, String>
where
    W: FnOnce() -> UpdateOutcome + Send + 'static,
{
    let Some(_running) = gate.enter() else {
        return Ok(UpdateOutcome::Busy);
    };
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("the update could not be run: {e}"))
}

impl Drop for UpdateRunning<'_> {
    fn drop(&mut self) {
        self.0 .0.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::testing::run;
    use std::cell::{Cell, RefCell};

    fn answer(status: u16, body: &str) -> HttpAnswer {
        HttpAnswer { status, body: body.to_owned() }
    }

    fn release(tag: &str) -> HttpAnswer {
        answer(200, &format!(r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"name":"x"}}"#))
    }

    fn checked(current: &str, result: Result<HttpAnswer, CheckFailure>) -> UpdateCheck {
        run(check(current, || async { result }, || async { UpdateMethod::Brew }))
    }

    #[test]
    fn versions_are_compared_as_numbers() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("0.10.0") > v("0.9.0"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.2.1") > v("0.2.0"));
        assert_eq!(v("v0.2.0"), v("0.2.0"));
        assert_eq!(v("v12.0.3").to_string(), "12.0.3");
    }

    #[test]
    fn what_is_not_a_plain_version_is_refused() {
        for text in ["", "v", "1.2", "1.2.3.4", "1.2.x", "1.2.3-rc.1", "1.2.3+5", "vv1.2.3", "1..3", "-1.2.3", "1.2.3 ", "latest"] {
            // Blanks around the text are tolerated, so only judge the ones that have none.
            if text.trim() != text {
                continue;
            }
            assert_eq!(Version::parse(text), None, "{text:?}");
        }
        assert_eq!(Version::parse("99999999999999999999.0.0"), None);
    }

    #[test]
    fn a_newer_release_is_available() {
        let check = checked("0.2.0", Ok(release("v0.10.0")));
        assert_eq!(check, UpdateCheck::Available { current: "0.2.0".into(), latest: "0.10.0".into(), method: UpdateMethod::Brew });
    }

    #[test]
    fn the_same_or_an_older_release_is_up_to_date() {
        for tag in ["v0.2.0", "0.2.0", "v0.1.9"] {
            let check = checked("0.2.0", Ok(release(tag)));
            assert!(matches!(check, UpdateCheck::UpToDate { .. }), "{tag}: {check:?}");
        }
    }

    #[test]
    fn a_draft_or_a_prerelease_is_not_a_candidate() {
        for flag in ["draft", "prerelease"] {
            let body = format!(r#"{{"tag_name":"v9.9.9","{flag}":true}}"#);
            let check = checked("0.2.0", Ok(answer(200, &body)));
            assert!(matches!(check, UpdateCheck::Unknown { .. }), "{flag}: {check:?}");
        }
        let check = checked("0.2.0", Ok(release("v9.9.9-rc.1")));
        assert!(matches!(check, UpdateCheck::Unknown { .. }), "{check:?}");
    }

    #[test]
    fn an_answer_of_the_wrong_shape_is_unknown_with_a_reason() {
        let bodies = ["", "not json", "[]", "{}", r#"{"tag_name":1}"#, r#"{"tag_name":"latest"}"#, r#"{"name":"v1.0.0"}"#];
        for body in bodies {
            let check = checked("0.2.0", Ok(answer(200, body)));
            let UpdateCheck::Unknown { current, reason } = check else { panic!("{body:?}") };
            assert_eq!(current, "0.2.0");
            assert!(!reason.is_empty(), "{body:?}");
        }
    }

    #[test]
    fn a_refusal_or_a_failure_is_unknown_with_a_reason() {
        let UpdateCheck::Unknown { reason, .. } = checked("0.2.0", Ok(answer(403, r#"{"message":"API rate limit exceeded"}"#))) else {
            panic!()
        };
        assert!(reason.contains("rate limit") && reason.contains("403"), "{reason}");
        let UpdateCheck::Unknown { reason, .. } = checked("0.2.0", Ok(answer(404, "{}"))) else { panic!() };
        assert!(reason.contains("no release"), "{reason}");
        let UpdateCheck::Unknown { reason, .. } = checked("0.2.0", Ok(answer(500, ""))) else { panic!() };
        assert!(reason.contains("500"), "{reason}");
        let failed = checked("0.2.0", Err(CheckFailure("could not connect to GitHub".into())));
        assert_eq!(failed, UpdateCheck::Unknown { current: "0.2.0".into(), reason: "could not connect to GitHub".into() });
    }

    #[test]
    fn a_running_version_that_is_not_plain_is_unknown() {
        let check = checked("0.2.0-dev", Ok(release("v0.3.0")));
        assert!(matches!(check, UpdateCheck::Unknown { .. }), "{check:?}");
    }

    #[test]
    fn the_check_serializes_with_a_state_and_a_method() {
        let text = serde_json::to_string(&checked("0.2.0", Ok(release("v0.3.0")))).unwrap();
        assert_eq!(text, r#"{"state":"available","current":"0.2.0","latest":"0.3.0","method":{"kind":"brew"}}"#);
        let manual = UpdateCheck::Available {
            current: "0.2.0".into(),
            latest: "0.3.0".into(),
            method: UpdateMethod::Manual { reason: ManualReason::NoBrew },
        };
        let text = serde_json::to_string(&manual).unwrap();
        assert!(text.ends_with(r#""method":{"kind":"manual","reason":"no_brew"}}"#), "{text}");
    }

    #[test]
    fn the_way_to_update_is_judged_only_for_a_newer_version() {
        let asked = Cell::new(0);
        let judge = |current: &str, tag: &str, method: UpdateMethod| {
            run(check(current, || async { Ok(release(tag)) }, || async {
                asked.set(asked.get() + 1);
                method
            }))
        };
        let manual = UpdateMethod::Manual { reason: ManualReason::NotInstalledByCask };
        let found = judge("0.2.0", "v0.3.0", manual);
        assert_eq!(found, UpdateCheck::Available { current: "0.2.0".into(), latest: "0.3.0".into(), method: manual });
        assert_eq!(asked.get(), 1);
        // Up to date, or no answer from GitHub: Homebrew is not looked at.
        assert!(matches!(judge("0.2.0", "v0.2.0", UpdateMethod::Brew), UpdateCheck::UpToDate { .. }));
        assert!(matches!(judge("0.2.0", "latest", UpdateMethod::Brew), UpdateCheck::Unknown { .. }));
        assert!(matches!(judge("dev", "v0.3.0", UpdateMethod::Brew), UpdateCheck::Unknown { .. }));
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn brew_is_looked_for_in_order_without_path() {
        let both = |_: &Path| true;
        assert_eq!(find_brew(&both), Some(PathBuf::from("/opt/homebrew/bin/brew")));
        let intel_only = |p: &Path| p == Path::new("/usr/local/bin/brew");
        assert_eq!(find_brew(&intel_only), Some(PathBuf::from("/usr/local/bin/brew")));
        assert_eq!(find_brew(&|_: &Path| false), None);
    }

    #[test]
    fn the_update_commands_are_argument_lists() {
        assert_eq!(update_steps(), [vec!["update"], vec!["upgrade", "--cask", "binone"]]);
    }

    #[test]
    fn an_app_bundle_is_told_from_a_development_build() {
        assert!(is_app_bundle(Path::new("/Applications/binone.app/Contents/MacOS/binone")));
        assert!(is_app_bundle(Path::new("/Users/someone/Applications/binone.app/Contents/MacOS/binone")));
        assert!(!is_app_bundle(Path::new("/work/client/src-tauri/target/debug/binone")));
        assert!(!is_app_bundle(Path::new("/work/binone.app.d/binone")));
    }

    /// Answers each call from a script and remembers what was run.
    struct Fake {
        script: RefCell<Vec<Result<RunOutput, String>>>,
        calls: RefCell<Vec<String>>,
        limits: RefCell<Vec<Duration>>,
    }

    impl Fake {
        fn new(script: Vec<Result<RunOutput, String>>) -> Self {
            Fake { script: RefCell::new(script), calls: RefCell::new(Vec::new()), limits: RefCell::new(Vec::new()) }
        }
    }

    impl Runner for Fake {
        fn run(&self, program: &Path, args: &[&str], limit: Duration) -> Result<RunOutput, String> {
            self.limits.borrow_mut().push(limit);
            self.calls.borrow_mut().push(format!("{} {}", program.display(), args.join(" ")));
            self.script.borrow_mut().remove(0)
        }
    }

    fn out(code: i32, text: &str) -> Result<RunOutput, String> {
        Ok(RunOutput { code: Some(code), text: text.to_owned(), timed_out: false })
    }

    fn stopped(text: &str) -> Result<RunOutput, String> {
        Ok(RunOutput { code: None, text: text.to_owned(), timed_out: true })
    }

    fn brew() -> Plan {
        Plan::Brew(PathBuf::from(BREW))
    }

    const LIMIT: Duration = Duration::from_secs(600);

    const BREW: &str = "/opt/homebrew/bin/brew";

    #[test]
    fn with_brew_and_the_cask_the_plan_is_brew() {
        let fake = Fake::new(vec![out(0, "binone")]);
        assert_eq!(plan(true, &|_| true, &fake, LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(BREW)));
        assert_eq!(*fake.calls.borrow(), [format!("{BREW} list --cask binone")]);
        assert_eq!(*fake.limits.borrow(), [Duration::from_secs(10)]);
        assert_eq!(Plan::Brew(PathBuf::from(BREW)).method(), UpdateMethod::Brew);
    }

    #[test]
    fn a_list_that_runs_out_of_time_is_not_the_cask() {
        let fake = Fake::new(vec![stopped("")]);
        assert_eq!(plan(true, &|_| true, &fake, LIST_TIME_LIMIT), Plan::Manual(ManualReason::NotInstalledByCask));
        assert_eq!(
            Plan::Manual(ManualReason::NoBrew).method(),
            UpdateMethod::Manual { reason: ManualReason::NoBrew }
        );
    }

    #[test]
    fn without_brew_nothing_is_run() {
        let fake = Fake::new(vec![]);
        assert_eq!(plan(true, &|_| false, &fake, LIST_TIME_LIMIT), Plan::Manual(ManualReason::NoBrew));
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn without_the_cask_or_outside_a_bundle_the_plan_is_manual() {
        let not_installed = Fake::new(vec![out(1, "Error: Cask 'binone' is not installed.")]);
        assert_eq!(plan(true, &|_| true, &not_installed, LIST_TIME_LIMIT), Plan::Manual(ManualReason::NotInstalledByCask));
        let cannot_start = Fake::new(vec![Err("could not start".into())]);
        assert_eq!(plan(true, &|_| true, &cannot_start, LIST_TIME_LIMIT), Plan::Manual(ManualReason::NotInstalledByCask));
        // A development build is not the cask's app, even when the cask is installed.
        let dev = Fake::new(vec![]);
        assert_eq!(plan(false, &|_| true, &dev, LIST_TIME_LIMIT), Plan::Manual(ManualReason::NotInstalledByCask));
        assert!(dev.calls.borrow().is_empty());
    }

    #[test]
    fn a_manual_plan_runs_nothing_and_gives_the_page_and_the_command() {
        let fake = Fake::new(vec![]);
        let outcome = run_update(&Plan::Manual(ManualReason::NoBrew), &fake, LIMIT);
        assert_eq!(outcome, UpdateOutcome::Manual { reason: ManualReason::NoBrew, command: MANUAL_COMMAND, url: RELEASES_PAGE_URL });
        assert!(fake.calls.borrow().is_empty());
        assert_eq!(MANUAL_COMMAND, "brew upgrade --cask binone");
        let text = serde_json::to_string(&outcome).unwrap();
        assert!(text.starts_with(r#"{"state":"manual","reason":"no_brew""#), "{text}");
    }

    #[test]
    fn an_update_runs_both_steps_in_order() {
        let fake = Fake::new(vec![out(0, "Updated 1 tap"), out(0, "==> Upgrading binone\n🍺  binone was successfully upgraded!")]);
        let outcome = run_update(&brew(), &fake, LIMIT);
        assert_eq!(*fake.calls.borrow(), [format!("{BREW} update"), format!("{BREW} upgrade --cask binone")]);
        let UpdateOutcome::Finished { ok, code, step, output, .. } = outcome else { panic!() };
        assert!(ok);
        assert_eq!(code, Some(0));
        assert_eq!(step, "brew upgrade --cask binone");
        assert!(output.contains("successfully upgraded"));
    }

    #[test]
    fn a_failing_step_stops_the_update_with_its_code_and_output() {
        let fake = Fake::new(vec![out(1, "Error: could not update")]);
        let outcome = run_update(&brew(), &fake, LIMIT);
        assert_eq!(fake.calls.borrow().len(), 1);
        assert_eq!(outcome, UpdateOutcome::Finished { ok: false, code: Some(1), step: "brew update".into(), output: "Error: could not update".into(), timed_out: false });

        let fake = Fake::new(vec![out(0, ""), out(1, "Error: Cask 'binone' is not installed")]);
        let UpdateOutcome::Finished { ok, code, step, .. } = run_update(&brew(), &fake, LIMIT) else { panic!() };
        assert!(!ok);
        assert_eq!((code, step.as_str()), (Some(1), "brew upgrade --cask binone"));

        let killed = Fake::new(vec![Ok(RunOutput { code: None, text: String::new(), timed_out: false })]);
        let UpdateOutcome::Finished { ok, code, .. } = run_update(&brew(), &killed, LIMIT) else { panic!() };
        assert!(!ok && code.is_none());

        let missing = Fake::new(vec![Err("could not start brew: NotFound".into())]);
        let UpdateOutcome::Finished { ok, output, .. } = run_update(&brew(), &missing, LIMIT) else { panic!() };
        assert!(!ok && output.contains("NotFound"));
    }

    #[test]
    fn a_step_out_of_time_ends_the_update_as_timed_out_with_the_output_so_far() {
        let fake = Fake::new(vec![out(0, ""), stopped("==> Downloading\nstill going")]);
        let outcome = run_update(&brew(), &fake, LIMIT);
        let UpdateOutcome::Finished { ok, code, step, output, timed_out } = outcome else { panic!() };
        assert!(!ok && timed_out && code.is_none());
        assert_eq!(step, "brew upgrade --cask binone");
        assert!(output.contains("still going"));
        assert!(serde_json::to_string(&UpdateOutcome::Finished { ok, code, step, output, timed_out }).unwrap().contains(r#""timed_out":true"#));
        // The first step too: nothing after it is run.
        let fake = Fake::new(vec![stopped("x")]);
        assert!(matches!(run_update(&brew(), &fake, LIMIT), UpdateOutcome::Finished { timed_out: true, .. }));
        assert_eq!(fake.calls.borrow().len(), 1);
    }

    #[test]
    fn the_steps_share_one_time_limit() {
        let fake = Fake::new(vec![out(0, ""), out(0, "")]);
        run_update(&brew(), &fake, LIMIT);
        let limits = fake.limits.borrow();
        assert!(limits[0] <= LIMIT && limits[0] > LIMIT - Duration::from_secs(5));
        assert!(limits[1] <= limits[0]);
        // A limit already used up leaves nothing for the next step.
        let fake = Fake::new(vec![out(0, ""), out(0, "")]);
        run_update(&brew(), &fake, Duration::ZERO);
        assert_eq!(*fake.limits.borrow(), [Duration::ZERO, Duration::ZERO]);
    }

    #[test]
    fn the_judgment_shares_the_one_limit_with_the_update() {
        let fake = Fake::new(vec![out(0, "binone"), out(0, ""), out(0, "")]);
        let outcome = detect_and_update(true, &|_| true, &fake, LIMIT);
        assert!(matches!(outcome, UpdateOutcome::Finished { ok: true, .. }), "{outcome:?}");
        let limits = fake.limits.borrow();
        assert_eq!(limits[0], LIST_TIME_LIMIT);
        assert!(limits[1] <= LIMIT && limits[2] <= limits[1], "{limits:?}");
        // A limit shorter than the list limit caps the list call too.
        let short = Fake::new(vec![stopped("")]);
        detect_and_update(true, &|_| true, &short, Duration::from_secs(3));
        assert_eq!(short.limits.borrow()[0], Duration::from_secs(3));
    }

    #[test]
    fn a_judgment_that_used_the_time_leaves_less_for_the_steps() {
        struct Slow(Fake);
        impl Runner for Slow {
            fn run(&self, program: &Path, args: &[&str], limit: Duration) -> Result<RunOutput, String> {
                std::thread::sleep(Duration::from_millis(200));
                self.0.run(program, args, limit)
            }
        }
        let slow = Slow(Fake::new(vec![out(0, "binone"), out(0, ""), out(0, "")]));
        detect_and_update(true, &|_| true, &slow, Duration::from_secs(60));
        let limits = slow.0.limits.borrow();
        assert!(limits[1] < Duration::from_secs(60) - Duration::from_millis(150), "{limits:?}");
    }

    #[test]
    fn the_output_is_cut_to_its_end() {
        let many: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        let cut = tail(&many);
        assert_eq!(cut.lines().count(), OUTPUT_TAIL_LINES);
        assert!(cut.ends_with("line 100") && cut.starts_with("line 81"));
        let long = "x".repeat(10_000);
        assert_eq!(tail(&long).chars().count(), OUTPUT_TAIL_CHARS);
        assert_eq!(tail(""), "");
    }

    #[test]
    fn only_one_update_runs_at_a_time() {
        let gate = UpdateGate::default();
        let first = gate.enter();
        assert!(first.is_some());
        assert!(gate.enter().is_none());
        drop(first);
        assert!(gate.enter().is_some());
    }

    #[test]
    fn the_gate_is_open_again_however_the_update_ends() {
        let gate = UpdateGate::default();
        type Work = Box<dyn FnOnce() -> UpdateOutcome + Send>;
        let ending: [(&str, Work); 4] = [
            ("success", Box::new(|| run_update(&brew(), &Fake::new(vec![out(0, ""), out(0, "ok")]), LIMIT))),
            ("failure", Box::new(|| run_update(&brew(), &Fake::new(vec![out(1, "Error")]), LIMIT))),
            ("timeout", Box::new(|| run_update(&brew(), &Fake::new(vec![stopped("slow")]), LIMIT))),
            ("panic", Box::new(|| panic!("the update panicked"))),
        ];
        for (name, work) in ending {
            let answer = run(run_exclusive(&gate, work));
            match name {
                "panic" => assert!(answer.is_err(), "{answer:?}"),
                "success" => assert!(matches!(answer, Ok(UpdateOutcome::Finished { ok: true, .. })), "{answer:?}"),
                _ => assert!(matches!(answer, Ok(UpdateOutcome::Finished { ok: false, .. })), "{name}: {answer:?}"),
            }
            let again = gate.enter();
            assert!(again.is_some(), "the gate stayed closed after {name}");
        }
    }

    #[test]
    fn a_second_update_is_refused_while_one_runs() {
        let gate = UpdateGate::default();
        let _running = gate.enter();
        assert_eq!(run(run_exclusive(&gate, || panic!("must not run"))), Ok(UpdateOutcome::Busy));
    }

    #[cfg(unix)]
    mod system {
        use super::*;

        fn sh(script: &str, limit: Duration) -> (Result<RunOutput, String>, Duration) {
            let started = Instant::now();
            let answer = SystemRunner.run(Path::new("/bin/sh"), &["-c", script], limit);
            (answer, started.elapsed())
        }

        #[test]
        fn a_program_that_finishes_gives_its_code_and_output() {
            let (answer, _) = sh("echo out; echo err >&2; exit 3", Duration::from_secs(30));
            let output = answer.unwrap();
            assert_eq!(output.code, Some(3));
            assert!(!output.timed_out);
            assert!(output.text.contains("out") && output.text.contains("err"), "{:?}", output.text);
        }

        #[test]
        fn a_program_past_its_limit_is_killed_and_what_it_wrote_is_kept() {
            let (answer, took) = sh("echo before; exec sleep 60", Duration::from_millis(300));
            let output = answer.unwrap();
            assert!(output.timed_out);
            assert_eq!(output.code, None);
            assert!(output.text.contains("before"), "{:?}", output.text);
            assert!(took < Duration::from_secs(20), "{took:?}");
        }

        /// Whether a process still runs (a zombie waiting to be reaped by someone else does not count).
        fn alive(pid: u32) -> bool {
            let shown = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().unwrap();
            let stat = String::from_utf8_lossy(&shown.stdout).trim().to_owned();
            !stat.is_empty() && !stat.starts_with('Z')
        }

        #[test]
        fn a_program_past_its_limit_takes_its_children_with_it() {
            // The program and its child both ignore TERM, so only the KILL to the group ends them.
            let (answer, took) = sh("trap '' TERM; sleep 300 & echo child=$!; wait", Duration::from_millis(300));
            let output = answer.unwrap();
            let pid: u32 = output.text.split("child=").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("{:?}", output.text));
            let mut gone = false;
            for _ in 0..100 {
                if !alive(pid) {
                    gone = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if !gone {
                let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
            }
            assert!(output.timed_out);
            assert!(gone, "the child {pid} outlived the time-out");
            assert!(took < Duration::from_secs(20), "{took:?}");
        }

        #[test]
        fn the_wait_for_the_output_does_not_pass_its_limit_while_a_pipe_is_held() {
            use std::process::Stdio;
            // `sleep` holds its (never written) standard output open, like a child that keeps the pipe.
            let mut holder = std::process::Command::new("sleep").arg("300").stdout(Stdio::piped()).spawn().unwrap();
            let (buffer, reader) = collect(holder.stdout.take().unwrap());
            let started = Instant::now();
            drain(vec![reader], Duration::from_millis(300));
            let took = started.elapsed();
            let _ = holder.kill();
            let _ = holder.wait();
            assert!(took >= Duration::from_millis(250) && took < Duration::from_secs(5), "{took:?}");
            assert_eq!(text_of(&buffer), "");
        }

        #[test]
        fn what_was_written_just_before_the_stop_is_kept_to_its_end() {
            let (answer, _) = sh("trap '' TERM; i=0; while [ $i -lt 20 ]; do echo line$i; i=$((i+1)); done; echo last-line; exec sleep 300", Duration::from_millis(500));
            let output = answer.unwrap();
            assert!(output.timed_out);
            assert!(output.text.trim_end().ends_with("last-line"), "{:?}", output.text);
        }

        #[test]
        fn a_program_that_cannot_start_is_an_error() {
            let answer = SystemRunner.run(Path::new("/nonexistent/brew"), &[], Duration::from_secs(1));
            assert!(answer.unwrap_err().contains("could not start"));
        }
    }
}
