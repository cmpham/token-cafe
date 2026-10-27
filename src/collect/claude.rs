//! Claude Code: ~/.claude/sessions/<pid>.json, one per interactive session.
//! status busy -> work, idle -> idle, waiting -> wait (waitingFor
//! "permission prompt" -> "!", anything else -> "?").

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::{Found, basename, process, truthy};
use crate::claude_config;
use crate::scene::{Kind, State, Why};

fn millis(d: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| d.get(*k).filter(|v| truthy(v)).and_then(Value::as_f64))
}

#[derive(Default)]
pub struct Claude {
    desktop: DesktopMetadata,
    roots: Vec<PathBuf>,
    refreshed: Option<Instant>,
}

impl Claude {
    pub fn sessions(&mut self, now: f64) -> Vec<Found> {
        let desktop = if cfg!(target_os = "macos") {
            self.desktop.refresh(&crate::usage::home().join("Library/Application Support/Claude/claude-code-sessions"))
        } else {
            HashMap::new()
        };
        if self.refreshed.is_none_or(|t| t.elapsed() >= Duration::from_secs(5)) {
            self.roots = claude_config::roots();
            self.refreshed = Some(Instant::now());
        }
        let mut all = Vec::new();
        for root in &self.roots {
            let current = claude_config::current_account(root);
            let mut found = sessions_at(now, &root.join("sessions"), &desktop, process::matches_session);
            for f in &mut found {
                let identity = f.claude_identity.as_mut().unwrap();
                let start = process::start_epoch(identity.pid);
                let env = process::environment(identity.pid, claude_config::IDENTITY_KEYS);
                let get = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
                // Recheck after reading the environment in case the PID exited/reused.
                if start <= 0.0 || process::start_epoch(identity.pid) != start {
                    continue;
                }
                if identity.surface == "cli" {
                    f.target = terminal_target(&env);
                }
                if let Some(account) = get("CLAUDE_CODE_ACCOUNT_UUID").filter(|s| !s.is_empty()) {
                    identity.account = Some(serde_json::json!({
                        "account_id": account,
                        "organization_id": get("CLAUDE_CODE_ORGANIZATION_UUID"),
                        "source": "process_environment",
                    }));
                } else if identity.account.is_none() && identity.surface == "cli" {
                    identity.account = current.clone();
                }
            }
            all.extend(found);
        }
        deduplicate(all)
    }
}

fn terminal_target(env: &[(String, String)]) -> Option<crate::scene::SessionTarget> {
    let get = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let id = get("ITERM_SESSION_ID")
        .or_else(|| (get("TERM_PROGRAM") == Some("iTerm.app")).then(|| get("TERM_SESSION_ID")).flatten())?;
    let target = crate::scene::SessionTarget::ITerm(id.to_owned());
    target.url()?;
    Some(target)
}

#[derive(Clone, Debug)]
pub(super) struct Identity {
    pub config_dir: PathBuf,
    pub surface: &'static str,
    pub account: Option<Value>,
    pid: u32,
}

impl Identity {
    pub fn json(&self) -> Value {
        serde_json::json!({"config_dir": self.config_dir, "surface": self.surface,
            "account": self.account, "pid": self.pid})
    }
}

fn deduplicate(found: Vec<Found>) -> Vec<Found> {
    let mut sessions: HashMap<String, Found> = HashMap::new();
    for f in found {
        let key = if f.session_id.is_empty() { f.id.clone() } else { f.session_id.clone() };
        if sessions.get(&key).is_none_or(|old| old.since < f.since) {
            sessions.insert(key, f);
        }
    }
    sessions.into_values().collect()
}

#[derive(Clone)]
struct DesktopSession {
    desktop_id: Option<String>,
    id: String,
    title: String,
    archived: bool,
    account: Option<Value>,
}

fn desktop_session(raw: &[u8]) -> Option<DesktopSession> {
    let d: Value = serde_json::from_slice(raw).ok()?;
    let id = d.get("cliSessionId")?.as_str()?.to_owned();
    if id.is_empty() {
        return None;
    }
    Some(DesktopSession {
        desktop_id: d.get("sessionId").and_then(Value::as_str).map(str::to_owned),
        id,
        title: d.get("title").and_then(Value::as_str).unwrap_or("").to_owned(),
        archived: d.get("isArchived").and_then(Value::as_bool).unwrap_or(false),
        account: None,
    })
}

