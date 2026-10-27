//! Visual QA: every creature/action pair and timestamped arrival/departure frames.
//! cargo run --example patron_qa
use chrono::NaiveTime;
use std::{
    fs::{self, File},
    io::BufWriter,
};
use token_cafe::{
    assets::{H, W},
    patrons::{NAMES, Patrons},
    scene::{ForcedPose, Kind, Scene, Session, State, Usage, Why},
};
fn save(scene: &Scene, path: &str) {
    let mut fb = vec![0; W * H * 4];
    scene.render(&mut fb, NaiveTime::from_hms_opt(21, 47, 0).unwrap());
    let rgb: Vec<u8> = fb.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    let mut enc = png::Encoder::new(BufWriter::new(File::create(path).unwrap()), W as u32 * 2, H as u32 * 2);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgb).unwrap();
}
fn tick(scene: &mut Scene, ms: u32) {
    for _ in 0..ms / 10 {
        scene.tick(10);
    }
}
fn sessions(roster: usize, state: State, pose: ForcedPose) -> Vec<Session> {
    (0..8)
        .map(|i| {
            // Fixed synthetic IDs with near-zero animation phases, for paired frames.
            let id = (0..)
                .map(|n| format!("qa-{i}-{n}"))
                .find(|s| {
                    let h = s.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
                    ((h >> 16) % 9000) < 30
                })
                .unwrap();
            Session {
                id,
                name: NAMES[1 + roster * 8 + i].into(),
                kind: if i % 4 < 2 { Kind::Claude } else { Kind::Codex },
                state,
                why: Why::Perm,
                pose,
                age_s: 0,
            }
        })
        .collect()
}
fn main() {
    let root = "target/patron-qa";
    fs::create_dir_all(root).unwrap();
    for (r, roster) in ["dragons", "pokemon"].iter().enumerate() {
        for (action, state, pose, a, b) in [
            ("type", State::Work, ForcedPose::Auto, 3000, 3180),
            ("think", State::Work, ForcedPose::Think, 0, 850),
            ("wave", State::Wait, ForcedPose::Auto, 0, 280),
            ("sip", State::Idle, ForcedPose::Sip, 0, 1100),
            ("sleep", State::Idle, ForcedPose::Sleep, 0, 1000),
            ("stretch", State::Idle, ForcedPose::Stretch, 0, 650),
        ] {
            let mut scene = Scene::new();
            scene.patrons = Patrons::roster(roster).unwrap();
            scene.set_usage(Usage { tokens: 12345., usd: 1. });
            scene.set_sessions(&sessions(r, state, pose));
            tick(&mut scene, a);
            save(&scene, &format!("{root}/{roster}-{action}-a.png"));
            tick(&mut scene, b - a);
            save(&scene, &format!("{root}/{roster}-{action}-b.png"));
        }
        let mut scene = Scene::new();
        scene.patrons = Patrons::roster(roster).unwrap();
        scene.set_sessions(&[]);
        let list = sessions(r, State::Work, ForcedPose::Auto);
        // Stagger entries so the crowd does not obscure the individual gaits.
        for n in 0..72 {
            if n % 8 == 0 && n / 8 < 8 {
                scene.set_sessions(&list[..n / 8 + 1]);
            }
            tick(&mut scene, 100);
            save(&scene, &format!("{root}/{roster}-arrival-{n:02}.png"));
        }
        scene.set_sessions(&[]);
        for n in 0..64 {
            tick(&mut scene, 100);
            save(&scene, &format!("{root}/{roster}-departure-{n:02}.png"));
        }
    }
    println!("Visual QA frames: {root}");
}
