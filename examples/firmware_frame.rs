//! Export the real desktop renderer as RGB565 LE, centered on a 480x320 panel.
//! cargo run --example firmware_frame -- target/cafe.rgb565 [scene-x scene-y]
use std::error::Error;
use token_cafe::assets::{H, W};
use token_cafe::scene::{ForcedPose, Kind, Scene, Session, SessionDetail, State, Why};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 1 && args.len() != 3 {
        return Err("usage: firmware_frame OUTPUT [SCENE_X SCENE_Y]".into());
    }
    let mut scene = Scene::new();
    let sessions: Vec<_> = ["api-server", "checkout", "docs", "infra"]
        .iter()
        .enumerate()
        .map(|(i, name)| Session {
            id: format!("firmware-demo-{i}"),
            name: (*name).into(),
            kind: if i % 2 == 0 { Kind::Claude } else { Kind::Codex },
            state: if i == 1 { State::Wait } else { State::Work },
            why: if i == 1 { Why::Perm } else { Why::None },
            pose: ForcedPose::Auto,
            age_s: 0,
            branch: None,
            active_subagents: None,
        })
        .collect();
    scene.set_sessions(&sessions);
    scene.set_details(
        sessions
            .iter()
            .map(|s| SessionDetail {
                id: s.id.clone(),
                target: None,
                task: "Test frame updates and patron selection on the virtual display.".into(),
            })
            .collect(),
    );
    for _ in 0..60 {
        scene.tick(33);
    }
    if args.len() == 3 {
        scene.click(args[1].parse()?, args[2].parse()?);
    }
    let mut pixels = vec![0; W * H];
    scene.render(&mut pixels, chrono::NaiveTime::from_hms_opt(20, 0, 0).unwrap());
    let mut frame = vec![0u8; 480 * 320 * 2];
    for y in 0..H {
        for x in 0..W {
            let p = pixels[y * W + x];
            let rgb565 = (((p >> 19) & 31) << 11) | (((p >> 10) & 63) << 5) | ((p >> 3) & 31);
            let i = ((y + 40) * 480 + x + 80) * 2;
            frame[i..i + 2].copy_from_slice(&(rgb565 as u16).to_le_bytes());
        }
    }
    std::fs::write(&args[0], frame)?;
    Ok(())
}
