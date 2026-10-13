//! Deterministic hardware-in-the-loop host using the production Mirror and Scene.
#[cfg(feature = "bluetooth")]
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use serde_json::{Value, json};
    use std::{
        io::{self, BufRead},
        sync::mpsc,
        time::Duration,
    };
    use token_cafe::{
        companion::{Mirror, encode_frame, read_key},
        scene::*,
    };
    fn fixture(v: &Value) -> Scene {
        let count = v["count"].as_u64().unwrap_or(8) as usize;
        let mut s = Scene::new();
        let sessions: Vec<_> = (0..count)
            .map(|i| Session {
                id: format!("qa-{i}"),
                name: if v["long"].as_bool() == Some(true) {
                    format!("very-long-project-name-{i}")
                } else {
                    format!("project-{i}")
                },
                kind: if i % 2 == 0 { Kind::Claude } else { Kind::Codex },
                state: if v["attention"].as_bool() == Some(true) && (i == 1 || i == 5) {
                    State::Wait
                } else {
                    State::Work
                },
                why: if (i == 1 || i == 5) && v["attention"].as_bool() == Some(true) { Why::Perm } else { Why::None },
                pose: ForcedPose::Auto,
                age_s: 0,
                branch: Some(format!("feature/task-{i}")),
                active_subagents: Some(i),
            })
            .collect();
        s.set_sessions(&sessions);
        s.set_details(sessions.iter().enumerate().map(|(i,p)| SessionDetail {id:p.id.clone(),target: if v["targets"].as_bool()==Some(true) && i<2 {Some(if i==0 {SessionTarget::Claude("local_qa".into())} else {SessionTarget::Codex("qa-codex".into())})} else {None},
            task:if i==2 {String::new()} else if i==3 {"Unicode café 日本語 👋 and SupercalifragilisticexpialidociousABCDEFGHIJKLMNOPQRSTUVWXYZ".into()}
            else {format!("Patron {i}: Implement the Bluetooth companion display. Check selection, scrolling, attention, and final page boundaries carefully.")}
        }).collect());
        s.set_usage(Usage { tokens: 987654321.0, usd: 12345.67 });
        for _ in 0..60 {
            s.tick(33);
        }
        s
    }
    fn action(s: &mut Scene, v: &Value) {
        match v["action"].as_str().unwrap_or("") {
            "click" => s.click(v["x"].as_i64().unwrap() as i32, v["y"].as_i64().unwrap() as i32),
            "scroll" => s.scroll_task_to(v["y"].as_i64().unwrap() as i32),
            "next" => s.select_next(),
            "focus" => s.focus_patron(v["id"].as_str().unwrap().into()),
            "clear" => s.clear_selection(),
            "replace" => {
                s.set_details(vec![SessionDetail { id: "qa-0".into(), target: None, task: "Replacement task".into() }])
            }
            "remove" => s.set_sessions(&[]),
            "tick" => {
                for _ in 0..v["steps"].as_u64().unwrap_or(120) {
                    s.tick(33);
                }
            }
            _ => {}
        }
    }
    fn frame(s: &Scene) -> Vec<u8> {
        let mut pixels = vec![0; 320 * 240];
        s.render(&mut pixels, chrono::NaiveTime::from_hms_opt(20, 0, 0).unwrap());
        encode_frame(&pixels)
    }
    let dir = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&dir)?;
    let key = read_key("target/esp/ble-pairing-key")?;
    let mut mirror = Mirror::start(key);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut actual = fixture(&json!({}));
    let mut expected = fixture(&json!({}));
    let mut inputs = 0u64;
    let mut activations: Vec<String> = Vec::new();
    println!("{}", json!({"ready":true}));
    loop {
        for event in mirror.drain_input() {
            if let Some(url) = event.apply(&mut actual) {
                activations.push(url);
            }
            inputs += 1;
        }
        while let Ok(line) = rx.try_recv() {
            let v: Value = serde_json::from_str(&line)?;
            match v["cmd"].as_str().unwrap_or("") {
                "reset" => {
                    activations.clear();
                    actual = fixture(&v);
                    expected = fixture(&v);
                }
                "expect" => action(&mut expected, &v),
                "both" => {
                    action(&mut actual, &v);
                    action(&mut expected, &v);
                }
                "quit" => return Ok(()),
                _ => {}
            }
            let a = frame(&actual);
            let e = frame(&expected);
            std::fs::write(format!("{dir}/host.rgb565"), &a)?;
            std::fs::write(format!("{dir}/expected.rgb565"), &e)?;
            println!(
                "{}",
                json!({"id":v["request"],"inputs":inputs,"actual":format!("{:08x}",crc32fast::hash(&a)),"expected":format!("{:08x}",crc32fast::hash(&e)),"status":mirror.status(),"activations":activations})
            );
        }
        let mut pixels = vec![0; 320 * 240];
        actual.render(&mut pixels, chrono::NaiveTime::from_hms_opt(20, 0, 0).unwrap());
        mirror.publish(&pixels);
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[cfg(not(feature = "bluetooth"))]
fn main() {
    eprintln!("board_e2e requires bluetooth");
}
