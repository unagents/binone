//! Checking for a newer client and updating it through Homebrew.
//!
//! Both happen on this side: the webview never reaches GitHub and never starts a program. The
//! latest release is read from GitHub's public API without credentials; the update refreshes the
//! tap `unagents/tap` with git, runs `brew upgrade --cask binone` and then reads the installed
//! version back, with each program and its arguments given separately (no shell). Nothing here
//! touches the app's data directory, where the connection settings and the key are kept.
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
/// The tap that holds the cask: only this one is refreshed (not every tap, as `brew update` would).
const TAP: &str = "unagents/tap";
const GIT: &str = "/usr/bin/git";
/// Tells whether the Command Line Tools are installed (without them `git` is a shim that opens an
/// installer dialog instead of running).
const XCODE_SELECT: &str = "/usr/bin/xcode-select";
const UPGRADE_ARGS: [&str; 3] = ["upgrade", "--cask", CASK];
/// Set for the upgrade: the tap was refreshed above, and an automatic update of every tap is what
/// failed on a user's Mac.
const UPGRADE_ENV: [(&str, &str); 1] = [("HOMEBREW_NO_AUTO_UPDATE", "1")];
/// Looked for in this order (the first is preferred when both have the cask); `PATH` is not
/// consulted (an app started from Finder has a short one).
const BREW_CANDIDATES: [&str; 2] = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"];
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// The whole update (the choice of brew, the tap, the upgrade and the check of the result) may take this long.
pub const UPDATE_TIME_LIMIT: Duration = Duration::from_secs(10 * 60);
/// The `git pull` of the tap: a tap that cannot be reached must not use up the time of the upgrade.
pub const TAP_PULL_TIME_LIMIT: Duration = Duration::from_secs(60);
/// Each brew call made to find out how to update (`brew list --cask binone`, once per brew).
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

/// Why the latest version could not be found out, by kind. `text()` is the sentence shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckFailure {
    /// GitHub could not be reached (the detail says how).
    Network(&'static str),
    /// GitHub refused (a rate limit is the usual cause); carries the HTTP status.
    Refused(u16),
    /// No release has been published (404).
    NoRelease,
    /// The answer does not have the expected shape (the detail says what is wrong).
    Shape(String),
    /// Anything else, with its own words.
    Other(String),
}

