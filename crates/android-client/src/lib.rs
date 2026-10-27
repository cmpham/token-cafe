//! Android owns a local scene; only session snapshots cross the USB bridge.
use cafe_ui::assets::{H, W};
use cafe_ui::scene::{ForcedPose, Kind, Scene, Session, SessionDetail, State, Usage, Why};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

pub const MAX_SNAPSHOT: usize = 1024 * 1024;

/// Validate the entire snapshot before changing the scene.
pub fn apply_snapshot(scene: &mut Scene, bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.len() > MAX_SNAPSHOT {
        return Err("snapshot too large");
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "invalid JSON")?;
    if value["version"] != 1 {
        return Err("unsupported snapshot version");
    }
    let entries = value["sessions"].as_array().filter(|v| v.len() <= 256).ok_or("invalid sessions")?;
    let mut sessions = Vec::new();
    let mut details = Vec::new();
    for entry in entries {
        let text = |key: &str, max| -> Result<String, &'static str> {
            entry[key].as_str().filter(|v| v.len() <= max).map(String::from).ok_or("invalid text")
        };
        let id = text("id", 256)?;
        if id.is_empty() || sessions.iter().any(|s: &Session| s.id == id) {
            return Err("invalid session identity");
        }
        let kind = match entry["kind"].as_str() {
            Some("claude") => Kind::Claude,
            Some("codex") => Kind::Codex,
            _ => return Err("invalid kind"),
        };
        let state = match entry["state"].as_str() {
            Some("work") => State::Work,
            Some("idle") => State::Idle,
            Some("wait") => State::Wait,
            _ => return Err("invalid state"),
        };
        let why = match entry["why"].as_str() {
            Some("none") => Why::None,
            Some("perm") => Why::Perm,
            Some("ask") => Why::Ask,
            _ => return Err("invalid wait reason"),
        };
        details.push(SessionDetail { id: id.clone(), task: text("task", 4096)?, target: None });
        sessions.push(Session {
            id,
            name: text("name", 512)?,
            kind,
            state,
            why,
            pose: ForcedPose::Auto,
            age_s: entry["age_s"].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("invalid age")?,
            branch: entry["branch"].as_str().map(|s| s.chars().take(256).collect()),
            active_subagents: entry["active_subagents"].as_u64().and_then(|v| usize::try_from(v).ok()),
        });
    }
    let tokens = value["usage"]["tokens"].as_f64().filter(|v| *v >= 0.0).ok_or("invalid tokens")?;
    let usd = value["usage"]["usd"].as_f64().filter(|v| *v >= 0.0).ok_or("invalid cost")?;
    scene.set_sessions(&sessions);
    scene.set_details(details);
    scene.set_usage(Usage { tokens, usd });
    Ok(())
}

fn scene() -> &'static Mutex<Scene> {
    static SCENE: OnceLock<Mutex<Scene>> = OnceLock::new();
    SCENE.get_or_init(|| Mutex::new(Scene::new()))
}

#[unsafe(no_mangle)]
pub extern "C" fn tc_reset(demo: bool) {
    let mut current = scene().lock().unwrap();
    *current = Scene::new();
    if demo {
        let sessions: Vec<_> = (0..8)
            .map(|i| serde_json::json!({
                "id": format!("demo-{i}"), "name": (["api-server", "checkout", "docs", "migration", "infra", "web-app", "billing", "pipeline"][i]),
                "kind": if i < 4 { "claude" } else { "codex" },
                "state": (["work", "wait", "work", "idle"][i % 4]),
                "why": if i % 4 == 1 { "ask" } else { "none" },
                "age_s": if i % 4 == 3 { 700 } else { 0 },
                "branch": "feature/moon-cafe", "active_subagents": 2,
                "task": "Review the preview environment and verify retries before continuing."
            }))
            .collect();
        let fixture =
            serde_json::json!({"version": 1, "sessions": sessions, "usage": {"tokens": 14900000, "usd": 6.5}});
        let _ = apply_snapshot(&mut current, fixture.to_string().as_bytes());
    }
}

/// # Safety
/// `bytes` must point to `len` readable bytes for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tc_snapshot(bytes: *const u8, len: usize) -> bool {
    if bytes.is_null() || len > MAX_SNAPSHOT {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
    apply_snapshot(&mut scene().lock().unwrap(), bytes).is_ok()
}

/// # Safety
/// `pixels` must point to `len` writable u32 values, with no concurrent access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tc_render(pixels: *mut u32, len: usize, hour: u32, minute: u32, second: u32) {
    if pixels.is_null() || len != W * H {
        return;
    }
    let Some(now) = chrono::NaiveTime::from_hms_opt(hour, minute, second) else { return };
    let mut scene = scene().lock().unwrap();
    scene.tick(33);
    let pixels = unsafe { std::slice::from_raw_parts_mut(pixels, len) };
    scene.render(pixels, now);
    for pixel in pixels {
        *pixel |= 0xff000000;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tc_touch(x: i32, y: i32, scroll: bool) {
    if !(0..W as i32).contains(&x) || !(0..H as i32).contains(&y) {
        return;
    }
    let mut scene = scene().lock().unwrap();
    if scroll && scene.scrollbar_hit(x, y) {
        scene.scroll_task_to(y);
    } else if !scroll {
        scene.click(x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_snapshot_without_replacing_current_sessions() {
        let mut scene = Scene::new();
        let valid = serde_json::json!({"version": 1, "sessions": [{
            "id": "sample", "name": "sample-project", "kind": "codex", "state": "work",
            "why": "none", "age_s": 3, "task": "Verify the preview"
        }], "usage": {"tokens": 100, "usd": 0.5}});
        apply_snapshot(&mut scene, valid.to_string().as_bytes()).unwrap();
        let mut invalid = valid.clone();
        invalid["sessions"][0]["state"] = "unknown".into();
        assert!(apply_snapshot(&mut scene, invalid.to_string().as_bytes()).is_err());
        assert_eq!(scene.sessions()[0].id, "sample");
        assert!(apply_snapshot(&mut scene, b"{}").is_err());
        assert!(apply_snapshot(&mut scene, &vec![0; MAX_SNAPSHOT + 1]).is_err());
        let empty = serde_json::json!({"version": 1, "sessions": [], "usage": {"tokens": 0, "usd": 0}});
        apply_snapshot(&mut scene, empty.to_string().as_bytes()).unwrap();
        assert!(scene.sessions().is_empty());
    }
}
