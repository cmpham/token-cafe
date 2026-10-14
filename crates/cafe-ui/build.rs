use sha2::{Digest, Sha256};
fn main() {
    let mut hash = Sha256::new();
    for file in ["assets.rs", "bg.rgb", "scene.rs", "gfx.rs", "review.rs", "text.rs"] {
        let path = format!("src/{file}");
        hash.update(file.as_bytes());
        hash.update(std::fs::read(&path).unwrap());
        println!("cargo:rerun-if-changed={path}");
    }
    let value: [u8; 32] = hash.finalize().into();
    std::fs::write(
        std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("asset_digest.rs"),
        format!("{value:?}"),
    )
    .unwrap();
}