impl CheckFailure {
    pub fn text(&self) -> String {
        match self {
            CheckFailure::Network(detail) => format!("GitHub に届きません（{detail}）"),
            CheckFailure::Refused(status) => format!("GitHub に断られました（回数の上限の可能性があります。HTTP {status}）"),
            CheckFailure::NoRelease => "公開された release がありません（HTTP 404）".to_owned(),
            CheckFailure::Shape(detail) => format!("GitHub の応答の形が想定と違います（{detail}）"),
            CheckFailure::Other(detail) => format!("確認できませんでした（{detail}）"),
        }
    }
}

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
    match answer.status {
        200 => {}
        status @ (403 | 429) => return Err(CheckFailure::Refused(status)),
        404 => return Err(CheckFailure::NoRelease),
        status => return Err(CheckFailure::Other(format!("GitHub が HTTP {status} を返しました"))),
    }
    let shape = |text: &str| Err(CheckFailure::Shape(text.to_owned()));
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&answer.body) else {
        return shape("JSON ではありません");
    };
    let flag = |name: &str| value.get(name).and_then(|v| v.as_bool()).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return shape("最新の release が下書きか先行版です");
    }
    let Some(tag) = value.get("tag_name").and_then(|v| v.as_str()) else {
        return shape("tag_name がありません");
    };
    match Version::parse(tag) {
        Some(version) => Ok(version),
        None => shape(&format!("tag「{}」は版ではありません", tag.chars().take(40).collect::<String>())),
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
        return unknown(CheckFailure::Other(format!("このアプリの版「{current}」は a.b.c の形ではありません")).text());
    };
    let latest = match fetch().await.and_then(|answer| interpret_release(&answer)) {
        Ok(latest) => latest,
        Err(failure) => return unknown(failure.text()),
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
        .map_err(|_| CheckFailure::Other("HTTP の用意ができませんでした".to_owned()))?;
    let failure = |e: &reqwest::Error| {
        CheckFailure::Network(if e.is_timeout() {
            "時間切れです"
        } else if e.is_connect() {
            "接続できません"
        } else {
            "要求に失敗しました"
        })
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

/// Runs a program with its arguments (never through a shell), with `env` added to the
/// environment it inherits. A program that is still running after `limit` is killed and reaped,
/// and the answer says so.
pub trait Runner {
    fn run(&self, program: &Path, args: &[&str], env: &[(&str, &str)], limit: Duration) -> Result<RunOutput, String>;
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
    fn run(&self, program: &Path, args: &[&str], env: &[(&str, &str)], limit: Duration) -> Result<RunOutput, String> {
        use std::process::Stdio;
        let started = |e: std::io::Error| format!("{} を起動できませんでした（{}）", program.display(), e.kind());
        let mut command = std::process::Command::new(program);
        command.args(args).envs(env.iter().copied()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().map_err(started)?;
        let (stdout, out_reader) = collect(child.stdout.take().ok_or("標準出力を取得できませんでした")?);
        let (stderr, err_reader) = collect(child.stderr.take().ok_or("標準エラー出力を取得できませんでした")?);
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
                    return Err(format!("{} の終わりを待てませんでした（{}）", program.display(), e.kind()));
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

/// The usual places that have a brew, in the order of preference.
pub fn find_brews(exists: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    BREW_CANDIDATES.iter().map(PathBuf::from).filter(|path| exists(path)).collect()
}

/// The `.app` bundle that the running executable is inside (a development build has none).
pub fn app_bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|dir| dir.extension().is_some_and(|e| e == "app") && exe.strip_prefix(dir).is_ok_and(|rest| rest.starts_with("Contents/MacOS")))
        .map(Path::to_path_buf)
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

/// Whether a line of `brew list` names `bundle` itself (not a longer name that begins with it).
fn names_bundle(line: &str, bundle: &str) -> bool {
    line.trim().strip_prefix(bundle).is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '/']))
}

/// Whether the update can be run with Homebrew, and with which one. Each brew that exists is
/// asked `brew list --cask binone` (at most `list_limit` each); those where it succeeds have the
/// cask. With the cask in both, the one whose list names the running app's bundle is used (the
/// first of the usual places when that cannot be told). A development build (`bundle` is `None`)
/// is not the cask's app, and nothing is asked.
pub fn plan(bundle: Option<&Path>, exists: &dyn Fn(&Path) -> bool, runner: &dyn Runner, list_limit: Duration) -> Plan {
    let brews = find_brews(exists);
    if brews.is_empty() {
        return Plan::Manual(ManualReason::NoBrew);
    }
    let Some(bundle) = bundle else {
        return Plan::Manual(ManualReason::NotInstalledByCask);
    };
    let bundle = bundle.to_string_lossy();
    let mut chosen: Option<PathBuf> = None;
    for brew in brews {
        let Ok(RunOutput { code: Some(0), timed_out: false, text }) = runner.run(&brew, &["list", "--cask", CASK], &[], list_limit) else {
            continue;
        };
        // `brew list --cask` prints the paths of what the cask installed, the app among them.
        if text.lines().any(|line| names_bundle(line, &bundle)) {
            return Plan::Brew(brew);
        }
        chosen.get_or_insert(brew);
    }
    chosen.map_or(Plan::Manual(ManualReason::NotInstalledByCask), Plan::Brew)
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
    /// `note`: something that went wrong on the way without stopping the update (the tap).
    Finished { ok: bool, code: Option<i32>, step: String, output: String, timed_out: bool, note: Option<String> },
}

/// The last lines of `text`, limited in size.
pub fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let joined = lines[lines.len().saturating_sub(OUTPUT_TAIL_LINES)..].join("\n");
    let skip = joined.chars().count().saturating_sub(OUTPUT_TAIL_CHARS);
    joined.chars().skip(skip).collect()
}

/// The last non-empty line of a program's text (what `brew --repo` prints is one path).
fn last_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).rfind(|line| !line.is_empty())
}

/// Refreshes the tap with git: `brew --repo unagents/tap` for its directory, `xcode-select -p`
/// for the Command Line Tools, then `git pull --ff-only` in it (at most
/// `TAP_PULL_TIME_LIMIT`). It never stops the update; what went wrong is returned as a sentence.
fn refresh_tap(brew: &Path, runner: &dyn Runner, deadline: Instant) -> Option<String> {
    let left = || deadline.saturating_duration_since(Instant::now());
    let why = |reason: String| Some(format!("tap {TAP} は更新できませんでした（{reason}）。"));
    let dir = match runner.run(brew, &["--repo", TAP], &[], left()) {
        Ok(RunOutput { code: Some(0), timed_out: false, text }) => last_line(&text).map(str::to_owned),
        Ok(RunOutput { timed_out: true, .. }) => return why("tap の場所を調べている間に時間切れになりました".into()),
        Ok(_) => None,
        Err(text) => return why(text),
    };
    let Some(dir) = dir else {
        return why("tap の場所が分かりません".into());
    };
    // Without the Command Line Tools `git` would open an installer dialog: it is not called then.
    let tools_note = || Some("Command Line Tools が無いので tap の更新を省いた（git は呼んでいません）。".to_owned());
    match runner.run(Path::new(XCODE_SELECT), &["-p"], &[], left().min(LIST_TIME_LIMIT)) {
        Ok(RunOutput { code: Some(0), timed_out: false, .. }) => {}
        Ok(RunOutput { timed_out: true, .. }) => return why("Command Line Tools の有無を調べている間に時間切れになりました".into()),
        // A non-zero code, or no such program: they are not there.
        Ok(_) | Err(_) => return tools_note(),
    }
    match runner.run(Path::new(GIT), &["-C", &dir, "pull", "--ff-only"], &[], left().min(TAP_PULL_TIME_LIMIT)) {
        Ok(RunOutput { code: Some(0), timed_out: false, .. }) => None,
        Ok(RunOutput { timed_out: true, .. }) => why(format!("tap の更新が時間切れになりました（git pull は {} 秒まで）", TAP_PULL_TIME_LIMIT.as_secs())),
        Ok(RunOutput { code, .. }) => why(match code {
            Some(code) => format!("git pull が終了 code {code} で失敗しました"),
            None => "git pull が途中で止まりました".to_owned(),
        }),
        Err(text) => why(text),
    }
}

