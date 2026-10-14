//! Render review states with the production renderer: cargo run --example review_screenshots.
use cafe_ui::{
    approval::Request,
    assets::{H, W},
    gfx::Gfx,
    review::Review,
};
use std::{fs::File, io::BufWriter};

fn main() {
    std::fs::create_dir_all("target/review-previews").unwrap();
    let mut review = Review::new(
        Request {
            id: "preview-request".into(),
            session: "preview-session".into(),
            title: "sg-FSA-003: Enforce FundSub environment policy for file access".into(),
            tool_name: String::new(),
            text: "yes, apply the scope fix and add the test".into(),
        },
        false,
    );
    review.status = "READ ONLY - OPEN DESKTOP".into();
    save("task", &review);
    review.actionable = true;
    review.status.clear();
    let mut request = review.request().clone();
    request.tool_name = "Bash".into();
    request.text = (1..=20).map(|n| format!("Line {n}: preserve LiteralCase and exact command text.\n")).collect();
    review = Review::new(request, true);
    save("approval", &review);
    review.page(true);
    save("scrolled", &review);
    review.dismiss_pending = true;
    save("dismiss", &review);
}
fn save(name: &str, review: &Review) {
    let mut frame = vec![0; W * H];
    review.draw(&mut Gfx::new(&mut frame));
    let rgb: Vec<u8> = frame.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    let mut encoder = png::Encoder::new(
        BufWriter::new(File::create(format!("target/review-previews/{name}.png")).unwrap()),
        W as u32,
        H as u32,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&rgb).unwrap();
}
