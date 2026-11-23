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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
    Available { current: String, latest: String },
    Unknown { current: String, reason: String },
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

/// Compares the running version with the latest one. `fetch` gets the answer of GitHub's API.
pub async fn check<F, Fut>(current: &str, fetch: F) -> UpdateCheck
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<HttpAnswer, CheckFailure>>,
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
        UpdateCheck::Available { current, latest }
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
    /// Standard output, then standard error.
    pub text: String,
}

/// Runs a program with its arguments (never through a shell).
pub trait Runner {
    fn run(&self, program: &Path, args: &[&str]) -> Result<RunOutput, String>;
}

pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &Path, args: &[&str]) -> Result<RunOutput, String> {
        let output = std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("could not start {}: {}", program.display(), e.kind()))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok(RunOutput { code: output.status.code(), text })
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
pub fn plan(bundled: bool, exists: &dyn Fn(&Path) -> bool, runner: &dyn Runner) -> Plan {
    let Some(brew) = find_brew(exists) else {
        return Plan::Manual(ManualReason::NoBrew);
    };
    let installed = bundled && matches!(runner.run(&brew, &["list", "--cask", CASK]), Ok(RunOutput { code: Some(0), .. }));
    if installed {
        Plan::Brew(brew)
    } else {
        Plan::Manual(ManualReason::NotInstalledByCask)
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
    Finished { ok: bool, code: Option<i32>, step: String, output: String },
}

/// The last lines of `text`, limited in size.
pub fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let joined = lines[lines.len().saturating_sub(OUTPUT_TAIL_LINES)..].join("\n");
    let skip = joined.chars().count().saturating_sub(OUTPUT_TAIL_CHARS);
    joined.chars().skip(skip).collect()
}

