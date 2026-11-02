//! Read-only account limits. Never refresh credentials or persist tokens.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
struct Window {
    remaining: u8,
    reset: i64,
    fetched: i64,
}
impl Window {
    fn new(used: f64, reset: i64, now: i64) -> Option<Self> {
        (used.is_finite() && used >= 0.0 && reset > now).then(|| Self {
            remaining: (100.0 - used).clamp(0.0, 100.0).floor() as u8,
            reset,
            fetched: now,
        })
    }
    fn remaining(self, now: i64) -> Option<u8> {
        (now < self.reset && (0..600).contains(&(now - self.fetched))).then_some(self.remaining)
    }
}
#[cfg(target_os = "macos")]
#[derive(Default)]
pub(super) struct Weekly {
    claude: [ClaudeUsage; 2],
    codex: Option<Window>,
}
#[cfg(target_os = "macos")]
impl Weekly {
    pub(super) fn remaining(&self) -> [Option<u8>; 3] {
        let now = chrono::Utc::now().timestamp();
        let windows = claude_windows(&self.claude[0], &self.claude[1]);
        [windows[0], windows[1], self.codex].map(|w| w.and_then(|w| w.remaining(now)))
    }
    pub(super) fn dual_claude(&self) -> bool {
        separate_claude(&self.claude[0], &self.claude[1])
    }
}
#[cfg(target_os = "macos")]
pub(super) fn start() -> Arc<Mutex<Weekly>> {
    let shared = Arc::new(Mutex::new(Weekly::default()));
    for slot in 0..2 {
        let output = shared.clone();
        thread::spawn(move || {
            let mut poller = ClaudePoller::default();
            loop {
                let credential = if slot == 0 { claude() } else { claude_desktop() };
                poller.poll(credential, chrono::Utc::now().timestamp(), claude_get);
                {
                    let mut latest = output.lock().unwrap();
                    latest.claude[slot] = poller.usage.clone();
                    eprintln!(
                        "Keypad weekly Claude {}: {:?}; remaining {:?}",
                        if slot == 0 { "CLI" } else { "Desktop" },
                        poller.status,
                        latest.remaining()[slot]
                    );
                }
                // Re-read existing credentials sooner than the quota request interval.
                thread::sleep(Duration::from_secs(60));
            }
        });
    }
    let output = shared.clone();
    thread::spawn(move || {
        loop {
            let window = codex();
            let mut latest = output.lock().unwrap();
            // A transient failure must not discard a still-fresh successful response.
            if window.is_some() {
                latest.codex = window;
            }
            eprintln!("Keypad weekly Codex remaining: {:?}", latest.remaining()[2]);
            drop(latest);
            thread::sleep(Duration::from_secs(300));
        }
    });
    shared
}

// Explicit one-shot diagnosis: no keypad ownership, app navigation or token output.
pub fn diagnose() {
    for (label, credential) in [("Claude CLI", claude()), ("Claude Desktop", claude_desktop())] {
        let mut poller = ClaudePoller::default();
        poller.poll(credential, chrono::Utc::now().timestamp(), claude_get);
        println!(
            "{label}: {:?}, remaining {:?}",
            poller.status,
            poller.usage.window.and_then(|w| w.remaining(chrono::Utc::now().timestamp()))
        );
    }
    println!("Codex: remaining {:?}", codex().and_then(|w| w.remaining(chrono::Utc::now().timestamp())));
}

