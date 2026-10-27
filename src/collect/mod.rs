//! Which Claude Code and Codex sessions run on this machine and what each is
//! doing.

mod claude;
mod codex;
mod info;
pub use info::SessionInfo;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
pub(crate) mod process;
#[cfg(not(target_os = "macos"))]
#[path = "procfs.rs"]
pub(crate) mod process;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::scene::{ForcedPose, Kind, Session, SessionDetail, State, Why};
pub(crate) use crate::usage::truthy;

/// One live session, as found on disk.
struct Found {
    id: String,
    name: String,
    kind: Kind,
    state: State,
    why: Why,
    /// Epoch seconds the session entered this state.
    since: f64,
    /// Epoch seconds the session started, 0 if unknown.
    started: f64,
    session_id: String,
    project: String,
    transcript: Option<std::path::PathBuf>,
    target: Option<crate::scene::SessionTarget>,
    cwd: String,
    active_subagents: Option<usize>,
    claude_identity: Option<claude::Identity>,
}

#[derive(Default)]
pub struct Collector {
    claude: claude::Claude,
    codex: codex::Codex,
    info: info::InfoCache,
}

impl Collector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every live session, first come first seated.
    fn found(&mut self) -> Vec<Found> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let mut all = self.claude.sessions(now);
        all.extend(self.codex.sessions(now));
        // Full IDs avoid collisions between UUID prefixes and between providers.
        for f in &mut all {
            f.id = session_key(f.kind, if f.session_id.is_empty() { &f.id } else { &f.session_id });
        }
        all.sort_by(|a, b| a.started.total_cmp(&b.started).then_with(|| a.id.cmp(&b.id)));
        all
    }

    /// Terminal prototype: transcript details for confirmed live sessions only.
    pub fn session_info(&mut self) -> Vec<SessionInfo> {
        let mut found = self.found();
        self.info.collect(&mut found)
    }

    /// One discovery snapshot supplies both patrons and their transcript details.
    pub fn sessions_with_details(&mut self) -> (Vec<Session>, Vec<SessionDetail>) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let mut found = self.found();
        let info = self.info.collect(&mut found);
        let details = found
            .iter()
            .zip(info.iter())
            .map(|(f, i)| {
                let mut detail = i.display(&f.id);
                detail.target = f.target.clone();
                detail
            })
            .collect();
        let mut branches = std::collections::HashMap::new();
        let sessions = found
            .into_iter()
            .map(|f| Session {
                branch: branches.entry(f.cwd.clone()).or_insert_with(|| git_branch(&f.cwd)).clone(),
                active_subagents: f.active_subagents,
                id: f.id,
                name: f.name,
                kind: f.kind,
                state: f.state,
                why: f.why,
                pose: ForcedPose::Auto,
                age_s: (now - f.since).max(0.0) as u32,
            })
            .collect();
        (sessions, details)
    }

    pub fn sessions(&mut self) -> Vec<Session> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let mut branches = std::collections::HashMap::new();
        let mut found = self.found();
        self.info.collect(&mut found);
        found
            .into_iter()
            .map(|f| Session {
                branch: branches.entry(f.cwd.clone()).or_insert_with(|| git_branch(&f.cwd)).clone(),
                active_subagents: f.active_subagents,
                id: f.id,
                name: f.name,
                kind: f.kind,
                state: f.state,
                why: f.why,
                pose: ForcedPose::Auto,
                age_s: (now - f.since).max(0.0) as u32,
            })
            .collect()
    }
}

fn session_key(kind: Kind, id: &str) -> String {
    let provider = if kind == Kind::Codex { "codex" } else { "claude" };
    format!("{provider}:{id}")
}

#[test]
fn session_identity_uses_full_id_and_provider() {
    assert_ne!(session_key(Kind::Claude, "019a1234-aaaa"), session_key(Kind::Claude, "019a1234-bbbb"));
    assert_ne!(session_key(Kind::Codex, "aaaa-12345678"), session_key(Kind::Codex, "bbbb-12345678"));
    assert_ne!(session_key(Kind::Claude, "same"), session_key(Kind::Codex, "same"));
}

/// The last path component ('' after a trailing '/').
fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// Filter before decoding or retaining values; never return the full environment.
fn selected_environment(raw: &[u8], keys: &[&str]) -> Vec<(String, String)> {
    raw.split(|&b| b == 0)
        .filter_map(|field| {
            let split = field.iter().position(|&b| b == b'=')?;
            let key = std::str::from_utf8(&field[..split]).ok()?;
            if !keys.contains(&key) {
                return None;
            }
            let value = std::str::from_utf8(&field[split + 1..]).ok()?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

#[cfg(test)]
pub(crate) struct TestDir(pub(crate) std::path::PathBuf);

#[cfg(test)]
impl TestDir {
    pub(crate) fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("token-cafe-{}-{unique}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

#[cfg(test)]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Compile the portable Linux identity regression tests on macOS too.
#[cfg(all(test, target_os = "macos"))]
#[allow(dead_code)]
#[path = "procfs.rs"]
mod linux_identity_tests;

/// Read the current checkout, including linked worktrees, without changing it.
fn git_branch(cwd: &str) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    let run = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    run(&["symbolic-ref", "--quiet", "--short", "HEAD"])
        .or_else(|| run(&["rev-parse", "--short", "HEAD"]).map(|sha| format!("DETACHED {sha}")))
}

#[cfg(test)]
mod branch_tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn reads_branch_changes_linked_worktrees_and_detached_heads() {
        let dir = TestDir::new();
        let repo = dir.0.join("repo");
        std::fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap().status.success());
        };
        git(&["init", "-b", "moon"]);
        assert_eq!(git_branch(repo.to_str().unwrap()).as_deref(), Some("moon"));
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ]);
        let linked = dir.0.join("linked");
        git(&["worktree", "add", "-b", "stars", linked.to_str().unwrap()]);
        assert_eq!(git_branch(linked.to_str().unwrap()).as_deref(), Some("stars"));
        git(&["checkout", "--detach"]);
        assert!(git_branch(repo.to_str().unwrap()).unwrap().starts_with("DETACHED "));
        assert_eq!(git_branch(linked.to_str().unwrap()).as_deref(), Some("stars"));
        assert!(git_branch(dir.0.to_str().unwrap()).is_none());
        assert!(git_branch("").is_none());
    }
}
