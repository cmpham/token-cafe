//! Read-only account limits. Never refresh credentials or persist tokens.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

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
#[derive(Default)]
pub(super) struct Weekly([Option<Window>; 3]);
impl Weekly {
    pub(super) fn remaining(&self) -> [Option<u8>; 3] {
        let now = chrono::Utc::now().timestamp();
        self.0.map(|w| w.and_then(|w| w.remaining(now)))
    }
}
pub(super) fn start() -> Arc<Mutex<Weekly>> {
    let shared = Arc::new(Mutex::new(Weekly::default()));
    let output = shared.clone();
    thread::spawn(move || {
        loop {
            let cli = claude();
            let desktop = claude_desktop();
            let claude = claude_windows(cli, desktop);
            let codex = codex();
            let latest = Weekly([claude[0], claude[1], codex]);
            eprintln!("Keypad weekly remaining (Claude CLI, Claude Desktop, Codex): {:?}", latest.remaining());
            *output.lock().unwrap() = latest;
            thread::sleep(Duration::from_secs(300));
        }
    });
    shared
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
fn claude() -> Option<ClaudeUsage> {
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
    claude_usage(token)
}

struct ClaudeUsage {
    identity: Option<(String, String)>,
    window: Option<Window>,
}
fn claude_identity(value: &Value) -> Option<(String, String)> {
    let account = value["account"]["uuid"].as_str()?.trim();
    let org = value["organization"]["uuid"].as_str()?.trim();
    (!account.is_empty() && !org.is_empty()).then(|| (account.into(), org.into()))
}
fn claude_windows(cli: Option<ClaudeUsage>, desktop: Option<ClaudeUsage>) -> [Option<Window>; 2] {
    let separate = desktop
        .as_ref()
        .and_then(|d| d.identity.as_ref())
        .is_some_and(|id| cli.as_ref().is_none_or(|c| c.identity.as_ref().is_some_and(|cli_id| cli_id != id)));
    [cli.and_then(|c| c.window), desktop.filter(|_| separate).and_then(|d| d.window)]
}
fn claude_get(token: &str, endpoint: &str) -> Option<Value> {
    // Credentials stay in memory and curl's stdin, never argv or logs.
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    let header = serde_json::to_string(&format!("Authorization: Bearer {token}")).ok()?;
    let config = format!("header = {header}\nheader = \"anthropic-beta: oauth-2025-04-20\"\n");
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.args(["--silent", "--fail", "--max-time", "15", "--config", "-", endpoint]);
    serde_json::from_slice(&output(cmd, config.as_bytes())?).ok()
}
fn claude_usage(token: &str) -> Option<ClaudeUsage> {
    let identity = claude_get(token, "https://api.anthropic.com/api/oauth/profile").and_then(|v| claude_identity(&v));
    let window = claude_get(token, "https://api.anthropic.com/api/oauth/usage")
        .and_then(|v| claude_window(&v, chrono::Utc::now().timestamp()));
    Some(ClaudeUsage { identity, window })
}
fn claude_desktop() -> Option<ClaudeUsage> {
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
        // Newer sessions carry fresher tokens; never fall back to desktop history
        // or the CLI keychain when no live desktop login can be identified.
        for (pid, start) in candidates.into_iter().take(3) {
            let fields = process::environment(pid, &["CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_OAUTH_TOKEN"]);
            if process::start_epoch(pid) != start {
                continue;
            }
            if !fields.iter().any(|(k, v)| k == "CLAUDE_CODE_ENTRYPOINT" && v == "claude-desktop") {
                continue;
            }
            if let Some((_, token)) = fields.iter().find(|(k, _)| k == "CLAUDE_CODE_OAUTH_TOKEN")
                && let Some(usage) = claude_usage(token)
                && usage.identity.is_some()
                && usage.window.is_some()
            {
                return Some(usage);
            }
        }
    }
    None
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
            |org| Some(ClaudeUsage { identity: claude_identity(&profile(org)), window: Window::new(5.0, 2000, 1000) });
        assert!(claude_windows(usage("cli-org"), usage("desktop-org"))[1].is_some());
        assert!(claude_windows(usage("same-org"), usage("same-org"))[1].is_none());
        assert!(claude_windows(usage("cli-org"), None)[1].is_none());
        let unknown = Some(ClaudeUsage { identity: None, window: Window::new(5.0, 2000, 1000) });
        assert!(claude_windows(unknown, usage("desktop-org"))[1].is_none());
        assert!(claude_identity(&json!({"account":{"email":"same@example.invalid"}})).is_none());
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