// Tokens stay in the poller's memory, never Debug output or persisted caches.
struct Credential {
    token: String,
    expires: Option<i64>,
    identity: Option<(String, String)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ready,
    LoginUnavailable,
    ExpiredCredential,
    Transport,
    Http(u16),
    InvalidResponse,
    NoWeeklyWindow,
}
#[derive(Clone, Copy, Debug)]
struct Failure {
    status: Status,
    retry_after: i64,
}
impl From<Status> for Failure {
    fn from(status: Status) -> Self {
        Self { status, retry_after: 300 }
    }
}
struct ClaudePoller {
    token: String,
    usage: ClaudeUsage,
    next_poll: i64,
    rejected: bool,
    status: Status,
}
impl Default for ClaudePoller {
    fn default() -> Self {
        Self {
            token: String::new(),
            usage: ClaudeUsage::default(),
            next_poll: 0,
            rejected: false,
            status: Status::LoginUnavailable,
        }
    }
}
impl ClaudePoller {
    fn poll(
        &mut self,
        credential: Option<Credential>,
        now: i64,
        mut get: impl FnMut(&str, &str) -> Result<Value, Failure>,
    ) {
        let Some(credential) = credential else {
            *self = Self::default();
            return;
        };
        if self.token != credential.token {
            *self = Self { token: credential.token, ..Self::default() };
        }
        if credential.expires.is_some_and(|expiry| expiry <= now) {
            self.usage.window = None;
            self.status = Status::ExpiredCredential;
            return;
        }
        // A rejected bearer cannot recover without the owner supplying a new token.
        if self.rejected || now < self.next_poll {
            return;
        }
        self.next_poll = now.saturating_add(300);
        let result = (|| {
            if self.usage.identity.is_none() {
                let profile = get(&self.token, "https://api.anthropic.com/api/oauth/profile")?;
                let identity = claude_identity(&profile).ok_or(Status::InvalidResponse)?;
                if credential.identity.as_ref().is_some_and(|expected| expected != &identity) {
                    return Err(Status::InvalidResponse.into());
                }
                self.usage.identity = Some(identity);
            }
            let value = get(&self.token, "https://api.anthropic.com/api/oauth/usage")?;
            self.usage.window = claude_window(&value, now);
            Ok::<_, Failure>(())
        })();
        match result {
            Ok(()) => {
                self.status = if self.usage.window.is_some() { Status::Ready } else { Status::NoWeeklyWindow };
            }
            Err(error) => {
                self.status = error.status;
                self.next_poll = now.saturating_add(error.retry_after.clamp(300, 86_400));
                if error.status == Status::Http(401) {
                    self.rejected = true;
                    self.usage.window = None;
                }
            }
        }
    }
}

// Drain bounded output concurrently so a verbose child cannot block its timeout.
fn output(mut command: Command, input: &[u8]) -> Option<Vec<u8>> {
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(1_048_577).read_to_end(&mut bytes).ok()?;
        (bytes.len() <= 1_048_576).then_some(bytes)
    });
    let written = child.stdin.take().is_some_and(|mut stdin| stdin.write_all(input).is_ok());
    let deadline = Instant::now() + Duration::from_secs(20);
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break written && status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let bytes = reader.join().ok().flatten();
    success.then_some(bytes).flatten()
}
fn claude_window(value: &Value, now: i64) -> Option<Window> {
    let w = &value["seven_day"];
    Window::new(
        w["utilization"].as_f64()?,
        chrono::DateTime::parse_from_rfc3339(w["resets_at"].as_str()?).ok()?.timestamp(),
        now,
    )
}
fn claude() -> Option<Credential> {
    #[cfg(target_os = "macos")]
    let bytes = {
        let mut cmd = Command::new("/usr/bin/security");
        cmd.args(["find-generic-password", "-s", "Claude Code-credentials", "-w"]);
        output(cmd, &[])?
    };
    #[cfg(not(target_os = "macos"))]
    let bytes = std::fs::read(
        std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".claude")))?
            .join(".credentials.json"),
    )
    .ok()?;
    let credentials: Value = serde_json::from_slice(&bytes).ok()?;
    let token = credentials["claudeAiOauth"]["accessToken"].as_str()?;
    Some(Credential {
        token: token.into(),
        expires: credentials["claudeAiOauth"]["expiresAt"].as_i64().map(|ms| ms / 1000),
        identity: None,
    })
}

