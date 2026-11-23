//! The request-writing assistant: a conversation held by the user's own `claude` CLI (Claude Code).
//!
//! One answer is one `claude -p` run. The first answer's session id is kept by the screen and sent
//! back, and the next run continues it with `--resume`. The run has every tool switched off and
//! starts in an empty directory of this app's own, so the assistant can neither read nor write the
//! user's files nor run a command. The request being written is handed over as text each time; what
//! the assistant wants changed comes back as a title and/or a body, and the screen applies it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Environment variable that names the `claude` program to run (first choice; for tests and checks).
pub const BIN_ENV: &str = "BINONE_CLAUDE_BIN";
/// One answer may take this long.
pub const ANSWER_LIMIT: Duration = Duration::from_secs(120);
/// Looking for `claude` through the login shell may take this long.
pub const LOOKUP_LIMIT: Duration = Duration::from_secs(10);
/// How much of the CLI's output is shown with a failure.
const DETAIL_CHARS: usize = 400;

/// Where `claude` is usually installed, in the order of preference. `~` is the home directory.
const CANDIDATES: [&str; 4] = ["/opt/homebrew/bin/claude", "/usr/local/bin/claude", "~/.local/bin/claude", "~/.claude/local/claude"];

/// The system prompt: what the assistant is, what it may do, and the shape of every answer.
pub const SYSTEM_PROMPT: &str = "\
あなたは「依頼作成支援エージェント」です。利用者が書いている作業依頼（題名と本文）を、会話で一緒に整えます。\n\
- 利用者の発言のたびに、今編集中の依頼（題名・本文・プロジェクト・種別）が <request> の中に付きます。それを前提に答えてください。\n\
- あなたにできる事は、返事を書く事と、依頼の題名・本文の新しい値を提案する事だけです。file・command・外部の情報は使えません。\n\
- 題名や本文を書き換えるのは、利用者がそう頼んだ時だけです。頼まれていなければ書き換えないでください。\n\
- 本文を書き換える時は、直す所だけでなく本文の全体を新しい値として返してください（Markdown）。\n\
- 答えは、次の形の JSON オブジェクト 1 つだけにしてください。前後に説明や ``` を付けないでください。\n\
{\"reply\": \"利用者への返事（日本語）\", \"title\": \"題名の新しい値\", \"body\": \"本文の新しい値\"}\n\
- reply は必ず入れます。title と body は、書き換える時だけ入れます（書き換えない方のキーは付けません）。";

/// The request being written, as the user has it on the screen right now.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Draft {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub kind: String,
}

/// What the screen is told.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Outcome {
    /// An answer. `title` / `body` are new values to put in the form, when the assistant gave them.
    Answer {
        reply: String,
        title: Option<String>,
        body: Option<String>,
        session_id: Option<String>,
    },
    /// No answer: why, in Japanese, and the CLI's own words when there are any.
    Notice { kind: NoticeKind, message: String, detail: Option<String> },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    NotFound,
    NotLoggedIn,
    Failed,
    TimedOut,
}

fn notice(kind: NoticeKind, message: impl Into<String>, detail: Option<String>) -> Outcome {
    Outcome::Notice { kind, message: message.into(), detail: detail.filter(|d| !d.is_empty()) }
}

// ---------------------------------------------------------------------------------------------
// Finding the CLI

/// `claude`'s place: `BINONE_CLAUDE_BIN` if set (used as it is; if it is not a file, not found),
/// else the usual places, else what a login shell finds (a GUI app has no login shell's PATH).
pub fn find_claude(
    env: &dyn Fn(&str) -> Option<String>,
    home: Option<&Path>,
    is_file: &dyn Fn(&Path) -> bool,
    login_shell: &dyn Fn() -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(set) = env(BIN_ENV).filter(|v| !v.trim().is_empty()) {
        let path = PathBuf::from(set.trim());
        return is_file(&path).then_some(path);
    }
    for candidate in CANDIDATES {
        let path = match candidate.strip_prefix("~/") {
            Some(rest) => match home {
                Some(home) => home.join(rest),
                None => continue,
            },
            None => PathBuf::from(candidate),
        };
        if is_file(&path) {
            return Some(path);
        }
    }
    login_shell().filter(|p| is_file(p))
}

