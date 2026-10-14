//! Live permission requests. Only a still-open provider connection can decide.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

pub use cafe_ui::approval::{Decision, Request, lines};
struct Pending {
    request: Request,
    reply: mpsc::Sender<Option<Decision>>,
    created: Instant,
}
#[derive(Clone, Default)]
pub struct Broker {
    pending: Arc<Mutex<BTreeMap<String, Pending>>>,
    outcomes: Arc<Mutex<BTreeMap<String, (bool, Instant)>>>,
}
impl Broker {
    pub fn requests(&self) -> Vec<Request> {
        let pending = self.pending.lock().unwrap();
        let mut ordered: Vec<_> = pending.values().collect();
        ordered.sort_by_key(|p| p.created);
        ordered.into_iter().map(|p| p.request.clone()).collect()
    }
    /// Remove before delivery: repeated touches cannot answer twice.
    pub fn decide(&self, id: &str, decision: Decision) -> bool {
        self.pending.lock().unwrap().remove(id).is_some_and(|p| p.reply.send(Some(decision)).is_ok())
    }
    pub fn cancel(&self, id: &str) {
        if let Some(p) = self.pending.lock().unwrap().remove(id) {
            let _ = p.reply.send(None);
        }
    }
    pub fn take_outcome(&self, id: &str) -> Option<bool> {
        self.outcomes.lock().unwrap().remove(id).map(|(ok, _)| ok)
    }
    pub fn start() -> std::io::Result<Self> {
        let home = std::env::var_os("HOME").ok_or_else(|| std::io::Error::other("HOME unavailable"))?;
        Self::listen(PathBuf::from(home).join(".token-cafe/approval.sock"))
    }
    pub fn listen(path: PathBuf) -> std::io::Result<Self> {
        let parent = path.parent().ok_or_else(|| std::io::Error::other("No socket parent"))?;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
        let meta = std::fs::symlink_metadata(parent)?;
        if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
            return Err(std::io::Error::other("Approval directory must be private (0700)"));
        }
        if path.exists() {
            if UnixStream::connect(&path).is_ok() {
                return Err(std::io::Error::other("Approval broker already running"));
            }
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let broker = Self::default();
        let worker = broker.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let worker = worker.clone();
                std::thread::spawn(move || {
                    let _ = worker.handle(stream);
                });
            }
        });
        Ok(broker)
    }
    fn handle(&self, mut stream: UnixStream) -> std::io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut line = String::new();
        BufReader::new(stream.try_clone()?).take(65537).read_line(&mut line)?;
        if line.len() > 65536 || !line.ends_with('\n') {
            return Ok(());
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { return Ok(()) };
        let Some(session) = v["session_id"].as_str().filter(|s| !s.is_empty() && s.len() < 200) else { return Ok(()) };
        let Some(tool) = v["tool_name"].as_str().filter(|s| !s.is_empty()) else { return Ok(()) };
        if v["hook_event_name"] != "PermissionRequest" || !v["tool_input"].is_object() {
            return Ok(());
        }
        let mut random = [0; 16];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let id: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let request = Request {
            id: id.clone(),
            session: format!("claude:{session}"),
            title: format!("ALLOW {tool}?"),
            tool_name: tool.to_owned(),
            text: format!(
                "{}\n\nWorking directory: {}\nClaude session: {session}",
                serde_json::to_string_pretty(&v["tool_input"])?,
                v["cwd"].as_str().unwrap_or("Unknown")
            ),
        };
        stream.set_nonblocking(true)?;
        let (tx, rx) = mpsc::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            if pending.len() >= 32 {
                return Ok(());
            }
            pending.insert(id.clone(), Pending { request, reply: tx, created: Instant::now() });
        }
        let start = Instant::now();
        let mut decision = None;
        while start.elapsed() < Duration::from_secs(590) {
            if let Ok(d) = rx.try_recv() {
                decision = d;
                break;
            }
            let mut byte = [0];
            match stream.read(&mut byte) {
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                _ => break, // closed/cancelled provider: invalidate immediately
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.pending.lock().unwrap().remove(&id);
        stream.set_nonblocking(false)?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let result = match decision {
            Some(d) => {
                json!({"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":if d == Decision::Allow {"allow"} else {"deny"}}}})
            }
            None => json!({}), // native provider prompt remains authoritative
        };
        let delivered = writeln!(stream, "{result}").is_ok();
        if decision.is_some() {
            let mut outcomes = self.outcomes.lock().unwrap();
            outcomes.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(600));
            if outcomes.len() >= 128
                && let Some(oldest) = outcomes.iter().min_by_key(|(_, (_, at))| *at).map(|(id, _)| id.clone())
            {
                outcomes.remove(&oldest);
            }
            outcomes.insert(id, (delivered, Instant::now()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decisions_are_exactly_once_and_never_guess_a_request() {
        let broker = Broker::default();
        let (tx, rx) = mpsc::channel();
        broker.pending.lock().unwrap().insert(
            "one".into(),
            Pending {
                request: Request {
                    id: "one".into(),
                    session: "claude:a".into(),
                    title: "Test".into(),
                    tool_name: "test".into(),
                    text: "text".into(),
                },
                reply: tx,
                created: Instant::now(),
            },
        );
        assert!(!broker.decide("other", Decision::Allow));
        assert!(broker.decide("one", Decision::Deny));
        assert!(!broker.decide("one", Decision::Allow));
        assert_eq!(rx.recv().unwrap(), Some(Decision::Deny));
    }
    #[test]
    fn hook_round_trip_and_disconnect_cleanup() {
        for answer in [Some(Decision::Allow), Some(Decision::Deny), None] {
            let broker = Broker::default();
            let (mut client, server) = UnixStream::pair().unwrap();
            let worker = broker.clone();
            let handle = std::thread::spawn(move || worker.handle(server).unwrap());
            writeln!(client, "{}", json!({"hook_event_name":"PermissionRequest","session_id":"test-session","tool_name":"Bash","tool_input":{"command":"true"},"cwd":"/tmp"})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while broker.requests().is_empty() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            let request = broker.requests().pop().expect("pending request");
            assert_eq!(request.session, "claude:test-session");
            if let Some(answer) = answer {
                assert!(broker.decide(&request.id, answer));
            } else {
                broker.cancel(&request.id);
            }
            let mut line = String::new();
            BufReader::new(client).read_line(&mut line).unwrap();
            let value: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(
                value["hookSpecificOutput"]["decision"]["behavior"].as_str(),
                answer.map(|d| if d == Decision::Allow { "allow" } else { "deny" })
            );
            handle.join().unwrap();
            assert!(broker.requests().is_empty());
        }
        let broker = Broker::default();
        let worker = broker.clone();
        let (mut client, server) = UnixStream::pair().unwrap();
        let handle = std::thread::spawn(move || {
            let _ = worker.handle(server);
        });
        writeln!(
            client,
            "{}",
            json!({"hook_event_name":"PermissionRequest","session_id":"closed","tool_name":"Bash","tool_input":{}})
        )
        .unwrap();
        drop(client);
        handle.join().unwrap();
        assert!(broker.requests().is_empty());
    }

    #[test]
    fn literal_text_keeps_case_and_exposes_hidden_characters() {
        assert_eq!(lines("aB\u{202e}\nxy", 80), vec!["aB\\u{202e}", "xy"]);
    }
}