#[derive(Clone, Default)]
struct ClaudeUsage {
    identity: Option<(String, String)>,
    window: Option<Window>,
}
fn claude_identity(value: &Value) -> Option<(String, String)> {
    let account = value["account"]["uuid"].as_str()?.trim();
    let org = value["organization"]["uuid"].as_str()?.trim();
    (!account.is_empty() && !org.is_empty()).then(|| (account.into(), org.into()))
}
#[cfg(any(target_os = "macos", test))]
fn separate_claude(cli: &ClaudeUsage, desktop: &ClaudeUsage) -> bool {
    desktop.identity.as_ref().is_some_and(|id| cli.identity.as_ref() != Some(id))
}
#[cfg(any(target_os = "macos", test))]
fn claude_windows(cli: &ClaudeUsage, desktop: &ClaudeUsage) -> [Option<Window>; 2] {
    if separate_claude(cli, desktop) {
        [cli.window, desktop.window]
    } else {
        [cli.window.into_iter().chain(desktop.window).max_by_key(|w| w.fetched), None]
    }
}
fn claude_get(token: &str, endpoint: &str) -> Result<Value, Failure> {
    // Credentials stay in memory and curl's stdin, never argv or logs.
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Status::InvalidResponse.into());
    }
    let header =
        serde_json::to_string(&format!("Authorization: Bearer {token}")).map_err(|_| Status::InvalidResponse)?;
    let config = format!("header = {header}\nheader = \"anthropic-beta: oauth-2025-04-20\"\n");
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.args(["--silent", "--include", "--max-time", "15", "--config", "-", endpoint]);
    let bytes = output(cmd, config.as_bytes()).ok_or(Status::Transport)?;
    claude_response(&bytes, chrono::Utc::now().timestamp())
}
fn claude_response(mut bytes: &[u8], now: i64) -> Result<Value, Failure> {
    // A proxy's CONNECT response or an informational response may precede the
    // provider headers. Do not save headers or include response bodies in errors.
    loop {
        let end = bytes.windows(4).position(|w| w == b"\r\n\r\n").ok_or(Status::InvalidResponse)?;
        let header = std::str::from_utf8(&bytes[..end]).map_err(|_| Status::InvalidResponse)?;
        let mut lines = header.split("\r\n");
        let mut status_line = lines.next().ok_or(Status::InvalidResponse)?.split_whitespace();
        if !status_line.next().is_some_and(|s| s.starts_with("HTTP/")) {
            return Err(Status::InvalidResponse.into());
        }
        let status: u16 = status_line.next().and_then(|s| s.parse().ok()).ok_or(Status::InvalidResponse)?;
        let retry_after =
            lines.find_map(|line| {
                let (key, value) = line.split_once(':')?;
                if !key.eq_ignore_ascii_case("retry-after") {
                    return None;
                }
                value.trim().parse::<i64>().ok().or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(value.trim()).ok().map(|date| date.timestamp() - now)
                })
            });
        bytes = &bytes[end + 4..];
        if (100..200).contains(&status) || (status == 200 && bytes.starts_with(b"HTTP/")) {
            continue;
        }
        if status != 200 {
            return Err(Failure { status: Status::Http(status), retry_after: retry_after.unwrap_or(300) });
        }
        return serde_json::from_slice(bytes).map_err(|_| Status::InvalidResponse.into());
    }
}
fn claude_desktop() -> Option<Credential> {
    #[cfg(target_os = "macos")]
    {
        use crate::collect::process;
        let root = crate::usage::home().join("Library/Application Support/Claude/claude-code");
        let mut candidates: Vec<_> = process::pids()
            .into_iter()
            .filter_map(|pid| {
                let exe = process::exe(pid)?;
                (exe.starts_with(&root) && exe.file_name().is_some_and(|n| n == "claude"))
                    .then(|| (pid, process::start_epoch(pid)))
            })
            .filter(|(_, start)| *start > 0.0)
            .collect();
        candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
        // A verified Code process identifies its organization. Prefer Desktop's
        // current encrypted cache; the inherited launch token cannot reflect refreshes.
        let mut launch = None;
        let mut organization = None;
        for (pid, start) in candidates.into_iter().take(3) {
            let fields = process::environment(
                pid,
                &["CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CODE_ORGANIZATION_UUID"],
            );
            if process::start_epoch(pid) != start {
                continue;
            }
            if !fields.iter().any(|(k, v)| k == "CLAUDE_CODE_ENTRYPOINT" && v == "claude-desktop") {
                continue;
            }
            if let Some((_, token)) = fields.iter().find(|(k, _)| k == "CLAUDE_CODE_OAUTH_TOKEN") {
                launch = Some(Credential { token: token.clone(), expires: None, identity: None });
                organization = fields
                    .iter()
                    .find(|(key, _)| key == "CLAUDE_CODE_ORGANIZATION_UUID")
                    .map(|(_, value)| value.clone());
                break;
            }
        }
        if let Some(exe) = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.join("keypad-quota"))) {
            let mut command = Command::new(exe);
            command.arg("read");
            if let Some(org) = organization {
                command.arg(org);
            }
            if let Some(bytes) = output(command, &[])
                && let Some(credential) = desktop_cache_credential(&bytes)
            {
                return Some(credential);
            }
        }
        launch
    }
    #[cfg(not(target_os = "macos"))]
    None
}
#[cfg(any(target_os = "macos", test))]
fn desktop_cache_credential(bytes: &[u8]) -> Option<Credential> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let token = value["token"].as_str()?.to_string();
    let account = value["account"].as_str()?.to_string();
    let org = value["org"].as_str()?.to_string();
    if token.is_empty() || account.is_empty() || org.is_empty() {
        return None;
    }
    Some(Credential { token, expires: Some(value["expiresAt"].as_i64()? / 1000), identity: Some((account, org)) })
}