/// `/bin/zsh -lc 'command -v claude'`: the path it prints, if it prints one.
pub fn login_shell_lookup() -> Option<PathBuf> {
    let out = run(Path::new("/bin/zsh"), &["-lc", "command -v claude"], None, &[], None, LOOKUP_LIMIT).ok()?;
    if out.timed_out || out.code != Some(0) {
        return None;
    }
    // Startup files may print something first; the answer is the last line that is a path.
    out.stdout.lines().rev().map(str::trim).find(|l| l.starts_with('/')).map(PathBuf::from)
}

pub fn not_found() -> Outcome {
    notice(
        NoticeKind::NotFound,
        "claude の CLI（Claude Code）が見つかりません。Claude Code を入れて（https://claude.com/claude-code）、ターミナルで claude を一度起動して login してから、もう一度送ってください。",
        None,
    )
}

// ---------------------------------------------------------------------------------------------
// Running it

#[derive(Debug)]
pub struct RunOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Runs a program to its end, or kills it (and what it started) at the limit. `input` goes to its
/// standard input, which is then closed.
pub fn run(program: &Path, args: &[&str], cwd: Option<&Path>, env: &[(&str, &str)], input: Option<&str>, limit: Duration) -> Result<RunOutput, String> {
    let mut command = Command::new(program);
    command.args(args).envs(env.iter().copied()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().map_err(|e| format!("{} を起動できませんでした（{}）", program.display(), e.kind()))?;

    // The input is written from its own thread so that a program that does not read cannot hold us.
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.unwrap_or("").to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    // Each stream is read in pieces from its own thread, so that what was written before a stop is kept.
    let (tx, rx) = mpsc::channel::<(bool, Option<Vec<u8>>)>();
    for (is_out, stream) in [
        (true, child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>)),
        (false, child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)),
    ] {
        let Some(mut stream) = stream else { continue };
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 || tx.send((is_out, Some(buf[..n].to_vec()))).is_err() {
                    break;
                }
            }
            let _ = tx.send((is_out, None));
        });
    }
    drop(tx);

    // Both streams reach their end when the program and what it started are gone; that is the wait.
    let mut out = Vec::new();
    let mut err = Vec::new();
    let deadline = std::time::Instant::now() + limit;
    let mut open = 2;
    let mut timed_out = false;
    while open > 0 {
        match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
            Ok((_, None)) => open -= 1,
            Ok((is_out, Some(piece))) => {
                if is_out { out.extend(piece) } else { err.extend(piece) }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                timed_out = true;
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if timed_out {
        kill_group(&mut child);
        // What was written before the stop is still worth showing.
        while let Ok((is_out, piece)) = rx.recv_timeout(Duration::from_millis(200)) {
            match piece {
                Some(piece) if is_out => out.extend(piece),
                Some(piece) => err.extend(piece),
                None => {}
            }
        }
    }
    let code = if timed_out { None } else { child.wait().ok().and_then(|s| s.code()) };
    if timed_out {
        let _ = child.wait();
    }
    Ok(RunOutput { code, stdout: String::from_utf8_lossy(&out).into_owned(), stderr: String::from_utf8_lossy(&err).into_owned(), timed_out })
}

fn kill_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The group was made at the start (process_group), so one signal reaches what it started too.
        let _ = Command::new("kill").args(["-KILL", &format!("-{}", child.id())]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
}

/// The arguments of one run. The tools are all switched off (`--tools ""`); no settings, slash
/// commands, skills or MCP servers are read (`--setting-sources ""`, `--disable-slash-commands`,
/// `--strict-mcp-config`); the system prompt is ours. The message itself goes through standard input.
pub fn build_args(session_id: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = ["-p", "--output-format", "json", "--tools", "", "--setting-sources", "", "--disable-slash-commands", "--strict-mcp-config", "--system-prompt"]
        .into_iter()
        .map(String::from)
        .collect();
    args.push(SYSTEM_PROMPT.to_owned());
    if let Some(id) = session_id.filter(|s| !s.is_empty()) {
        args.push("--resume".into());
        args.push(id.to_owned());
    }
    args
}

/// What goes to the assistant for one turn: the request as it is now, then what the user said.
pub fn build_prompt(draft: &Draft, message: &str) -> String {
    format!(
        "<request>\nproject: {}\nkind: {}\ntitle: {}\nbody:\n{}\n</request>\n\n{}",
        draft.project, draft.kind, draft.title, draft.body, message
    )
}

/// One answer: runs `claude` once and turns what came back into an outcome. Never fails; every
/// problem is a notice.
pub fn ask(claude: &Path, cwd: &Path, draft: &Draft, message: &str, session_id: Option<&str>, limit: Duration) -> Outcome {
    if let Err(e) = std::fs::create_dir_all(cwd) {
        return notice(NoticeKind::Failed, "会話用の作業場所を作れませんでした。", Some(e.kind().to_string()));
    }
    let args = build_args(session_id);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = match run(claude, &args, Some(cwd), &[], Some(&build_prompt(draft, message)), limit) {
        Ok(o) => o,
        Err(e) => return notice(NoticeKind::Failed, "claude を起動できませんでした。", Some(e)),
    };
    interpret(&output, limit)
}

pub fn interpret(output: &RunOutput, limit: Duration) -> Outcome {
    if output.timed_out {
        return notice(
            NoticeKind::TimedOut,
            format!("返事が {} 秒たっても返らなかったので、待つのをやめました。もう一度送ってください。", limit.as_secs()),
            summary(&format!("{}{}", output.stdout, output.stderr)),
        );
    }
    let envelope: Option<serde_json::Value> = serde_json::from_str(output.stdout.trim()).ok();
    let result_text = envelope.as_ref().and_then(|v| v.get("result")).and_then(|v| v.as_str());
    let is_error = envelope.as_ref().and_then(|v| v.get("is_error")).and_then(|v| v.as_bool()).unwrap_or(false);

    if output.code != Some(0) || is_error {
        // The CLI's own words: its result when it gave one, else what it wrote to the error output.
        let words = result_text.filter(|t| !t.trim().is_empty()).map(str::to_owned).unwrap_or_else(|| format!("{}{}", output.stderr, output.stdout));
        let lowered = words.to_lowercase();
        if lowered.contains("not logged in") || lowered.contains("/login") || lowered.contains("authentication") || lowered.contains("api key") {
            return notice(
                NoticeKind::NotLoggedIn,
                "claude にログインしていません。ターミナルで claude を起動して /login を実行してから、もう一度送ってください。",
                summary(&words),
            );
        }
        let message = match output.code {
            Some(code) if code != 0 => format!("claude がエラーで終わりました（終了 code {code}）。"),
            _ => "claude がエラーを返しました。".to_owned(),
        };
        return notice(NoticeKind::Failed, message, summary(&words));
    }

    let Some(text) = result_text else {
        // Exit 0 but not the shape expected: show what there is rather than nothing.
        let raw = output.stdout.trim();
        return if raw.is_empty() {
            notice(NoticeKind::Failed, "claude から返事が返りませんでした。", summary(&output.stderr))
        } else {
            Outcome::Answer { reply: raw.to_owned(), title: None, body: None, session_id: None }
        };
    };
    let session_id = envelope.as_ref().and_then(|v| v.get("session_id")).and_then(|v| v.as_str()).map(str::to_owned);
    parse_answer(text, session_id)
}

/// The answer's own shape (`reply`, and `title` / `body` when it rewrites). A shape that is not that
/// is shown as the reply, as it is.
pub fn parse_answer(text: &str, session_id: Option<String>) -> Outcome {
    #[derive(Deserialize)]
    struct Shape {
        reply: String,
        title: Option<String>,
        body: Option<String>,
    }
    let stripped = strip_fence(text.trim());
    let shaped = serde_json::from_str::<Shape>(stripped).ok().or_else(|| {
        // Words around the object: take from the first `{` to the last `}`.
        let (start, end) = (stripped.find('{')?, stripped.rfind('}')?);
        serde_json::from_str::<Shape>(stripped.get(start..=end)?).ok()
    });
    match shaped {
        Some(s) => Outcome::Answer { reply: s.reply, title: s.title, body: s.body, session_id },
        None => Outcome::Answer { reply: text.trim().to_owned(), title: None, body: None, session_id },
    }
}

fn strip_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else { return text };
    let rest = rest.split_once('\n').map_or(rest, |(_, r)| r);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

/// The last part of the CLI's words, short enough for one line of the conversation.
fn summary(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    Some(if chars.len() <= DETAIL_CHARS { text.to_owned() } else { format!("…{}", chars[chars.len() - DETAIL_CHARS..].iter().collect::<String>()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A directory of this test's own under the build's temporary place.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("binone-assist-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
        /// A fake `claude`: a shell script with this body.
        fn script(&self, body: &str) -> PathBuf {
            let path = self.0.join("claude");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn draft() -> Draft {
        Draft { title: "題名".into(), body: "本文".into(), project: "p".into(), kind: "task".into() }
    }
    fn ask_with(script: &str, session: Option<&str>, limit: Duration) -> Outcome {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let s = Scratch::new(&format!("run{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let bin = s.script(script);
        ask(&bin, &s.0.join("cwd"), &draft(), "直して", session, limit)
    }
    /// What a CLI prints for one answer: the JSON envelope with `result` holding the given text.
    fn envelope(result: &str) -> String {
        serde_json::json!({"type": "result", "is_error": false, "session_id": "sess-1", "result": result}).to_string()
    }
    fn echo(json: &str) -> String {
        format!("cat >/dev/null; printf '%s' '{}'", json.replace('\'', "'\\''"))
    }

    #[test]
    fn a_plain_answer() {
        let out = ask_with(&echo(&envelope(r#"{"reply":"了解です"}"#)), None, Duration::from_secs(10));
        assert_eq!(out, Outcome::Answer { reply: "了解です".into(), title: None, body: None, session_id: Some("sess-1".into()) });
    }

    #[test]
    fn a_title_only_rewrite() {
        let out = ask_with(&echo(&envelope(r#"{"reply":"題名を直しました","title":"新しい題名"}"#)), None, Duration::from_secs(10));
        assert_eq!(out, Outcome::Answer { reply: "題名を直しました".into(), title: Some("新しい題名".into()), body: None, session_id: Some("sess-1".into()) });
    }

    #[test]
    fn a_body_rewrite_even_inside_a_fence() {
        let out = ask_with(&echo(&envelope("```json\n{\"reply\":\"r\",\"title\":\"t\",\"body\":\"# 見出し\\n本文\"}\n```")), None, Duration::from_secs(10));
        assert_eq!(out, Outcome::Answer { reply: "r".into(), title: Some("t".into()), body: Some("# 見出し\n本文".into()), session_id: Some("sess-1".into()) });
    }

    #[test]
    fn a_broken_shape_is_shown_as_the_reply() {
        let out = ask_with(&echo(&envelope("ただの文です。{壊れた")), None, Duration::from_secs(10));
        assert_eq!(out, Outcome::Answer { reply: "ただの文です。{壊れた".into(), title: None, body: None, session_id: Some("sess-1".into()) });
    }

    #[test]
    fn a_non_zero_exit_is_a_notice_with_the_cli_words() {
        let out = ask_with("cat >/dev/null; echo 'something broke' >&2; exit 3", None, Duration::from_secs(10));
        let Outcome::Notice { kind, message, detail } = out else { panic!("expected a notice") };
        assert_eq!(kind, NoticeKind::Failed);
        assert!(message.contains("終了 code 3"), "{message}");
        assert_eq!(detail.as_deref(), Some("something broke"));
    }

    #[test]
    fn not_logged_in_is_named() {
        let json = r#"{"type":"result","is_error":true,"result":"Not logged in · Please run /login"}"#;
        let out = ask_with(&format!("cat >/dev/null; printf '%s' '{json}'; exit 1"), None, Duration::from_secs(10));
        let Outcome::Notice { kind, message, detail } = out else { panic!("expected a notice") };
        assert_eq!(kind, NoticeKind::NotLoggedIn);
        assert!(message.contains("ログインしていません"));
        assert_eq!(detail.as_deref(), Some("Not logged in · Please run /login"));
    }

    #[test]
    fn an_error_envelope_with_exit_zero_is_still_a_failure() {
        let json = r#"{"is_error":true,"result":"API Error: 500"}"#;
        let out = ask_with(&format!("cat >/dev/null; printf '%s' '{json}'"), None, Duration::from_secs(10));
        let Outcome::Notice { kind, detail, .. } = out else { panic!("expected a notice") };
        assert_eq!(kind, NoticeKind::Failed);
        assert_eq!(detail.as_deref(), Some("API Error: 500"));
    }

    #[test]
    fn a_run_past_the_limit_is_stopped() {
        let started = std::time::Instant::now();
        let out = ask_with("cat >/dev/null; echo partial; sleep 30", None, Duration::from_millis(500));
        let Outcome::Notice { kind, detail, .. } = out else { panic!("expected a notice") };
        assert_eq!(kind, NoticeKind::TimedOut);
        assert_eq!(detail.as_deref(), Some("partial"));
        assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    }

    #[test]
    fn nothing_found_is_the_guidance() {
        let found = find_claude(&|_| Some("/no/such/claude".into()), None, &|p| p.is_file(), &|| None);
        assert_eq!(found, None);
        let Outcome::Notice { kind, message, .. } = not_found() else { panic!("expected a notice") };
        assert_eq!(kind, NoticeKind::NotFound);
        assert!(message.starts_with("claude の CLI（Claude Code）が見つかりません。"));
    }

    #[test]
    fn the_run_is_given_what_it_needs_and_nothing_else() {
        // The fake records its arguments, its directory and what it was sent.
        let s = Scratch::new("record");
        let log = s.0.join("log");
        let bin = s.script(&format!("pwd >> '{0}'; for a in \"$@\"; do printf '[%s]' \"$a\"; done >> '{0}'; echo >> '{0}'; cat >> '{0}'; printf '%s' '{1}'", log.display(), envelope(r#"{"reply":"ok"}"#)));
        let cwd = s.0.join("empty");
        let first = ask(&bin, &cwd, &draft(), "題名を短く", None, Duration::from_secs(10));
        assert!(matches!(first, Outcome::Answer { .. }));
        let second = ask(&bin, &cwd, &draft(), "次", Some("sess-1"), Duration::from_secs(10));
        assert!(matches!(second, Outcome::Answer { .. }));
        let logged = std::fs::read_to_string(&log).unwrap();
        // The second run was logged after the first, appended; check each part in the whole text.
        assert!(logged.contains(cwd.file_name().unwrap().to_str().unwrap()), "{logged}");
        assert!(logged.contains("[-p][--output-format][json][--tools][][--setting-sources][]"), "{logged}");
        assert!(logged.contains("[--resume][sess-1]"), "{logged}");
        assert_eq!(logged.matches("[--resume]").count(), 1, "only the second run resumes");
        assert!(logged.contains("title: 題名\nbody:\n本文"), "the request goes along");
        assert!(logged.contains("題名を短く"), "the message goes along");
    }

    #[test]
    fn finding_goes_env_then_places_then_login_shell() {
        let has = |set: &'static [&'static str]| move |p: &Path| set.iter().any(|s| Path::new(s) == p);
        let none = || None;
        let env_set = |_: &str| Some("/x/claude".to_owned());
        let env_unset = |_: &str| None;
        let home = Path::new("/home/u");
        // The variable wins, even over places that have one.
        let all = has(&["/x/claude", "/opt/homebrew/bin/claude"]);
        assert_eq!(find_claude(&env_set, Some(home), &all, &none), Some("/x/claude".into()));
        // Places are tried in order.
        let all = has(&["/usr/local/bin/claude", "/home/u/.local/bin/claude", "/home/u/.claude/local/claude"]);
        assert_eq!(find_claude(&env_unset, Some(home), &all, &none), Some("/usr/local/bin/claude".into()));
        let all = has(&["/home/u/.claude/local/claude", "/home/u/.local/bin/claude"]);
        assert_eq!(find_claude(&env_unset, Some(home), &all, &none), Some("/home/u/.local/bin/claude".into()));
        let all = has(&["/home/u/.claude/local/claude"]);
        assert_eq!(find_claude(&env_unset, Some(home), &all, &none), Some("/home/u/.claude/local/claude".into()));
        // Then the login shell; and nothing.
        let shell = || Some(PathBuf::from("/shell/claude"));
        assert_eq!(find_claude(&env_unset, Some(home), &has(&["/shell/claude"]), &shell), Some("/shell/claude".into()));
        assert_eq!(find_claude(&env_unset, Some(home), &has(&[]), &shell), None);
        // An empty variable is as good as unset.
        assert_eq!(find_claude(&|_| Some("  ".into()), Some(home), &has(&["/usr/local/bin/claude"]), &none), Some("/usr/local/bin/claude".into()));
    }

    #[test]
    fn the_arguments_switch_everything_off() {
        let args = build_args(None);
        let at = |flag: &str| args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("no {flag}"));
        assert_eq!(args[at("--tools") + 1], "");
        assert_eq!(args[at("--setting-sources") + 1], "");
        assert!(args.contains(&"--disable-slash-commands".to_owned()) && args.contains(&"--strict-mcp-config".to_owned()));
        assert!(!args.contains(&"--no-session-persistence".to_owned()), "a session that cannot be resumed is no conversation");
        assert!(!args.contains(&"--resume".to_owned()));
        assert!(build_args(Some("abc")).windows(2).any(|w| w == ["--resume", "abc"]));
    }
}