/// The versions `brew list --cask --versions binone` printed (`binone 0.3.0`), when it printed any.
fn installed_versions(text: &str) -> Vec<Version> {
    let Some(line) = text.lines().map(str::trim).find(|line| line.split_whitespace().next() == Some(CASK)) else {
        return Vec::new();
    };
    line.split_whitespace().skip(1).filter_map(Version::parse).collect()
}

/// Runs the update as planned: the tap, the upgrade (with no automatic update of every tap), and
/// then a look at the installed version, which has to be `latest` for the update to count as done
/// (brew ends with code 0 when it did nothing). `limit` is for the whole update: each step gets
/// what is left of it, and a step that goes past it is killed.
pub fn run_update(plan: &Plan, latest: &str, runner: &dyn Runner, limit: Duration) -> UpdateOutcome {
    let brew = match plan {
        Plan::Manual(reason) => return UpdateOutcome::Manual { reason: *reason, command: MANUAL_COMMAND, url: RELEASES_PAGE_URL },
        Plan::Brew(brew) => brew,
    };
    let deadline = Instant::now() + limit;
    let left = || deadline.saturating_duration_since(Instant::now());
    let note = refresh_tap(brew, runner, deadline);

    let step = format!("brew {}", UPGRADE_ARGS.join(" "));
    let finished = |ok, code, output: String, timed_out| UpdateOutcome::Finished { ok, code, step: step.clone(), output, timed_out, note: note.clone() };
    let (code, text) = match runner.run(brew, &UPGRADE_ARGS, &UPGRADE_ENV, left()) {
        Err(text) => return finished(false, None, text, false),
        Ok(RunOutput { code, text, timed_out }) if timed_out || code != Some(0) => return finished(false, code, tail(&text), timed_out),
        Ok(RunOutput { code, text, .. }) => (code, text),
    };

    let latest = Version::parse(latest);
    let shown = latest.map_or_else(|| "不明".to_owned(), |v| v.to_string());
    let verdict = match runner.run(brew, &["list", "--cask", "--versions", CASK], &[], left()) {
        Ok(RunOutput { code: Some(0), timed_out: false, text }) => {
            let versions = installed_versions(&text);
            match (latest, versions.as_slice()) {
                (Some(latest), versions) if versions.contains(&latest) => None,
                (_, []) => Some(format!("更新できたか確認できませんでした（入っている版を読めません、最新 {shown}）")),
                (None, _) => Some(format!("更新できたか確認できませんでした（最新の版が分かりません、入っている版 {}）", join_versions(&versions))),
                (Some(_), versions) => Some(format!("更新されませんでした（入っている版 {}、最新 {shown}）", join_versions(versions))),
            }
        }
        _ => Some(format!("更新できたか確認できませんでした（入っている版を読めません、最新 {shown}）")),
    };
    let mut output = tail(&text);
    match verdict {
        None => finished(true, code, output, false),
        Some(line) => {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&line);
            finished(false, code, output, false)
        }
    }
}

fn join_versions(versions: &[Version]) -> String {
    versions.iter().map(Version::to_string).collect::<Vec<_>>().join("、")
}

