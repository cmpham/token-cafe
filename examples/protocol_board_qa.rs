//! Synthetic v2 board host, deliberately disconnected from real provider adapters.
use companion_core::{crypto, model::Model, sync, wire::*};
use std::{collections::BTreeMap, time::Duration};
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<_> = std::env::args().collect();
    let path = args.get(1).expect("output JSONL path");
    let seconds = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(600);
    let body =
        "Synthetic hardware approval fixture.\nNo command or desktop action is executed.\nTool: Bash\n\nprintf 'test'\n"
            .to_owned() + &"Full question line for scrolling.\n".repeat(20);
    let body = if std::env::var_os("TC_QA_STRESS_TEXT").is_some() { "\t".repeat(32768) } else { body };
    let blob = sync::blob(body.as_bytes()).unwrap();
    let mut sessions = BTreeMap::new();
    for (i, seat) in [0, 1, 4, 5, 2, 3, 6, 7].into_iter().enumerate() {
        let id = format!("qa-{}:{i}", if i < 4 { "claude" } else { "codex" });
        let approval = if i == 0 {
            Some(Request {
                id: "physical-fixture-1".into(),
                digest: crypto::request_digest(
                    "physical-fixture-1",
                    &id,
                    1,
                    "Synthetic approval",
                    "Bash",
                    &blob.sha256.as_slice().try_into().unwrap(),
                    blob.byte_length,
                    &[1, 2],
                )
                .unwrap()
                .to_vec(),
                title: "Synthetic approval".into(),
                tool_name: "Bash".into(),
                body_blob: Some(blob.clone()),
                state: 1,
                allowed_decisions: vec![1, 2],
                ..Default::default()
            })
        } else {
            None
        };
        sessions.insert(
            id.clone(),
            Session {
                id,
                provider: if i < 4 { 1 } else { 2 },
                title: format!("QA PATRON {}", i + 1),
                display_name: format!("QA PATRON {}", i + 1),
                status: if i == 0 { 3 } else { 1 },
                waiting_reason: if i == 0 { 1 } else { 0 },
                seat,
                appearance_seed: 1234 + i as u32,
                task_blob: Some(blob.clone()),
                navigation_available: true,
                approval,
                ..Default::default()
            },
        );
    }
    let model = Model {
        revision: 1,
        sessions,
        view: View { page: 1, ..Default::default() },
        usage: Usage { tokens_display: "11.0B".into(), cost_display: "$5.1K".into() },
        ..Default::default()
    };
    let bodies = BTreeMap::from([(blob.sha256, body.into_bytes())]);
    tokio::runtime::Runtime::new()?.block_on(token_cafe::companion::semantic::exercise_fixture(
        model,
        bodies,
        Duration::from_secs(seconds),
        std::path::Path::new(path),
    ))
}