/// Runs the update as planned.
pub fn run_update(plan: &Plan, runner: &dyn Runner) -> UpdateOutcome {
    let brew = match plan {
        Plan::Manual(reason) => return UpdateOutcome::Manual { reason: *reason, command: MANUAL_COMMAND, url: RELEASES_PAGE_URL },
        Plan::Brew(brew) => brew,
    };
    let mut last = UpdateOutcome::Busy;
    for args in update_steps() {
        let step = format!("brew {}", args.join(" "));
        last = match runner.run(brew, &args) {
            Err(text) => return UpdateOutcome::Finished { ok: false, code: None, step, output: text },
            Ok(RunOutput { code, text }) if code != Some(0) => {
                return UpdateOutcome::Finished { ok: false, code, step, output: tail(&text) }
            }
            Ok(RunOutput { code, text }) => UpdateOutcome::Finished { ok: true, code, step, output: tail(&text) },
        };
    }
    last
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

impl Drop for UpdateRunning<'_> {
    fn drop(&mut self) {
        self.0 .0.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::testing::run;
    use std::cell::RefCell;

    fn answer(status: u16, body: &str) -> HttpAnswer {
        HttpAnswer { status, body: body.to_owned() }
    }

    fn release(tag: &str) -> HttpAnswer {
        answer(200, &format!(r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"name":"x"}}"#))
    }

    fn checked(current: &str, result: Result<HttpAnswer, CheckFailure>) -> UpdateCheck {
        run(check(current, || async { result }))
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
        assert_eq!(check, UpdateCheck::Available { current: "0.2.0".into(), latest: "0.10.0".into() });
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
    fn the_check_serializes_with_a_state() {
        let text = serde_json::to_string(&checked("0.2.0", Ok(release("v0.3.0")))).unwrap();
        assert_eq!(text, r#"{"state":"available","current":"0.2.0","latest":"0.3.0"}"#);
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
    }

    impl Fake {
        fn new(script: Vec<Result<RunOutput, String>>) -> Self {
            Fake { script: RefCell::new(script), calls: RefCell::new(Vec::new()) }
        }
    }

    impl Runner for Fake {
        fn run(&self, program: &Path, args: &[&str]) -> Result<RunOutput, String> {
            self.calls.borrow_mut().push(format!("{} {}", program.display(), args.join(" ")));
            self.script.borrow_mut().remove(0)
        }
    }

    fn out(code: i32, text: &str) -> Result<RunOutput, String> {
        Ok(RunOutput { code: Some(code), text: text.to_owned() })
    }

    const BREW: &str = "/opt/homebrew/bin/brew";

    #[test]
    fn with_brew_and_the_cask_the_plan_is_brew() {
        let fake = Fake::new(vec![out(0, "binone")]);
        assert_eq!(plan(true, &|_| true, &fake), Plan::Brew(PathBuf::from(BREW)));
        assert_eq!(*fake.calls.borrow(), [format!("{BREW} list --cask binone")]);
    }

    #[test]
    fn without_brew_nothing_is_run() {
        let fake = Fake::new(vec![]);
        assert_eq!(plan(true, &|_| false, &fake), Plan::Manual(ManualReason::NoBrew));
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn without_the_cask_or_outside_a_bundle_the_plan_is_manual() {
        let not_installed = Fake::new(vec![out(1, "Error: Cask 'binone' is not installed.")]);
        assert_eq!(plan(true, &|_| true, &not_installed), Plan::Manual(ManualReason::NotInstalledByCask));
        let cannot_start = Fake::new(vec![Err("could not start".into())]);
        assert_eq!(plan(true, &|_| true, &cannot_start), Plan::Manual(ManualReason::NotInstalledByCask));
        // A development build is not the cask's app, even when the cask is installed.
        let dev = Fake::new(vec![]);
        assert_eq!(plan(false, &|_| true, &dev), Plan::Manual(ManualReason::NotInstalledByCask));
        assert!(dev.calls.borrow().is_empty());
    }

    #[test]
    fn a_manual_plan_runs_nothing_and_gives_the_page_and_the_command() {
        let fake = Fake::new(vec![]);
        let outcome = run_update(&Plan::Manual(ManualReason::NoBrew), &fake);
        assert_eq!(outcome, UpdateOutcome::Manual { reason: ManualReason::NoBrew, command: MANUAL_COMMAND, url: RELEASES_PAGE_URL });
        assert!(fake.calls.borrow().is_empty());
        assert_eq!(MANUAL_COMMAND, "brew upgrade --cask binone");
        let text = serde_json::to_string(&outcome).unwrap();
        assert!(text.starts_with(r#"{"state":"manual","reason":"no_brew""#), "{text}");
    }

    #[test]
    fn an_update_runs_both_steps_in_order() {
        let fake = Fake::new(vec![out(0, "Updated 1 tap"), out(0, "==> Upgrading binone\n🍺  binone was successfully upgraded!")]);
        let outcome = run_update(&Plan::Brew(PathBuf::from(BREW)), &fake);
        assert_eq!(*fake.calls.borrow(), [format!("{BREW} update"), format!("{BREW} upgrade --cask binone")]);
        let UpdateOutcome::Finished { ok, code, step, output } = outcome else { panic!() };
        assert!(ok);
        assert_eq!(code, Some(0));
        assert_eq!(step, "brew upgrade --cask binone");
        assert!(output.contains("successfully upgraded"));
    }

    #[test]
    fn a_failing_step_stops_the_update_with_its_code_and_output() {
        let fake = Fake::new(vec![out(1, "Error: could not update")]);
        let outcome = run_update(&Plan::Brew(PathBuf::from(BREW)), &fake);
        assert_eq!(fake.calls.borrow().len(), 1);
        assert_eq!(outcome, UpdateOutcome::Finished { ok: false, code: Some(1), step: "brew update".into(), output: "Error: could not update".into() });

        let fake = Fake::new(vec![out(0, ""), out(1, "Error: Cask 'binone' is not installed")]);
        let UpdateOutcome::Finished { ok, code, step, .. } = run_update(&Plan::Brew(PathBuf::from(BREW)), &fake) else { panic!() };
        assert!(!ok);
        assert_eq!((code, step.as_str()), (Some(1), "brew upgrade --cask binone"));

        let killed = Fake::new(vec![Ok(RunOutput { code: None, text: String::new() })]);
        let UpdateOutcome::Finished { ok, code, .. } = run_update(&Plan::Brew(PathBuf::from(BREW)), &killed) else { panic!() };
        assert!(!ok && code.is_none());

        let missing = Fake::new(vec![Err("could not start brew: NotFound".into())]);
        let UpdateOutcome::Finished { ok, output, .. } = run_update(&Plan::Brew(PathBuf::from(BREW)), &missing) else { panic!() };
        assert!(!ok && output.contains("NotFound"));
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
}
