//! A local, non-decision protocol peer: the Mac owns the keypad independently
//! of any BLE board. The shared client keeps navigation/OLED semantics identical.
use super::state::Store;
use companion_core::{
    authority::{Dispatch, Peer},
    client::Client,
    codec,
    crypto::Result,
    model::Model,
    sync,
    wire::{envelope::Payload, *},
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

struct Local {
    client: Client,
    peer: Peer,
    previous: Option<Model>,
}
impl Local {
    fn new(epoch: &[u8], now: u64) -> Result<Self> {
        let mut connection = vec![0; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut connection))
            .map_err(|_| companion_core::crypto::Error::Invalid)?;
        let mut client = Client::new("mac-keypad".into(), connection.clone(), false, now);
        client.outgoing();
        let mut local = Self { client, peer: Peer::new("mac-keypad".into(), connection, false, true), previous: None };
        let e = sync::envelope(
            &local.client.connection,
            epoch,
            Payload::HelloAck(HelloAck {
                asset_digest: cafe_ui::ASSET_DIGEST.to_vec(),
                capabilities: vec![1, 2, 5, 6],
                required_capabilities: vec![1, 2, 6],
                heartbeat_ms: 1000,
                session_limit: 64,
                blob_limit: 32768,
                max_envelope_bytes: 4096,
                ..Default::default()
            }),
        );
        let bytes = codec::encode(&e)?;
        local.client.receive(e, &bytes, now)?;
        Ok(local)
    }
    fn receive(&mut self, p: Payload, now: u64) -> Result<()> {
        let e = sync::envelope(&self.client.connection, &self.client.authority, p);
        let raw = codec::encode(&e)?;
        self.client.receive(e, &raw, now)
    }
    fn update(&mut self, store: &Store, now: u64) -> Result<()> {
        let model = &store.authority.model;
        if self.previous.as_ref().is_none_or(|old| old.revision != model.revision) {
            let messages = match self.previous.as_ref() {
                Some(old) => sync::update(old, model, &self.client.connection, &self.client.authority)?,
                None => sync::snapshot(model, &self.client.connection, &self.client.authority)?,
            };
            for e in messages {
                let raw = codec::encode(&e)?;
                self.client.receive(e, &raw, now)?;
            }
            self.previous = Some(model.clone());
        }
        self.client.tick(now)
    }
    fn drain(&mut self, store: &mut Store, now: u64) -> Result<Vec<(Action, Dispatch)>> {
        let mut dispatches = Vec::new();
        for _ in 0..128 {
            let Some(e) = self.client.outgoing() else { return Ok(dispatches) };
            match e.payload {
                Some(Payload::BlobGet(b)) => {
                    if let Some(body) = store.body(&b.sha256) {
                        let chunks = sync::chunks(body)?;
                        for c in chunks {
                            self.receive(Payload::BlobChunk(c), now)?;
                        }
                    } else {
                        self.receive(Payload::BlobUnavailable(BlobUnavailable { sha256: b.sha256, reason: 2 }), now)?;
                    }
                }
                Some(Payload::Action(a)) => {
                    let body = store.body(&a.blob_id).map(Vec::from);
                    let (r, d) = store.authority.action(&mut self.peer, &a, now, body.as_deref())?;
                    self.receive(Payload::ActionResult(r), now)?;
                    if let Some(d) = d {
                        dispatches.push((a, d));
                    }
                    self.update(store, now)?;
                }
                _ => (),
            }
        }
        Err(companion_core::crypto::Error::Limit)
    }
}

