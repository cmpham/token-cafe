//! Deterministic visual QA of the real renderer and BLE RGB565 encoder.
//! cargo run --example companion_qa (writes target/companion-qa)
#[cfg(feature = "bluetooth")]
fn main() {
    use std::{
        fs::{self, File},
        io::BufWriter,
    };
    use token_cafe::{
        assets::{H, W},
        companion::{HEIGHT, WIDTH, encode_frame},
        scene::*,
    };
    let dir = "target/companion-qa";
    fs::create_dir_all(dir).unwrap();
    let mut frames: Vec<(String, Vec<u8>)> = Vec::new();
    let mut shot = |name: &str, s: &Scene| {
        let mut pixels = vec![0; W * H];
        s.render(&mut pixels, chrono::NaiveTime::from_hms_opt(20, 0, 0).unwrap());
        let raw = encode_frame(&pixels);
        fs::write(format!("{dir}/{name}.rgb565"), &raw).unwrap();
        let rgb: Vec<u8> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|v| {
                let p = u16::from_le_bytes([v[0], v[1]]);
                let (r, g, b) = (p >> 11, (p >> 5) & 63, p & 31);
                [(r << 3 | r >> 2) as u8, (g << 2 | g >> 4) as u8, (b << 3 | b >> 2) as u8]
            })
            .collect();
        frames.push((name.into(), rgb));
    };
    let mut s = Scene::new();
    shot("01-empty", &s);
    let mut sessions: Vec<_> = (0..10)
        .map(|i| Session {
            id: format!("qa-{i}"),
            name: format!("project-{i}"),
            kind: if i % 4 < 2 { Kind::Claude } else { Kind::Codex },
            state: State::Work,
            why: Why::None,
            pose: ForcedPose::Auto,
            age_s: 0,
            branch: Some(format!("feature/task-{i}")),
            active_subagents: Some(i),
        })
        .collect();
    s.set_sessions(&sessions[..8]);
    s.set_details(sessions.iter().enumerate().map(|(i,v)| SessionDetail {
        id:v.id.clone(), target:None,
        task: if i==2 {"".into()} else if i==3 {"Unicode café 日本語 👋 and SupercalifragilisticexpialidociousABCDEFGHIJKLMNOPQRSTUVWXYZ".into()} else {format!("Patron {i}: Implement the Bluetooth companion display. Check selection, scrolling, attention, and final page boundaries carefully.")},
    }).collect());
    for _ in 0..60 {
        s.tick(33);
    }
    shot("02-eight-working", &s);
    s.click(60, 80);
    shot("03-patron-selected", &s);
    s.page_task_key(true);
    shot("04-down-second-page", &s);
    s.scroll_task_to(271);
    shot("05-drag-last-page", &s);
    s.page_task_key(true);
    shot("06-down-clamped", &s);
    s.scroll_task_to(224);
    shot("07-drag-first-page", &s);
    s.page_task_key(false);
    shot("08-up-clamped", &s);
    s.click(210, 252);
    shot("09-banner-next-patron", &s);
    s.select_patron(false);
    shot("10-left-previous", &s);
    s.click(60, 80);
    shot("11-toggle-off", &s);
    s.page_task_key(false);
    shot("12-up-opens-focus", &s);
    s.focus_patron("qa-2".into());
    shot("13-task-unavailable", &s);
    s.focus_patron("qa-3".into());
    shot("14-unicode-long-word", &s);
    s.scroll_task_to(271);
    shot("15-unicode-last-page", &s);
    sessions[1].state = State::Wait;
    sessions[1].why = Why::Perm;
    sessions[5].state = State::Wait;
    sessions[5].why = Why::Ask;
    s.set_sessions(&sessions[..8]);
    s.clear_selection();
    shot("16-attention", &s);
    s.focus_patron("qa-0".into());
    shot("17-manual-with-waiting", &s);
    s.clear_selection();
    for _ in 0..120 {
        s.tick(33);
    }
    shot("18-attention-rotated", &s);
    sessions[1].state = State::Idle;
    sessions[5].state = State::Idle;
    sessions[1].age_s = 600;
    sessions[5].age_s = 600;
    s.set_sessions(&sessions[..8]);
    s.focus_patron("qa-1".into());
    shot("19-idle-last-task", &s);
    s.set_sessions(&sessions);
    s.focus_patron("qa-9".into());
    shot("20-overflow-selected", &s);
    s.set_sessions(&sessions[..8]);
    shot("21-selected-removed", &s);
    s.set_sessions(&[]);
    for _ in 0..300 {
        s.tick(33);
    }
    shot("22-all-closed", &s);
    s = Scene::new();
    sessions[0].name = "very-long-project-name-with-suffix-1c".into();
    s.set_sessions(&sessions[..1]);
    s.set_usage(Usage { tokens: 987654321.0, usd: 12345.67 });
    s.set_details(vec![SessionDetail { id: "qa-0".into(), target: None, task: "One short task".into() }]);
    s.focus_patron("qa-0".into());
    shot("23-long-name-usage-short-task", &s);
    s.set_details(vec![SessionDetail {
        id: "qa-0".into(),
        target: None,
        task: "Updated task content should reset to its first page and stay on this same patron.".into(),
    }]);
    s.scroll_task_to(271);
    s.set_details(vec![SessionDetail { id: "qa-0".into(), target: None, task: "Replacement task".into() }]);
    shot("24-task-replaced", &s);
    // Record the complete swap: Claude 3 -> 1 and Codex 4 -> 1.
    let mut swap = Scene::new();
    for patron in &mut sessions {
        patron.state = State::Idle;
    }
    swap.set_sessions(&sessions[..8]);
    swap.focus_patron("qa-4".into());
    shot("25-swap-before", &swap);
    sessions[4].state = State::Work;
    sessions[7].state = State::Wait;
    swap.set_sessions(&sessions[..8]);
    for i in 0..32 {
        swap.tick(125);
        shot(&format!("26-swap-{i:02}"), &swap);
    }
    swap.tick(10000);
    shot("27-swap-after", &swap);
    // Equivalent interactions must preserve both selection and every displayed pixel.
    for (a, b) in [(4, 5), (6, 7), (2, 6), (2, 9), (2, 11)] {
        assert_eq!(frames[a].1, frames[b].1, "{} versus {}", frames[a].0, frames[b].0);
    }
    assert_ne!(frames[2].1, frames[3].1, "Down must change task text");
    assert_ne!(frames[2].1, frames[8].1, "Banner must change the selected patron");
    let write_png = |path: &str, w: usize, h: usize, rgb: &[u8]| {
        let mut enc = png::Encoder::new(BufWriter::new(File::create(path).unwrap()), w as u32, h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(rgb).unwrap();
    };
    for (name, rgb) in &frames {
        write_png(&format!("{dir}/{name}.png"), WIDTH, HEIGHT, rgb);
    }
    for (n, chunk) in frames.chunks(6).enumerate() {
        let mut rgb = vec![32; WIDTH * 2 * HEIGHT * 3 * 3];
        for (i, (_, f)) in chunk.iter().enumerate() {
            for y in 0..HEIGHT {
                let start = ((i / 2 * HEIGHT + y) * WIDTH * 2 + i % 2 * WIDTH) * 3;
                rgb[start..start + WIDTH * 3].copy_from_slice(&f[y * WIDTH * 3..(y + 1) * WIDTH * 3]);
            }
        }
        write_png(&format!("{dir}/sheet-{}.png", n + 1), WIDTH * 2, HEIGHT * 3, &rgb);
    }
    fs::write(format!("{dir}/index.txt"), frames.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join("\n"))
        .unwrap();
    println!("Rendered {} scenarios to {dir}", frames.len());
}
#[cfg(not(feature = "bluetooth"))]
fn main() {
    eprintln!("companion_qa requires the bluetooth feature");
}
