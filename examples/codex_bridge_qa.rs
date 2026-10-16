//! Isolated QEMU host fixture. A private socket, no provider or desktop dispatch.
use companion_core::{
    authority::{Dispatch, Peer},
    codec, crypto, sync,
    wire::envelope::Payload,
};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use token_cafe::{
    approval::Broker,
    companion::state::Store,
    scene::{ForcedPose, Kind, Scene, Session, State, Why},
};
fn unhex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn main() {
    let root = std::env::temp_dir().join(format!("tc-codex-qa-{}", std::process::id()));
    let socket = root.join("approval.sock");
    let broker = Broker::listen(socket.clone()).unwrap();
    let mut scene = Scene::new();
    let mut store = Store::new([0x22; 16]);
    let mut peer = Peer::new("qa-qemu".into(), vec![1; 16], true, false);
    println!("{}", json!({"socket":socket}));
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let out = match v["op"].as_str().unwrap() {
            "snapshot" => {
                let requests = broker.requests();
                assert!(requests.iter().all(
                    |r| r.session.starts_with("codex:tc-codex-qa") || r.session.starts_with("claude:tc-claude-qa")
                ));
                let sessions: Vec<_> = requests
                    .iter()
                    .map(|r| Session {
                        id: r.session.clone(),
                        name: "CODEX QA".into(),
                        kind: if r.session.starts_with("claude:") { Kind::Claude } else { Kind::Codex },
                        state: State::Wait,
                        why: if r.choices > 0 { Why::Ask } else { Why::Perm },
                        pose: ForcedPose::Auto,
                        age_s: 0,
                        branch: None,
                        active_subagents: None,
                    })
                    .collect();
                scene.set_sessions(&sessions);
                scene.set_requests(requests.clone());
                store.ingest(&mut scene, Some(&broker));
                let packets: Vec<_> = sync::snapshot(&store.authority.model, &[1; 16], &[0x22; 16])
                    .unwrap()
                    .iter()
                    .map(|e| crypto::hex(&codec::encode(e).unwrap()))
                    .collect();
                json!({"packets":packets,"bodies":requests.iter().map(|r|r.text.clone()).collect::<Vec<_>>()})
            }
            "action" => {
                let env = codec::decode(&unhex(v["hex"].as_str().unwrap()), true).unwrap();
                let Payload::Action(a) = env.payload.unwrap() else { panic!("action required") };
                let body = store.body(&a.blob_id).map(Vec::from);
                let (result, dispatch) =
                    store.authority.action(&mut peer, &a, v["time"].as_u64().unwrap(), body.as_deref()).unwrap();
                let delivered = if let Some(Dispatch::Decision { session, request, decision }) = &dispatch {
                    store.keep_terminal(session);
                    store.decide(request, *decision)
                } else {
                    false
                };
                json!({"status":result.status,"reason":result.reason,"dispatched":dispatch.is_some(),"delivered":delivered})
            }
            "settle" => {
                scene.set_requests(broker.requests());
                store.ingest(&mut scene, Some(&broker));
                let packets: Vec<_> = sync::snapshot(&store.authority.model, &[1; 16], &[0x22; 16])
                    .unwrap()
                    .iter()
                    .map(|e| crypto::hex(&codec::encode(e).unwrap()))
                    .collect();
                json!({"packets":packets})
            }
            "reset_peer" => {
                peer = Peer::new("qa-qemu".into(), vec![1; 16], true, false);
                json!({"ok":true})
            }
            _ => panic!("unknown QA command"),
        };
        println!("{out}");
        std::io::stdout().flush().unwrap();
    }
    let _ = std::fs::remove_file(socket);
    let _ = std::fs::remove_dir(root);
}