pub fn start(store: Arc<Mutex<Store>>, status: Arc<Mutex<String>>) {
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        loop {
            if let Err(e) = run(store.clone(), status.clone()) {
                eprintln!("Keypad transport: {e}");
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (store, status);
}
fn run(shared: Arc<Mutex<Store>>, status: Arc<Mutex<String>>) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let helper = executable.parent().ok_or("No helper directory")?.join("keypad-hid");
    let mut child = Command::new(helper).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
    let result = (|| {
        let mut input = child.stdin.take().ok_or("No keypad display pipe")?;
        let output = child.stdout.take().ok_or("No keypad input pipe")?;
        let (tx, rx) = mpsc::sync_channel(32);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(std::result::Result::ok) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(cmd) = value["command"].as_u64().filter(|v| (1..=11).contains(v)) {
                        // Drop overload rather than replay old key presses later.
                        let _ = tx.try_send((cmd as u8, Instant::now()));
                    } else if let Some(event) = value["status"].as_str() {
                        eprintln!("Keypad: {event}");
                        let mut state = status.lock().unwrap();
                        match event {
                            "input-permission-required" => *state = "allow Input Monitoring".into(),
                            "connected" if value["interface"] == "keys" => *state = "USB connected".into(),
                            "oled-ready" => *state = "USB + OLED ready".into(),
                            "usb-bluetooth-ready" => *state = "USB + OLED ready; Bluetooth standby".into(),
                            "bluetooth-ready" => *state = "Bluetooth + OLED ready".into(),
                            "bluetooth-connecting" => *state = "looking for Bluetooth keypad".into(),
                            "bluetooth-permission-required" => *state = "allow Bluetooth".into(),
                            "bluetooth-unavailable" => *state = "connect USB or enable Bluetooth".into(),
                            "disconnected" => *state = "connect USB".into(),
                            "firmware-required" => *state = "update keypad firmware".into(),
                            "usb-open-failed" => *state = "USB device unavailable".into(),
                            _ => (),
                        }
                    }
                }
            }
        });
        let start = Instant::now();
        let protocol = |e| format!("local keypad protocol: {e:?}");
        let mut local = Local::new(&shared.lock().unwrap().epoch, 0).map_err(protocol)?;
        let mut pending: Option<(Action, mpsc::Receiver<bool>, Instant)> = None;
        let mut display = None;
        loop {
            if child.try_wait()?.is_some() {
                return Err("keypad helper exited".into());
            }
            let now = start.elapsed().as_millis() as u64;
            let mut store = shared.lock().unwrap();
            local.update(&store, now).map_err(protocol)?;
            if let Some((a, rx, at)) = &pending {
                if let Some(ok) =
                    rx.try_recv().ok().or_else(|| (at.elapsed() > Duration::from_secs(15)).then_some(false))
                {
                    let r = store.authority.finish(&mut local.peer, a, ok);
                    local.receive(Payload::ActionResult(r), now).map_err(protocol)?;
                    eprintln!("Keypad desktop action={} accepted={ok}", a.kind);
                    pending = None;
                }
            }
            for (cmd, at) in rx.try_iter().take(32) {
                if at.elapsed() > Duration::from_millis(500) {
                    continue;
                }
                store.keypad_activity = store.keypad_activity.wrapping_add(1);
                local.client.keyboard(cmd, now).map_err(protocol)?;
            }
            let dispatches = local.drain(&mut store, now).map_err(protocol)?;
            for (a, d) in dispatches {
                if pending.is_some() {
                    let r = store.authority.finish(&mut local.peer, &a, false);
                    local.receive(Payload::ActionResult(r), now).map_err(protocol)?;
                    continue;
                }
                let url = match &d {
                    Dispatch::Voice(id) | Dispatch::Desktop(id) => store.targets.get(id).cloned(),
                    _ => None,
                };
                let title = match &d {
                    Dispatch::Voice(id) => super::voice_session_title(&store.authority.model, id),
                    _ => String::new(),
                };
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let ok = match d {
                        Dispatch::App { codex } => super::open_desktop_app(codex),
                        Dispatch::ToggleApp => super::keypad_desktop(&["toggle"]),
                        Dispatch::Voice(_) => url.is_some_and(|u| super::keypad_desktop(&["voice", &u, &title])),
                        Dispatch::Desktop(_) => url
                            .is_some_and(|u| Command::new("/usr/bin/open").arg(u).status().is_ok_and(|s| s.success())),
                        Dispatch::Decision { .. } => false,
                    };
                    let _ = tx.send(ok);
                });
                pending = Some((a, rx, Instant::now()));
            }
            let rows = local.client.keyboard_display();
            drop(store);
            if display != Some(rows) {
                let strings: Vec<_> = rows.iter().map(|r| String::from_utf8_lossy(r).into_owned()).collect();
                writeln!(input, "{}", serde_json::json!({"rows":strings}))?;
                display = Some(rows);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standalone_keypad_browses_selects_and_dispatches_without_board() {
        let mut store = Store::new([1; 16]);
        store.authority.model.sessions.insert(
            "codex:a".into(),
            Session {
                id: "codex:a".into(),
                title: "AGENT A".into(),
                display_name: "AGENT A".into(),
                provider: 2,
                status: 2,
                seat: 2,
                navigation_available: true,
                task_blob: Some(sync::blob(b"Task details").unwrap()),
                ..Default::default()
            },
        );
        store.authority.model.sessions.insert(
            "claude:b".into(),
            Session {
                id: "claude:b".into(),
                title: "AGENT B".into(),
                provider: 1,
                status: 3,
                seat: 1,
                navigation_available: true,
                ..Default::default()
            },
        );
        let mut local = Local::new(&store.epoch, 0).unwrap();
        local.update(&store, 0).unwrap();
        local.drain(&mut store, 0).unwrap();
        local.client.keyboard(10, 10).unwrap();
        assert!(local.drain(&mut store, 10).unwrap().is_empty());
        assert_eq!(store.authority.model.view.selected_session_id, "codex:a");
        assert!(String::from_utf8_lossy(&local.client.keyboard_display()[1]).contains("AGENT A"));
        local.client.keyboard(6, 1000).unwrap();
        let d = local.drain(&mut store, 1000).unwrap();
        assert_eq!(d[0].1, Dispatch::Voice("codex:a".into()));
        let result = store.authority.finish(&mut local.peer, &d[0].0, true);
        local.receive(Payload::ActionResult(result), 1100).unwrap();
        local.client.keyboard(4, 1200).unwrap();
        local.drain(&mut store, 1200).unwrap();
        assert_eq!(store.authority.model.view.page, 2);
        local.client.keyboard(9, 2200).unwrap();
        let open = local.drain(&mut store, 2200).unwrap();
        assert_eq!(open[0].1, Dispatch::Desktop("codex:a".into()));
        local.client.keyboard(11, 2300).unwrap();
        local.drain(&mut store, 2300).unwrap();
        assert_eq!(store.authority.model.view.selected_session_id, "claude:b");
        assert_eq!(store.authority.model.view.page, 1);
        local.client.keyboard(7, 3300).unwrap();
        assert_eq!(local.drain(&mut store, 3300).unwrap()[0].1, Dispatch::App { codex: false });
        assert_eq!(store.authority.model.view.selected_session_id, "claude:b");
    }
}