#[derive(Default)]
struct DesktopMetadata {
    last: Option<Instant>,
    files: HashMap<PathBuf, (SystemTime, u64, Option<DesktopSession>)>,
}

impl DesktopMetadata {
    fn refresh(&mut self, root: &Path) -> HashMap<String, DesktopSession> {
        if self.last.is_none_or(|t| t.elapsed() >= Duration::from_secs(5)) {
            let mut paths = Vec::new();
            metadata_files(root, 3, &mut paths);
            let mut fresh = HashMap::new();
            for path in paths {
                let Ok(meta) = fs::metadata(&path) else { continue };
                let Ok(modified) = meta.modified() else { continue };
                let len = meta.len();
                let entry = match self.files.remove(&path) {
                    Some(entry) if entry.0 == modified && entry.1 == len => entry,
                    _ => (
                        modified,
                        len,
                        fs::read(&path).ok().and_then(|raw| {
                            let mut session = desktop_session(&raw)?;
                            let parts: Vec<_> = path.strip_prefix(root).ok()?.components().collect();
                            if parts.len() == 3 {
                                session.account = Some(serde_json::json!({
                                    "account_id": parts[0].as_os_str().to_str()?,
                                    "organization_id": parts[1].as_os_str().to_str()?,
                                    "source": "desktop_index",
                                }));
                            }
                            Some(session)
                        }),
                    ),
                };
                fresh.insert(path, entry);
            }
            self.files = fresh;
            self.last = Some(Instant::now());
        }
        // A copied/imported session may appear under several accounts. Do not
        // randomly attach a title, archive bit or account from those copies.
        let mut grouped: HashMap<String, Vec<DesktopSession>> = HashMap::new();
        for session in self.files.values().filter_map(|(_, _, session)| session.clone()) {
            grouped.entry(session.id.clone()).or_default().push(session);
        }
        grouped
            .into_iter()
            .filter_map(|(id, mut sessions)| (sessions.len() == 1).then(|| (id, sessions.remove(0))))
            .collect()
    }
}

fn metadata_files(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() && depth > 0 {
            metadata_files(&entry.path(), depth - 1, out);
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "json") {
            out.push(entry.path());
        }
    }
}

