//! Claude config roots, shared by live discovery, transcripts and usage.
use std::path::{Path, PathBuf};

use crate::collect::process;

pub(crate) const IDENTITY_KEYS: &[&str] = &[
    "CLAUDE_CONFIG_DIR",
    "CLAUDE_CODE_ACCOUNT_UUID",
    "CLAUDE_CODE_ORGANIZATION_UUID",
    "TERM_PROGRAM",
    "ITERM_SESSION_ID",
    "TERM_SESSION_ID",
];

pub(crate) fn is_claude(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "claude" || name == "claude.exe")
}

pub(crate) fn normalize(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

pub(crate) fn roots() -> Vec<PathBuf> {
    let mut roots = vec![crate::usage::home().join(".claude"), crate::usage::claude_dir()];
    if let Some(extra) = std::env::var_os("TOKEN_CAFE_CLAUDE_CONFIG_DIRS") {
        roots.extend(std::env::split_paths(&extra).filter(|p| p.is_absolute()));
    }
    for pid in process::pids() {
        if !process::exe(pid).is_some_and(|path| is_claude(&path)) {
            continue;
        }
        for (_, path) in process::environment(pid, &["CLAUDE_CONFIG_DIR"]) {
            let path = PathBuf::from(path);
            // Relative overrides resolve against the Claude process, not Token Café.
            if path.is_absolute() {
                roots.push(path);
            } else if !path.as_os_str().is_empty()
                && let Some(cwd) = process::cwd(pid)
            {
                roots.push(cwd.join(path));
            }
        }
    }
    roots.into_iter().map(normalize).collect::<std::collections::BTreeSet<_>>().into_iter().collect()
}

/// A CLI config is a current login hint, never proof of an older process's login.
pub(crate) fn current_account(root: &Path) -> Option<serde_json::Value> {
    let path = if root == normalize(crate::usage::home().join(".claude")) {
        crate::usage::home().join(".claude.json")
    } else {
        root.join(".claude.json")
    };
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let account = value.get("oauthAccount")?;
    Some(serde_json::json!({
        "account_id": account.get("accountUuid").and_then(serde_json::Value::as_str)?,
        "organization_id": account.get("organizationUuid").and_then(serde_json::Value::as_str),
        "source": "current_config",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_config_exposes_only_current_identity_hint() {
        let dir = crate::collect::TestDir::new();
        std::fs::write(dir.0.join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"user","organizationUuid":"work","emailAddress":"private@example.invalid","accessToken":"secret"},"other":"private"}"#).unwrap();
        assert_eq!(
            current_account(&dir.0).unwrap(),
            serde_json::json!({"account_id":"user","organization_id":"work","source":"current_config"})
        );
        std::fs::write(dir.0.join(".claude.json"), "{}").unwrap();
        assert!(current_account(&dir.0).is_none());
    }
}