/// What "更新" runs: the judgment of the way (`brew list` for each brew, at most `LIST_TIME_LIMIT`
/// each) and then the update, all within the one `limit`: the judgment is part of it, not extra.
pub fn detect_and_update(bundle: Option<&Path>, latest: &str, exists: &dyn Fn(&Path) -> bool, runner: &dyn Runner, limit: Duration) -> UpdateOutcome {
    let deadline = Instant::now() + limit;
    let plan = plan(bundle, exists, runner, LIST_TIME_LIMIT.min(limit));
    run_update(&plan, latest, runner, deadline.saturating_duration_since(Instant::now()))
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
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("更新を実行できませんでした（{e}）"))
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

    fn reason_of(result: Result<HttpAnswer, CheckFailure>) -> String {
        let UpdateCheck::Unknown { reason, .. } = checked("0.2.0", result) else { panic!() };
        reason
    }

    #[test]
    fn each_kind_of_failure_has_its_own_sentence_in_japanese() {
        let refused = reason_of(Ok(answer(403, r#"{"message":"API rate limit exceeded"}"#)));
        assert_eq!(refused, "GitHub に断られました（回数の上限の可能性があります。HTTP 403）");
        assert!(reason_of(Ok(answer(429, ""))).contains("HTTP 429"));
        assert_eq!(reason_of(Ok(answer(404, "{}"))), "公開された release がありません（HTTP 404）");
        assert_eq!(reason_of(Ok(answer(500, ""))), "確認できませんでした（GitHub が HTTP 500 を返しました）");
        assert_eq!(reason_of(Ok(answer(200, "not json"))), "GitHub の応答の形が想定と違います（JSON ではありません）");
        assert_eq!(reason_of(Ok(answer(200, "{}"))), "GitHub の応答の形が想定と違います（tag_name がありません）");
        assert!(reason_of(Ok(answer(200, r#"{"tag_name":"latest"}"#))).contains("tag「latest」は版ではありません"));
        assert_eq!(reason_of(Err(CheckFailure::Network("接続できません"))), "GitHub に届きません（接続できません）");
        assert_eq!(reason_of(Err(CheckFailure::Other("何か".into()))), "確認できませんでした（何か）");
        // No English is left in any of them (the status number and the names are the only ASCII words).
        for reason in [&refused, &reason_of(Ok(answer(500, "")))] {
            assert!(!reason.contains("refused") && !reason.contains("answered") && !reason.contains("could not"), "{reason}");
        }
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
    fn brews_are_looked_for_in_order_without_path() {
        let both = |_: &Path| true;
        assert_eq!(find_brews(&both), [PathBuf::from("/opt/homebrew/bin/brew"), PathBuf::from("/usr/local/bin/brew")]);
        let intel_only = |p: &Path| p == Path::new("/usr/local/bin/brew");
        assert_eq!(find_brews(&intel_only), [PathBuf::from("/usr/local/bin/brew")]);
        assert!(find_brews(&|_: &Path| false).is_empty());
    }

    #[test]
    fn an_app_bundle_is_told_from_a_development_build() {
        let bundle = |p: &str| app_bundle(Path::new(p));
        assert_eq!(bundle("/Applications/binone.app/Contents/MacOS/binone"), Some(PathBuf::from("/Applications/binone.app")));
        assert_eq!(bundle("/Users/someone/Applications/binone.app/Contents/MacOS/binone"), Some(PathBuf::from("/Users/someone/Applications/binone.app")));
        assert_eq!(bundle("/work/client/src-tauri/target/debug/binone"), None);
        assert_eq!(bundle("/work/binone.app.d/binone"), None);
    }

    /// Answers each call from a script and remembers what was run.
    struct Fake {
        script: RefCell<Vec<Result<RunOutput, String>>>,
        calls: RefCell<Vec<String>>,
        envs: RefCell<Vec<Vec<(String, String)>>>,
        limits: RefCell<Vec<Duration>>,
    }

    impl Fake {
        fn new(script: Vec<Result<RunOutput, String>>) -> Self {
            Fake { script: RefCell::new(script), calls: RefCell::new(Vec::new()), envs: RefCell::new(Vec::new()), limits: RefCell::new(Vec::new()) }
        }
    }

    impl Runner for Fake {
        fn run(&self, program: &Path, args: &[&str], env: &[(&str, &str)], limit: Duration) -> Result<RunOutput, String> {
            self.limits.borrow_mut().push(limit);
            self.calls.borrow_mut().push(format!("{} {}", program.display(), args.join(" ")));
            self.envs.borrow_mut().push(env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect());
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
    const INTEL: &str = "/usr/local/bin/brew";
    const TAP_DIR: &str = "/opt/homebrew/Library/Taps/unagents/homebrew-tap";
    const BUNDLE: &str = "/Applications/binone.app";
    const TOOLS_DIR: &str = "/Library/Developer/CommandLineTools";

    /// The calls of a whole update when everything goes well, up to the upgrade's answer.
    fn tap_ok() -> Vec<Result<RunOutput, String>> {
        vec![out(0, &format!("{TAP_DIR}\n")), out(0, TOOLS_DIR), out(0, "Already up to date.")]
    }

    fn update_script(upgrade: Result<RunOutput, String>, installed: Result<RunOutput, String>) -> Fake {
        let mut script = tap_ok();
        script.push(upgrade);
        script.push(installed);
        Fake::new(script)
    }

    fn finished(outcome: UpdateOutcome) -> (bool, Option<i32>, String, String, bool, Option<String>) {
        let UpdateOutcome::Finished { ok, code, step, output, timed_out, note } = outcome else { panic!("{outcome:?}") };
        (ok, code, step, output, timed_out, note)
    }

    // --- Which brew -------------------------------------------------------------------------

    #[test]
    fn with_one_brew_that_has_the_cask_the_plan_is_that_brew() {
        let fake = Fake::new(vec![out(0, "binone")]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| true, &Fake::new(vec![out(0, "x"), out(1, "")]), LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(BREW)));
        let only_intel = |p: &Path| p == Path::new(INTEL);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &only_intel, &fake, LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(INTEL)));
        assert_eq!(*fake.calls.borrow(), [format!("{INTEL} list --cask binone")]);
        assert_eq!(*fake.limits.borrow(), [Duration::from_secs(10)]);
        assert_eq!(Plan::Brew(PathBuf::from(BREW)).method(), UpdateMethod::Brew);
    }

    #[test]
    fn the_brew_that_has_the_cask_is_used_even_when_it_is_the_second() {
        // The first brew is there but does not have the cask (the case on the Mac this was found on, reversed).
        let fake = Fake::new(vec![out(1, "Error: Cask 'binone' is not installed."), out(0, "binone")]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| true, &fake, LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(INTEL)));
        assert_eq!(*fake.calls.borrow(), [format!("{BREW} list --cask binone"), format!("{INTEL} list --cask binone")]);
        // A brew that cannot start or runs out of time does not have it either.
        let broken = Fake::new(vec![Err("x".into()), out(0, "")]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| true, &broken, LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(INTEL)));
        let slow = Fake::new(vec![stopped(""), out(0, "")]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| true, &slow, LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(INTEL)));
    }

    #[test]
    fn with_the_cask_in_both_the_one_that_names_the_running_bundle_is_used() {
        let names_it = format!("/usr/local/Caskroom/binone/0.3.0\n{BUNDLE} (1 files)");
        let other = "/opt/homebrew/Caskroom/binone/0.3.0\n/Users/x/Applications/binone.app (1 files)";
        let fake = Fake::new(vec![out(0, other), out(0, &names_it)]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| true, &fake, LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(INTEL)));
        // The first one names it: the second is not asked.
        let first = Fake::new(vec![out(0, &names_it)]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| true, &first, LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(BREW)));
        assert_eq!(first.calls.borrow().len(), 1);
        // Neither names it (or nothing is printed): /opt/homebrew first.
        let unclear = Fake::new(vec![out(0, ""), out(0, "")]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| true, &unclear, LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(BREW)));
        // The bundle of another place is not mistaken for a longer name of this one.
        let longer = Fake::new(vec![out(0, "/Applications/binone.app.old"), out(0, &names_it)]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| true, &longer, LIST_TIME_LIMIT), Plan::Brew(PathBuf::from(INTEL)));
    }

    #[test]
    fn with_the_cask_in_neither_the_plan_is_manual() {
        let fake = Fake::new(vec![out(1, "Error"), out(1, "Error")]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| true, &fake, LIST_TIME_LIMIT), Plan::Manual(ManualReason::NotInstalledByCask));
        assert_eq!(fake.calls.borrow().len(), 2);
        assert_eq!(Plan::Manual(ManualReason::NoBrew).method(), UpdateMethod::Manual { reason: ManualReason::NoBrew });
    }

    #[test]
    fn without_brew_or_outside_a_bundle_nothing_is_run() {
        let fake = Fake::new(vec![]);
        assert_eq!(plan(Some(Path::new(BUNDLE)), &|_| false, &fake, LIST_TIME_LIMIT), Plan::Manual(ManualReason::NoBrew));
        // A development build is not the cask's app, even when the cask is installed.
        assert_eq!(plan(None, &|_| true, &fake, LIST_TIME_LIMIT), Plan::Manual(ManualReason::NotInstalledByCask));
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn a_manual_plan_runs_nothing_and_gives_the_page_and_the_command() {
        let fake = Fake::new(vec![]);
        let outcome = run_update(&Plan::Manual(ManualReason::NoBrew), "0.3.0", &fake, LIMIT);
        assert_eq!(outcome, UpdateOutcome::Manual { reason: ManualReason::NoBrew, command: MANUAL_COMMAND, url: RELEASES_PAGE_URL });
        assert!(fake.calls.borrow().is_empty());
        assert_eq!(MANUAL_COMMAND, "brew upgrade --cask binone");
        let text = serde_json::to_string(&outcome).unwrap();
        assert!(text.starts_with(r#"{"state":"manual","reason":"no_brew""#), "{text}");
    }

    // --- The update -------------------------------------------------------------------------

    #[test]
    fn an_update_refreshes_only_the_tap_then_upgrades_and_reads_the_version() {
        let fake = update_script(out(0, "==> Upgrading binone\n🍺  binone was successfully upgraded!"), out(0, "binone 0.3.0\n"));
        let (ok, code, step, output, timed_out, note) = finished(run_update(&brew(), "0.3.0", &fake, LIMIT));
        assert_eq!(
            *fake.calls.borrow(),
            [
                format!("{BREW} --repo unagents/tap"),
                "/usr/bin/xcode-select -p".to_owned(),
                format!("/usr/bin/git -C {TAP_DIR} pull --ff-only"),
                format!("{BREW} upgrade --cask binone"),
                format!("{BREW} list --cask --versions binone"),
            ]
        );
        assert!(ok && !timed_out && note.is_none());
        assert_eq!((code, step.as_str()), (Some(0), "brew upgrade --cask binone"));
        assert!(output.contains("successfully upgraded"));
        // `brew update` is never run.
        assert!(!fake.calls.borrow().iter().any(|c| c.ends_with(" update")));
    }

    #[test]
    fn only_the_upgrade_gets_the_no_auto_update_variable() {
        let fake = update_script(out(0, ""), out(0, "binone 0.3.0"));
        run_update(&brew(), "0.3.0", &fake, LIMIT);
        let envs = fake.envs.borrow();
        assert_eq!(envs[3], [("HOMEBREW_NO_AUTO_UPDATE".to_owned(), "1".to_owned())]);
        assert!(envs[0].is_empty() && envs[1].is_empty() && envs[2].is_empty() && envs[4].is_empty(), "{envs:?}");
        assert_eq!(UPGRADE_ENV, [("HOMEBREW_NO_AUTO_UPDATE", "1")]);
    }

    #[test]
    fn a_tap_that_cannot_be_refreshed_does_not_stop_the_upgrade() {
        // `git pull` fails.
        let fake = Fake::new(vec![out(0, TAP_DIR), out(0, TOOLS_DIR), out(1, "fatal: Not possible to fast-forward"), out(0, ""), out(0, "binone 0.3.0")]);
        let (ok, _, _, _, _, note) = finished(run_update(&brew(), "0.3.0", &fake, LIMIT));
        assert!(ok);
        let note = note.unwrap();
        assert!(note.contains("unagents/tap") && note.contains("終了 code 1"), "{note}");
        assert_eq!(fake.calls.borrow().len(), 5);
        // The tap's directory cannot be had: git is not run.
        for bad in [out(1, "Error: No available tap"), out(0, "  \n"), Err("起動できません".to_owned())] {
            let fake = Fake::new(vec![bad, out(0, ""), out(0, "binone 0.3.0")]);
            let (ok, _, _, _, _, note) = finished(run_update(&brew(), "0.3.0", &fake, LIMIT));
            assert!(ok && note.is_some());
            assert!(!fake.calls.borrow().iter().any(|c| c.contains("git")), "{:?}", fake.calls.borrow());
        }
        // git cannot start, or runs out of time.
        for bad in [Err("git を起動できませんでした".to_owned()), stopped("")] {
            let fake = Fake::new(vec![out(0, TAP_DIR), out(0, TOOLS_DIR), bad, out(0, ""), out(0, "binone 0.3.0")]);
            let (ok, _, _, _, _, note) = finished(run_update(&brew(), "0.3.0", &fake, LIMIT));
            assert!(ok && note.is_some());
        }
        // The note is kept when the upgrade fails too.
        let fake = Fake::new(vec![out(1, ""), out(1, "Error: Cask 'binone' is not installed")]);
        let (ok, code, _, _, _, note) = finished(run_update(&brew(), "0.3.0", &fake, LIMIT));
        assert!(!ok && note.is_some());
        assert_eq!(code, Some(1));
    }

    #[test]
    fn a_pull_that_hangs_is_given_a_minute_and_the_upgrade_still_runs() {
        let fake = Fake::new(vec![out(0, TAP_DIR), out(0, TOOLS_DIR), stopped(""), out(0, ""), out(0, "binone 0.3.0")]);
        let (ok, _, _, _, timed_out, note) = finished(run_update(&brew(), "0.3.0", &fake, LIMIT));
        assert!(ok && !timed_out);
        assert!(note.unwrap().contains("tap の更新が時間切れ"));
        let limits = fake.limits.borrow();
        assert_eq!(limits[2], Duration::from_secs(60));
        assert!(limits[3] > Duration::from_secs(60), "{limits:?}");
        // The whole limit still caps the step.
        let short = Fake::new(vec![out(0, TAP_DIR), out(0, TOOLS_DIR), stopped(""), stopped(""), out(0, "")]);
        run_update(&brew(), "0.3.0", &short, Duration::from_secs(30));
        assert!(short.limits.borrow()[2] <= Duration::from_secs(30));
    }

    #[test]
    fn without_the_command_line_tools_git_is_not_called_and_the_upgrade_runs() {
        for missing in [out(2, "xcode-select: error: unable to get active developer directory"), Err("起動できませんでした".to_owned())] {
            let fake = Fake::new(vec![out(0, TAP_DIR), missing, out(0, ""), out(0, "binone 0.3.0")]);
            let (ok, _, _, _, _, note) = finished(run_update(&brew(), "0.3.0", &fake, LIMIT));
            assert!(ok);
            assert!(note.unwrap().contains("Command Line Tools が無いので tap の更新を省いた"));
            let calls = fake.calls.borrow();
            assert_eq!(calls[1], "/usr/bin/xcode-select -p");
            assert!(!calls.iter().any(|c| c.contains("git")), "{calls:?}");
            assert_eq!(calls.len(), 4);
        }
        // Without the tap's directory, xcode-select is not asked either (brew --repo, xcode-select, git).
        let fake = Fake::new(vec![out(1, ""), out(0, ""), out(0, "binone 0.3.0")]);
        run_update(&brew(), "0.3.0", &fake, LIMIT);
        assert!(!fake.calls.borrow().iter().any(|c| c.contains("xcode-select")));
    }

    #[test]
    fn the_path_of_the_tap_is_a_single_argument() {
        // A directory with blanks or shell characters stays one argument of git (no shell is used).
        let fake = Fake::new(vec![out(0, "warning: x\n/Users/a b/Taps/$(x);y\n"), out(0, TOOLS_DIR), out(0, ""), out(0, ""), out(0, "binone 0.3.0")]);
        run_update(&brew(), "0.3.0", &fake, LIMIT);
        assert_eq!(fake.calls.borrow()[2], "/usr/bin/git -C /Users/a b/Taps/$(x);y pull --ff-only");
    }

    #[test]
    fn a_failing_upgrade_stops_with_its_code_and_output() {
        let fake = update_script(out(1, "Error: Cask 'binone' is not installed"), out(0, ""));
        let outcome = run_update(&brew(), "0.3.0", &fake, LIMIT);
        // No look at the version after a failure.
        assert_eq!(fake.calls.borrow().len(), 4);
        assert_eq!(
            outcome,
            UpdateOutcome::Finished { ok: false, code: Some(1), step: "brew upgrade --cask binone".into(), output: "Error: Cask 'binone' is not installed".into(), timed_out: false, note: None }
        );

        let killed = update_script(Ok(RunOutput { code: None, text: String::new(), timed_out: false }), out(0, ""));
        let (ok, code, ..) = finished(run_update(&brew(), "0.3.0", &killed, LIMIT));
        assert!(!ok && code.is_none());

        let missing = update_script(Err("/opt/homebrew/bin/brew を起動できませんでした（NotFound）".into()), out(0, ""));
        let (ok, _, _, output, ..) = finished(run_update(&brew(), "0.3.0", &missing, LIMIT));
        assert!(!ok && output.contains("NotFound"));
    }

    #[test]
    fn an_upgrade_out_of_time_ends_the_update_as_timed_out_with_the_output_so_far() {
        let fake = update_script(stopped("==> Downloading\nstill going"), out(0, ""));
        let (ok, code, step, output, timed_out, _) = finished(run_update(&brew(), "0.3.0", &fake, LIMIT));
        assert!(!ok && timed_out && code.is_none());
        assert_eq!(step, "brew upgrade --cask binone");
        assert!(output.contains("still going"));
        assert_eq!(fake.calls.borrow().len(), 4);
        let text = serde_json::to_string(&run_update(&brew(), "0.3.0", &update_script(stopped(""), out(0, "")), LIMIT)).unwrap();
        assert!(text.contains(r#""timed_out":true"#), "{text}");
    }

    #[test]
    fn the_installed_version_must_be_the_latest_for_the_update_to_count() {
        // The same: done.
        let same = update_script(out(0, "upgraded"), out(0, "binone 0.3.0"));
        assert!(finished(run_update(&brew(), "v0.3.0", &same, LIMIT)).0);
        // Another version of the same cask kept beside it does not hide that the latest is there.
        let several = update_script(out(0, "upgraded"), out(0, "binone 0.2.0 0.3.0"));
        assert!(finished(run_update(&brew(), "0.3.0", &several, LIMIT)).0);
        // A different one (brew ended with 0 and did nothing): not done, with both versions in the end of the output.
        let stale = update_script(out(0, "Warning: binone 0.2.0 already installed"), out(0, "binone 0.2.0\n"));
        let (ok, code, _, output, timed_out, _) = finished(run_update(&brew(), "0.3.0", &stale, LIMIT));
        assert!(!ok && !timed_out);
        assert_eq!(code, Some(0));
        assert!(output.starts_with("Warning: binone 0.2.0 already installed\n"), "{output}");
        assert!(output.ends_with("更新されませんでした（入っている版 0.2.0、最新 0.3.0）"), "{output}");
        // A newer one than the latest is also "not the same".
        let newer = update_script(out(0, ""), out(0, "binone 0.4.0"));
        let (ok, _, _, output, ..) = finished(run_update(&brew(), "0.3.0", &newer, LIMIT));
        assert!(!ok && output.ends_with("更新されませんでした（入っている版 0.4.0、最新 0.3.0）"), "{output}");
    }

    #[test]
    fn a_version_that_cannot_be_read_is_not_reported_as_done() {
        // Not done, and said so in other words than "not updated": it is not known.
        let cases = [
            update_script(out(0, "upgraded"), out(0, "")),
            update_script(out(0, "upgraded"), out(0, "binone")),
            update_script(out(0, "upgraded"), out(0, "binone nightly")),
            update_script(out(0, "upgraded"), out(0, "other 0.3.0")),
            update_script(out(0, "upgraded"), out(1, "Error: Cask 'binone' is not installed.")),
            update_script(out(0, "upgraded"), stopped("")),
            update_script(out(0, "upgraded"), Err("x".into())),
        ];
        for fake in cases {
            let (ok, code, _, output, timed_out, _) = finished(run_update(&brew(), "0.3.0", &fake, LIMIT));
            assert!(!ok && !timed_out && code == Some(0));
            assert!(output.ends_with("更新できたか確認できませんでした（入っている版を読めません、最新 0.3.0）"), "{output}");
        }
        // The latest version is not known either: nothing to compare with.
        let fake = update_script(out(0, "upgraded"), out(0, "binone 0.3.0"));
        let (ok, _, _, output, ..) = finished(run_update(&brew(), "", &fake, LIMIT));
        assert!(!ok && output.ends_with("更新できたか確認できませんでした（最新の版が分かりません、入っている版 0.3.0）"), "{output}");
    }

    #[test]
    fn the_steps_share_one_time_limit() {
        let fake = update_script(out(0, ""), out(0, "binone 0.3.0"));
        run_update(&brew(), "0.3.0", &fake, LIMIT);
        let limits = fake.limits.borrow();
        // brew --repo, xcode-select, git pull, the upgrade, the version.
        assert_eq!(limits.len(), 5);
        assert!(limits[0] <= LIMIT && limits[0] > LIMIT - Duration::from_secs(5));
        // What is left of the whole limit only shrinks; xcode-select and git have caps of their own.
        assert!(limits[3] <= limits[0] && limits[4] <= limits[3], "{limits:?}");
        assert!(limits[1] <= LIST_TIME_LIMIT && limits[2] <= TAP_PULL_TIME_LIMIT, "{limits:?}");
        // A limit already used up leaves nothing for the next step.
        let fake = update_script(out(0, ""), out(0, ""));
        run_update(&brew(), "0.3.0", &fake, Duration::ZERO);
        assert_eq!(*fake.limits.borrow(), [Duration::ZERO; 5]);
    }

    #[test]
    fn the_judgment_shares_the_one_limit_with_the_update() {
        // Two brews are asked, then the tap, the upgrade and the version.
        let mut script = vec![out(0, "binone"), out(0, "binone")];
        script.extend(tap_ok());
        script.extend([out(0, ""), out(0, "binone 0.3.0")]);
        let fake = Fake::new(script);
        let outcome = detect_and_update(Some(Path::new(BUNDLE)), "0.3.0", &|_| true, &fake, LIMIT);
        assert!(matches!(outcome, UpdateOutcome::Finished { ok: true, .. }), "{outcome:?}");
        let limits = fake.limits.borrow();
        assert_eq!((limits[0], limits[1]), (LIST_TIME_LIMIT, LIST_TIME_LIMIT));
        // brew --repo, then the upgrade and the version (xcode-select and git have caps of their own).
        assert!(limits[2] <= LIMIT && limits[5] <= limits[2] && limits[6] <= limits[5], "{limits:?}");
        // A limit shorter than the list limit caps the list calls too.
        let short = Fake::new(vec![stopped(""), stopped("")]);
        detect_and_update(Some(Path::new(BUNDLE)), "0.3.0", &|_| true, &short, Duration::from_secs(3));
        assert_eq!(*short.limits.borrow(), [Duration::from_secs(3); 2]);
    }

    #[test]
    fn a_judgment_that_used_the_time_leaves_less_for_the_steps() {
        struct Slow(Fake);
        impl Runner for Slow {
            fn run(&self, program: &Path, args: &[&str], env: &[(&str, &str)], limit: Duration) -> Result<RunOutput, String> {
                std::thread::sleep(Duration::from_millis(200));
                self.0.run(program, args, env, limit)
            }
        }
        let mut script = vec![out(0, "binone")];
        script.extend(tap_ok());
        script.extend([out(0, ""), out(0, "binone 0.3.0")]);
        let slow = Slow(Fake::new(script));
        detect_and_update(Some(Path::new(BUNDLE)), "0.3.0", &|p| p == Path::new(BREW), &slow, Duration::from_secs(60));
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
            ("success", Box::new(|| run_update(&brew(), "0.3.0", &update_script(out(0, "ok"), out(0, "binone 0.3.0")), LIMIT))),
            ("failure", Box::new(|| run_update(&brew(), "0.3.0", &update_script(out(1, "Error"), out(0, "")), LIMIT))),
            ("timeout", Box::new(|| run_update(&brew(), "0.3.0", &update_script(stopped("slow"), out(0, "")), LIMIT))),
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
            let answer = SystemRunner.run(Path::new("/bin/sh"), &["-c", script], &[], limit);
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
        fn a_child_left_in_the_group_when_the_program_ends_first_does_not_outlive_the_time_out() {
            // The program ends on TERM at once; its child ignores TERM and stays in the group until the KILL.
            let (answer, _) = sh("(trap '' TERM; exec sleep 300) & echo child=$!; wait", Duration::from_millis(300));
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
        fn the_given_variables_are_added_to_what_the_program_inherits() {
            let answer = SystemRunner.run(Path::new("/bin/sh"), &["-c", "echo $BINONE_TEST_ADDED:$PATH"], &[("BINONE_TEST_ADDED", "1")], Duration::from_secs(30));
            let text = answer.unwrap().text;
            assert!(text.starts_with("1:") && text.trim_end().len() > 3, "{text:?}");
        }

        #[test]
        fn a_program_that_cannot_start_is_an_error() {
            let answer = SystemRunner.run(Path::new("/nonexistent/brew"), &[], &[], Duration::from_secs(1));
            assert!(answer.unwrap_err().contains("起動できませんでした"));
        }
    }
}
