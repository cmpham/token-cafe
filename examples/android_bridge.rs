//! USB-only, read-only Android companion bridge. No window or approval dispatch.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use token_cafe::collect::Collector;
use token_cafe::scene::{Kind, State, Why};
use token_cafe::usage::Counter;

fn authorized(request: &str, token: &str) -> bool {
    let mut lines = request.split("\r\n");
    if lines.next() != Some("GET /v1/snapshot HTTP/1.1") {
        return false;
    }
    let credentials: Vec<_> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.trim())
        .collect();
    credentials == [format!("Bearer {token}")]
}

fn serve(mut stream: TcpStream, token: &str, snapshot: &RwLock<Vec<u8>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut request = Vec::new();
    while request.len() < 8192 && Instant::now() < deadline {
        let mut byte = [0];
        if stream.read(&mut byte)? == 0 {
            return Ok(());
        }
        request.push(byte[0]);
        if request.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let allowed = request.ends_with(b"\r\n\r\n") && std::str::from_utf8(&request).is_ok_and(|r| authorized(r, token));
    let (status, body) =
        if allowed { ("200 OK", snapshot.read().unwrap().clone()) } else { ("403 Forbidden", b"{}".to_vec()) };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 38417))?;
    let mut random = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
    println!(
        "Token Cafe Android bridge: 127.0.0.1:38417\nPairing token: {token}\nRun: adb reverse tcp:38417 tcp:38417\nEnter the token in the Android app's USB connection dialog.\nThis bridge shares session names, task excerpts and usage with the paired app."
    );
    let initial = br#"{"version":1,"sessions":[],"usage":{"tokens":0,"usd":0}}"#.to_vec();
    let snapshot = Arc::new(RwLock::new(initial));
    let totals = Arc::new(RwLock::new((0.0f64, 0.0f64)));
    let usage_totals = totals.clone();
    std::thread::spawn(move || {
        let mut counter = Counter::new();
        loop {
            let usage = counter.month();
            *usage_totals.write().unwrap() = (usage.tokens as f64, usage.usd);
            std::thread::sleep(Duration::from_secs(60));
        }
    });
    let collected = snapshot.clone();
    std::thread::spawn(move || {
        let mut collector = Collector::new();
        loop {
            let (sessions, details) = collector.sessions_with_details();
            let entries: Vec<_> = sessions
                .iter()
                .filter(|s| !s.id.is_empty() && s.id.len() <= 256)
                .take(256)
                .map(|s| {
                    let task = details
                        .iter()
                        .find(|d| d.id == s.id)
                        .map(|d| d.task.chars().take(160).collect::<String>())
                        .unwrap_or_default();
                    serde_json::json!({
                        "id": s.id, "name": s.name.chars().take(128).collect::<String>(),
                        "kind": match s.kind { Kind::Claude => "claude", Kind::Codex => "codex" },
                        "state": match s.state { State::Work => "work", State::Idle => "idle", State::Wait => "wait" },
                        "why": match s.why { Why::None => "none", Why::Perm => "perm", Why::Ask => "ask" },
                        "age_s": s.age_s, "branch": s.branch.as_ref().map(|b| b.chars().take(64).collect::<String>()),
                        "active_subagents": s.active_subagents, "task": task
                    })
                })
                .collect();
            let (tokens, usd) = *totals.read().unwrap();
            let value = serde_json::json!({"version": 1, "sessions": entries, "usage": {"tokens": tokens, "usd": usd}});
            *collected.write().unwrap() = value.to_string().into_bytes();
            std::thread::sleep(Duration::from_secs(1));
        }
    });
    for connection in listener.incoming() {
        // A single bounded connection at a time avoids unbounded local worker creation.
        let _ = serve(connection?, &token, &snapshot);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_snapshot_route_and_exact_bearer_credential_are_accepted() {
        assert!(authorized("GET /v1/snapshot HTTP/1.1\r\nAuthorization: Bearer sample\r\n\r\n", "sample"));
        for request in [
            "GET /v1/snapshot HTTP/1.1\r\n\r\n",
            "POST /v1/snapshot HTTP/1.1\r\nAuthorization: Bearer sample\r\n\r\n",
            "GET /other HTTP/1.1\r\nAuthorization: Bearer sample\r\n\r\n",
            "GET /v1/snapshot HTTP/1.1\r\nAuthorization: Bearer wrong\r\n\r\n",
            "GET /v1/snapshot HTTP/1.1\r\nAuthorization: Bearer sample\r\nAuthorization: Bearer sample\r\n\r\n",
        ] {
            assert!(!authorized(request, "sample"));
        }
    }

    #[test]
    fn http_response_carries_snapshot_and_denies_unpaired_clients() {
        for (credential, expected) in [("sample", "200 OK"), ("wrong", "403 Forbidden")] {
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
            let address = listener.local_addr().unwrap();
            let worker = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                serve(stream, "sample", &RwLock::new(b"{\"version\":1}".to_vec())).unwrap();
            });
            let mut client = TcpStream::connect(address).unwrap();
            client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            write!(
                client,
                "GET /v1/snapshot HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {credential}\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            worker.join().unwrap();
            assert!(response.starts_with(&format!("HTTP/1.1 {expected}\r\n")));
            assert!(response.contains("Cache-Control: no-store\r\n"));
            assert!(response.ends_with(if credential == "sample" { "{\"version\":1}" } else { "{}" }));
        }
    }
}