fn codex_window(result: &Value, now: i64) -> Option<Window> {
    let limits = if let Some(map) = result.get("rateLimitsByLimitId").filter(|v| !v.is_null()) {
        map.get("codex")?
    } else {
        &result["rateLimits"]
    };
    ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| {
            let w = &limits[key];
            if w["windowDurationMins"].as_i64()? != 10080 {
                return None;
            }
            Window::new(w["usedPercent"].as_f64()?, w["resetsAt"].as_i64()?, now)
        })
        .min_by_key(|w| w.remaining)
}
fn codex() -> Option<Window> {
    let executable = ["/opt/homebrew/bin/codex", "/usr/local/bin/codex"]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
        .unwrap_or("codex");
    let mut child = Command::new(executable)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout.take(1_048_576)).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let result = (|| {
        let stdin = child.stdin.as_mut()?;
        writeln!(
            stdin,
            "{}",
            json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"token-cafe","version":"0.1.0"}}})
        )
        .ok()?;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let line = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok()?;
            let value: Value = serde_json::from_str(&line).ok()?;
            match value["id"].as_u64() {
                Some(1) => {
                    value.get("result")?;
                    writeln!(stdin, "{}", json!({"method":"initialized"})).ok()?;
                    writeln!(stdin, "{}", json!({"id":2,"method":"account/rateLimits/read","params":{}})).ok()?;
                }
                Some(2) => return codex_window(value.get("result")?, chrono::Utc::now().timestamp()),
                _ => (),
            }
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separate_claude_allowances_use_account_and_org_not_email_or_percent() {
        let profile =
            |org| json!({"account":{"uuid":"same-user","email":"same@example.invalid"},"organization":{"uuid":org}});
        let usage =
            |org| ClaudeUsage { identity: claude_identity(&profile(org)), window: Window::new(5.0, 2000, 1000) };
        assert!(claude_windows(&usage("cli-org"), &usage("desktop-org"))[1].is_some());
        assert!(claude_windows(&usage("same-org"), &usage("same-org"))[1].is_none());
        assert!(claude_windows(&usage("cli-org"), &ClaudeUsage::default())[1].is_none());
        let unknown = ClaudeUsage::default();
        assert!(claude_windows(&unknown, &usage("desktop-org"))[1].is_some());
        assert!(claude_identity(&json!({"account":{"email":"same@example.invalid"}})).is_none());
    }

    fn credential(token: &str) -> Option<Credential> {
        Some(Credential { token: token.into(), expires: None, identity: None })
    }
    fn profile(org: &str) -> Value {
        json!({"account":{"uuid":"same-user"},"organization":{"uuid":org}})
    }
    fn usage(used: u8) -> Value {
        json!({"seven_day":{"utilization":used,"resets_at":"1970-01-02T00:00:00Z"}})
    }
    #[test]
    fn desktop_cache_identity_must_match_provider_before_usage_is_requested() {
        let bytes = serde_json::to_vec(&json!({"token":"fictional", "expiresAt":2_000_000,
            "account":"same-user", "org":"expected-org"}))
        .unwrap();
        let mut poller = ClaudePoller::default();
        poller.poll(desktop_cache_credential(&bytes), 1000, |_, url| {
            assert!(url.ends_with("profile"));
            Ok(profile("different-org"))
        });
        assert_eq!(poller.status, Status::InvalidResponse);
        assert!(poller.usage.window.is_none());
        assert!(poller.usage.identity.is_none());
        assert!(desktop_cache_credential(b"{}").is_none());
    }
    #[test]
    fn expired_credentials_never_make_requests() {
        let mut poller = ClaudePoller::default();
        poller.poll(Some(Credential { token: "expired".into(), expires: Some(1000), identity: None }), 1000, |_, _| {
            panic!("expired credentials must stay local")
        });
        assert_eq!(poller.status, Status::ExpiredCredential);
        assert!(poller.usage.window.is_none());
    }
    #[test]
    fn profile_cached_usage_polled_every_five_minutes() {
        let mut poller = ClaudePoller::default();
        let mut calls = Vec::new();
        poller.poll(credential("cli"), 1000, |_, url| {
            calls.push(url.to_string());
            Ok(if url.ends_with("profile") { profile("cli-org") } else { usage(71) })
        });
        assert_eq!(calls.len(), 2);
        assert_eq!(poller.usage.window.unwrap().remaining(1000), Some(29));
        poller.poll(credential("cli"), 1299, |_, _| panic!("too early"));
        poller.poll(credential("cli"), 1300, |_, url| {
            assert!(url.ends_with("usage"));
            Ok(usage(72))
        });
        assert_eq!(poller.usage.window.unwrap().remaining(1300), Some(28));
    }
    #[test]
    fn rate_limit_retains_fresh_data_and_identity_and_honors_backoff() {
        let mut poller = ClaudePoller::default();
        poller.poll(credential("desktop"), 1000, |_, url| {
            Ok(if url.ends_with("profile") { profile("desktop-org") } else { usage(71) })
        });
        poller.poll(credential("desktop"), 1300, |_, _| Err(Failure { status: Status::Http(429), retry_after: 3600 }));
        assert_eq!(poller.usage.window.unwrap().remaining(1599), Some(29));
        assert_eq!(poller.usage.window.unwrap().remaining(1600), None);
        assert!(poller.usage.identity.is_some());
        for now in [1600, 2000, 4899] {
            poller.poll(credential("desktop"), now, |_, _| panic!("Retry-After ignored"));
        }
        poller.poll(credential("desktop"), 4900, |_, url| {
            assert!(url.ends_with("usage"));
            Ok(usage(75))
        });
        assert_eq!(poller.usage.window.unwrap().remaining(4900), Some(25));
    }
    #[test]
    fn unauthorized_token_is_not_retried_and_new_login_clears_old_values() {
        let mut poller = ClaudePoller::default();
        poller.poll(credential("first"), 1000, |_, url| {
            Ok(if url.ends_with("profile") { profile("first-org") } else { usage(71) })
        });
        poller.poll(credential("first"), 1300, |_, _| Err(Status::Http(401).into()));
        assert!(poller.usage.window.is_none());
        assert!(poller.usage.identity.is_some());
        poller.poll(credential("first"), 9999, |_, _| panic!("rejected token retried"));
        poller.poll(credential("second"), 10_000, |_, _| Err(Status::Transport.into()));
        assert!(poller.usage.identity.is_none());
        assert!(poller.usage.window.is_none());
        poller.poll(credential("second"), 10_300, |_, url| {
            Ok(if url.ends_with("profile") { profile("second-org") } else { usage(15) })
        });
        assert_eq!(poller.status, Status::Ready);
        assert_eq!(poller.usage.identity, claude_identity(&profile("second-org")));
        assert_eq!(poller.usage.window.unwrap().remaining(10_300), Some(85));
        poller.poll(None, 10_301, |_, _| panic!("missing credentials"));
        assert!(poller.usage.identity.is_none());
        assert!(poller.usage.window.is_none());
    }
    #[test]
    fn same_account_uses_fresher_response_and_distinct_accounts_keep_two_slots() {
        let cli = ClaudeUsage { identity: claude_identity(&profile("same")), window: Window::new(71.0, 9000, 1000) };
        let mut desktop = ClaudeUsage { identity: cli.identity.clone(), window: Window::new(72.0, 9000, 1300) };
        let windows = claude_windows(&cli, &desktop);
        assert_eq!(windows[0].unwrap().remaining(1700), Some(28));
        assert!(windows[1].is_none());
        desktop.identity = claude_identity(&profile("other"));
        desktop.window = None;
        assert!(separate_claude(&cli, &desktop));
        assert!(claude_windows(&cli, &desktop)[1].is_none());
    }
    #[test]
    fn http_status_and_retry_after_are_parsed_without_exposing_response_body() {
        let response = b"HTTP/1.1 200 Connection established\r\n\r\nHTTP/2 200\r\ncontent-type: application/json\r\n\r\n{\"ok\":true}";
        assert_eq!(claude_response(response, 1000).unwrap(), json!({"ok":true}));
        let failure = claude_response(b"HTTP/2 429\r\nRetry-After: 3600\r\n\r\nprivate error body", 1000).unwrap_err();
        assert_eq!(failure.status, Status::Http(429));
        assert_eq!(failure.retry_after, 3600);
        let failure =
            claude_response(b"HTTP/2 429\r\nretry-after: Thu, 01 Jan 1970 01:00:00 GMT\r\n\r\n", 1000).unwrap_err();
        assert_eq!(failure.retry_after, 2600);
        assert_eq!(claude_response(b"HTTP/2 401\r\n\r\n{}", 1000).unwrap_err().status, Status::Http(401));
        assert_eq!(claude_response(b"HTTP/2 200\r\n\r\nnot json", 1000).unwrap_err().status, Status::InvalidResponse);
        assert!(claude_response(b"not http", 1000).is_err());
    }

    #[test]
    fn weekly_buckets_remaining_and_expiration() {
        let now = 1000;
        for (used, expected) in [(0.0, 100), (71.0, 29), (100.0, 0), (101.0, 0), (70.2, 29)] {
            assert_eq!(Window::new(used, 2000, now).unwrap().remaining(now), Some(expected));
        }
        assert!(Window::new(-1.0, 2000, now).is_none());
        assert!(Window::new(f64::NAN, 2000, now).is_none());
        assert!(Window::new(50.0, now, now).is_none());
        assert_eq!(Window::new(71.0, 2000, now).unwrap().remaining(1600), None);
        let w = json!({"usedPercent":71,"windowDurationMins":10080,"resetsAt":2000});
        for key in ["primary", "secondary"] {
            let mut limits = json!({"primary":null,"secondary":null});
            limits[key] = w.clone();
            assert_eq!(
                codex_window(&json!({"rateLimitsByLimitId":{"codex":limits}}), now).unwrap().remaining(now),
                Some(29)
            );
        }
        assert!(
            codex_window(
                &json!({"rateLimits":{"primary":{"usedPercent":71,"windowDurationMins":300,"resetsAt":2000}}}),
                now
            )
            .is_none()
        );
        assert!(codex_window(&json!({"rateLimitsByLimitId":{"other":{"primary":w}}}), now).is_none());
        assert_eq!(
            claude_window(&json!({"seven_day":{"utilization":71,"resets_at":"1970-01-01T00:33:20Z"}}), now)
                .unwrap()
                .remaining(now),
            Some(29)
        );
        assert!(claude_window(&json!({"seven_day":null}), now).is_none());
    }
}
