//! Read-only desktop unread state. Never merge accounts or remote hosts.
use serde_json::Value;
use std::{collections::HashSet, fs::File, io::Read, path::Path};
const LIMIT: u64 = 4 * 1024 * 1024;
fn parse(bytes: &[u8]) -> Option<HashSet<String>> {
    let root: Value = serde_json::from_slice(bytes).ok()?;
    let state = root.get("electron-thread-read-state-v1")?;
    if state.get("version")?.as_u64()? != 1 {
        return None;
    }
    let identities = state.get("unreadByIdentity")?.as_object()?;
    // Multiple signed-in identities cannot be attributed without additional evidence.
    if identities.len() != 1 {
        return None;
    }
    let hosts = identities.values().next()?.as_object()?;
    let mut local = hosts.iter().filter(|(key, _)| key.starts_with("local:"));
    let (_, threads) = local.next()?;
    if local.next().is_some() {
        return None;
    }
    let threads = threads.as_array()?;
    if threads.len() > 10000 {
        return None;
    }
    let mut out = HashSet::new();
    for item in threads {
        let id = item.as_str()?;
        if id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return None;
        }
        out.insert(id.into());
    }
    Some(out)
}
pub(super) fn load(home: &Path) -> Option<HashSet<String>> {
    let file = File::open(home.join(".codex-global-state.json")).ok()?;
    if file.metadata().ok()?.len() > LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > LIMIT {
        return None;
    }
    parse(&bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn state(identities: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"electron-thread-read-state-v1":{"version":1,"unreadByIdentity":identities}}))
            .unwrap()
    }
    #[test]
    fn local_unread_survives_startup_and_remote_ids_are_excluded() {
        let s = parse(&state(json!({"account":{"local:machine":["unread-id"],"remote:host":["remote-id"]}}))).unwrap();
        assert!(s.contains("unread-id"));
        assert!(!s.contains("remote-id"));
    }
    #[test]
    fn empty_local_list_is_known_read() {
        assert_eq!(parse(&state(json!({"account":{"local:machine":[]}}))), Some(HashSet::new()));
    }
    #[test]
    fn ambiguous_and_invalid_state_is_unknown() {
        for ids in [
            json!({"a":{"local:x":[]},"b":{"local:y":[]}}),
            json!({"a":{"local:x":[],"local:y":[]}}),
            json!({"a":{"remote:x":[]}}),
            json!({"a":{"local:x":[7]}}),
        ] {
            assert!(parse(&state(ids)).is_none());
        }
        assert!(parse(b"invalid").is_none());
    }
    #[test]
    fn unsupported_version_is_unknown() {
        assert!(
            parse(br#"{"electron-thread-read-state-v1":{"version":2,"unreadByIdentity":{"a":{"local:x":[]}}}}"#)
                .is_none()
        );
    }
}