// Desktop records are saved history, not proof that a session is running.
// Only enrich sessions whose process identity passed the normal live check.
fn sessions_at(
    now: f64,
    directory: &Path,
    desktop: &HashMap<String, DesktopSession>,
    alive: impl Fn(u32, &Value) -> bool,
) -> Vec<Found> {
    let mut out: HashMap<String, Found> = HashMap::new();
    let Ok(rd) = fs::read_dir(directory) else { return Vec::new() };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || !name.ends_with(".json") {
            continue;
        }
        let Ok(raw) = fs::read(&path) else { continue };
        let Ok(d) = serde_json::from_slice::<Value>(&raw) else { continue };
        match d.get("kind") {
            None => {}
            Some(Value::String(k)) if k == "interactive" => {}
            _ => continue,
        }
        let Some(pid) = d.get("pid").filter(|p| truthy(p)).and_then(Value::as_u64) else { continue };
        let Ok(pid) = u32::try_from(pid) else { continue };
        if !alive(pid, &d) {
            continue; // dead process or a reused pid
        }
        let metadata = d
            .get("sessionId")
            .and_then(Value::as_str)
            .and_then(|id| desktop.get(id))
            .filter(|_| d.get("entrypoint").and_then(Value::as_str) != Some("cli"));
        if metadata.is_some_and(|s| s.archived) {
            continue;
        }
        let status = d.get("status").and_then(Value::as_str);
        let state = match status {
            Some("busy") => State::Work,
            Some("waiting") => State::Wait,
            _ => State::Idle,
        };
        let why = if state != State::Wait {
            Why::None
        } else {
            let wf =
                d.get("waitingFor").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("permission prompt");
            if wf == "permission prompt" { Why::Perm } else { Why::Ask }
        };
        let since = millis(&d, &["statusUpdatedAt", "updatedAt", "startedAt"]).unwrap_or(now * 1000.0) / 1000.0;
        let sid = d
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map_or_else(|| pid.to_string(), str::to_owned)
            .replace('-', "");
        let sid8: String = sid.chars().take(8).collect();
        let cwd = d.get("cwd").and_then(Value::as_str).unwrap_or("");
        let name = [
            metadata.map_or("", |s| s.title.as_str()),
            d.get("name").and_then(Value::as_str).unwrap_or(""),
            basename(cwd),
        ]
        .into_iter()
        .find(|s| !s.is_empty())
        .map_or_else(|| sid8.clone(), str::to_owned);
        if out.get(&sid).is_some_and(|old| old.since > since) {
            continue;
        }
        out.insert(
            sid,
            Found {
                claude_identity: Some(Identity {
                    config_dir: directory.parent().unwrap_or(directory).to_owned(),
                    surface: match d.get("entrypoint").and_then(Value::as_str) {
                        Some("claude-desktop") => "desktop",
                        Some("cli") => "cli",
                        _ if metadata.is_some() => "desktop",
                        _ => "unknown",
                    },
                    account: metadata.and_then(|s| s.account.clone()),
                    pid,
                }),
                cwd: cwd.to_owned(),
                active_subagents: None,
                id: sid8,
                name,
                kind: Kind::Claude,
                state,
                why,
                since,
                started: millis(&d, &["startedAt"]).unwrap_or(0.0) / 1000.0,
                session_id: d.get("sessionId").and_then(Value::as_str).unwrap_or("").to_owned(),
                project: cwd.to_owned(),
                transcript: None,
                target: metadata.and_then(|s| s.desktop_id.clone()).map(crate::scene::SessionTarget::Claude),
            },
        );
    }
    out.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::super::TestDir;
    use super::*;

    #[test]
    fn terminal_targets_use_exact_iterm_identity_and_validate_environment_data() {
        let env =
            |fields: &[(&str, &str)]| fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
        let id = "w0t0p0:11111111-2222-4333-8444-555555555555";
        let target = terminal_target(&env(&[("ITERM_SESSION_ID", id)])).unwrap();
        assert_eq!(target.url().as_deref(), Some(format!("iterm2:reveal?sessionid={id}").as_str()));
        assert_eq!(terminal_target(&env(&[("TERM_PROGRAM", "iTerm.app"), ("TERM_SESSION_ID", id)])), Some(target));
        assert!(terminal_target(&env(&[("TERM_PROGRAM", "Apple_Terminal"), ("TERM_SESSION_ID", id)])).is_none());
        assert!(terminal_target(&[]).is_none());
        for id in ["", "session&c=whoami", "../session", "iterm2:/command?c=whoami", "session\ncommand"] {
            assert!(terminal_target(&env(&[("ITERM_SESSION_ID", id)])).is_none());
        }
    }

    #[test]
    fn desktop_history_only_enriches_live_sessions_and_keeps_waiting_state() {
        let dir = TestDir::new();
        let raw = serde_json::json!({"sessionId":"local_ui-id", "cliSessionId":"live-cli-id", "title":"Moon Café", "isArchived":false});
        let metadata = desktop_session(raw.to_string().as_bytes()).unwrap();
        let mut desktop = HashMap::from([(metadata.id.clone(), metadata)]);
        assert!(sessions_at(1.0, &dir.0, &desktop, |_, _| true).is_empty());
        let status = serde_json::json!({"pid":42, "sessionId":"live-cli-id", "kind":"interactive",
            "status":"waiting", "waitingFor":"permission prompt", "name":"old title", "statusUpdatedAt":1000});
        fs::write(dir.0.join("42.json"), status.to_string()).unwrap();
        // Duplicate live records can exist while reconnecting a desktop session.
        fs::write(dir.0.join("43.json"), status.to_string()).unwrap();
        let found = sessions_at(2.0, &dir.0, &desktop, |pid, _| pid == 42);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].target.as_ref().and_then(|t| t.url()).as_deref(),
            Some("claude://code/continue?session=local_ui-id")
        );
        assert_eq!(found[0].name, "Moon Café");
        assert_eq!(found[0].state, State::Wait);
        assert_eq!(found[0].why, Why::Perm);
        assert!(sessions_at(2.0, &dir.0, &desktop, |_, _| false).is_empty());
        desktop.get_mut("live-cli-id").unwrap().archived = true;
        assert!(sessions_at(2.0, &dir.0, &desktop, |_, _| true).is_empty());
    }

    #[test]
    fn desktop_metadata_refreshes_titles_and_removes_deleted_history() {
        let dir = TestDir::new();
        let path = dir.0.join("session.json");
        fs::write(&path, r#"{"cliSessionId":"abc","title":"Moon"}"#).unwrap();
        let mut metadata = DesktopMetadata::default();
        assert_eq!(metadata.refresh(&dir.0)["abc"].title, "Moon");
        fs::write(&path, r#"{"cliSessionId":"abc","title":"Moon Café"}"#).unwrap();
        metadata.last = None;
        assert_eq!(metadata.refresh(&dir.0)["abc"].title, "Moon Café");
        fs::remove_file(path).unwrap();
        metadata.last = None;
        assert!(metadata.refresh(&dir.0).is_empty());
        assert!(desktop_session(br#"{"sessionId":"desktop-only"}"#).is_none());
    }

    #[test]
    fn multiple_roots_and_organizations_keep_separate_live_sessions() {
        let dir = TestDir::new();
        let index = dir.0.join("desktop");
        let root_a = dir.0.join("personal");
        let root_b = dir.0.join("work");
        for (id, org, root, pid) in [("personal", "org-a", &root_a, 41), ("work", "org-b", &root_b, 42)] {
            let scope = index.join("same-user").join(org);
            fs::create_dir_all(&scope).unwrap();
            fs::write(
                scope.join(format!("{id}.json")),
                serde_json::json!({"cliSessionId":id,"sessionId":format!("local_{id}"),"title":org}).to_string(),
            )
            .unwrap();
            fs::create_dir_all(root.join("sessions")).unwrap();
            fs::write(
                root.join("sessions").join(format!("{pid}.json")),
                serde_json::json!({"pid":pid,"sessionId":id,"entrypoint":"claude-desktop","status":"busy"}).to_string(),
            )
            .unwrap();
        }
        let mut metadata = DesktopMetadata::default();
        let desktop = metadata.refresh(&index);
        let a = sessions_at(1., &root_a.join("sessions"), &desktop, |_, _| true);
        let b = sessions_at(1., &root_b.join("sessions"), &desktop, |_, _| true);
        assert_eq!(a[0].claude_identity.as_ref().unwrap().account.as_ref().unwrap()["organization_id"], "org-a");
        assert_eq!(b[0].claude_identity.as_ref().unwrap().account.as_ref().unwrap()["organization_id"], "org-b");
        let mut all = a;
        all.extend(b);
        all.extend(sessions_at(1., &root_a.join("sessions"), &desktop, |_, _| true));
        assert_eq!(deduplicate(all).len(), 2);
        assert!(sessions_at(1., &root_b.join("sessions"), &desktop, |_, _| false).is_empty());
    }

    #[test]
    fn cli_resume_does_not_inherit_desktop_archive_or_account() {
        let dir = TestDir::new();
        let metadata =
            desktop_session(br#"{"cliSessionId":"shared","title":"Desktop title","isArchived":true}"#).unwrap();
        let desktop = HashMap::from([("shared".into(), metadata)]);
        fs::write(
            dir.0.join("42.json"),
            r#"{"pid":42,"sessionId":"shared","entrypoint":"cli","name":"CLI title","status":"waiting"}"#,
        )
        .unwrap();
        let found = sessions_at(1., &dir.0, &desktop, |_, _| true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "CLI title");
        assert!(found[0].target.is_none());
        let identity = found[0].claude_identity.as_ref().unwrap();
        assert_eq!(identity.surface, "cli");
        assert!(identity.account.is_none());
        assert_eq!(found[0].state, State::Wait);
    }

    #[test]
    fn copied_desktop_history_does_not_choose_an_arbitrary_account() {
        let dir = TestDir::new();
        for org in ["a", "b"] {
            let scope = dir.0.join("user").join(org);
            fs::create_dir_all(&scope).unwrap();
            fs::write(scope.join("session.json"), r#"{"cliSessionId":"shared"}"#).unwrap();
        }
        assert!(DesktopMetadata::default().refresh(&dir.0).is_empty());
    }
}
