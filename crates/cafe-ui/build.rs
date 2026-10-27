use sha2::{Digest, Sha256};
fn main() {
    let mut hash = Sha256::new();
    for file in ["assets.rs", "bg.rgb", "scene.rs", "gfx.rs", "review.rs", "text.rs"] {
        let path = format!("src/{file}");
        hash.update(file.as_bytes());
        hash.update(std::fs::read(&path).unwrap());
        println!("cargo:rerun-if-changed={path}");
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let mut data = Vec::new();
    let mut rows = Vec::new();
    for file in ["src/patrons.rs", "../../assets/patrons/poses.rgba", "../../assets/patrons/cup-anchors.bin"] {
        println!("cargo:rerun-if-changed={file}");
        hash.update(file.as_bytes());
        hash.update(std::fs::read(file).unwrap());
    }
    let pixels = std::fs::read("../../assets/patrons/poses.rgba").unwrap();
    for row in pixels.as_chunks::<{ 48 * 4 }>().0 {
        rows.push(data.len() as u32);
        let mut x = 0;
        while x < 48 {
            let canonical = |i: usize| -> [u8; 4] {
                if row[i * 4 + 3] < 128 { [0; 4] } else { row[i * 4..i * 4 + 4].try_into().unwrap() }
            };
            let pixel = canonical(x);
            let mut end = x + 1;
            while end < 48 && canonical(end) == pixel {
                end += 1;
            }
            data.push((end - x) as u8);
            data.extend_from_slice(&pixel);
            x = end;
        }
    }
    std::fs::write(out.join("patron-runs.bin"), data).unwrap();
    std::fs::write(out.join("patron-rows.rs"), format!("{rows:?}")).unwrap();
    // RGB24 plus an eight-bit run length: one aligned flash read per run,
    // followed by contiguous framebuffer writes. No decoded background heap.
    let background = std::fs::read("src/bg.rgb").unwrap();
    let mut runs: Vec<u32> = Vec::new();
    for pixel in background.as_chunks::<3>().0 {
        let rgb = (pixel[0] as u32) << 16 | (pixel[1] as u32) << 8 | pixel[2] as u32;
        match runs.last_mut() {
            Some(last) if *last & 0xffffff == rgb && *last >> 24 < 255 => *last += 1 << 24,
            _ => runs.push((1 << 24) | rgb),
        }
    }
    std::fs::write(
        std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("background_runs.rs"),
        format!("{runs:?}"),
    )
    .unwrap();
    // Bind the generated representation into the desktop/board compatibility digest.
    for run in &runs {
        hash.update(run.to_le_bytes());
    }
    let value: [u8; 32] = hash.finalize().into();
    std::fs::write(
        std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("asset_digest.rs"),
        format!("{value:?}"),
    )
    .unwrap();
}
