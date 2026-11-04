//! Render the real Controller design-system components at native board resolution.
use token_cafe::{
    assets::{H, W},
    controller_design::{self as d, Rect, StatusIcon, Text},
    gfx::Gfx,
};
fn main() {
    let mut frame = vec![0; W * H];
    let mut g = Gfx::new(&mut frame);
    Rect::new(0, 0, W as i32, H as i32).paint(&mut g, d::color::CANVAS);
    d::label(&mut g, "Controller / Design system v2", Rect::new(12, 5, 450, 22), Text::Title, d::color::TEXT);
    for (i, (name, color)) in [
        ("Canvas", d::color::CANVAS),
        ("Surface", d::color::SURFACE),
        ("Text", d::color::TEXT),
        ("Muted", d::color::SECONDARY),
        ("Focus", d::color::FOCUS),
        ("Attention", d::color::ATTENTION),
        ("Unread", d::color::UNREAD),
    ]
    .iter()
    .enumerate()
    {
        let x = 12 + i as i32 * 66;
        g.rect(x, 36, 52, 12, *color);
        d::label(&mut g, name, Rect::new(x, 50, 60, 22), Text::Caption, d::color::SECONDARY);
    }
    d::label(&mut g, "Session titles / 16px", Rect::new(12, 77, 450, 22), Text::Title, d::color::TEXT);
    d::label(&mut g, "Metadata 11px / controls 13px", Rect::new(12, 101, 240, 22), Text::Caption, d::color::SECONDARY);
    d::button(&mut g, Rect::new(264, 76, 100, 44), "Details", true, false);
    d::button(&mut g, Rect::new(370, 76, 100, 44), "Prev page", false, false);
    for (i, selected) in [true, false].iter().enumerate() {
        let r = Rect::new(4 + i as i32 * 238, 126, 234, 56);
        d::tile(&mut g, r, *selected);
        d::label(
            &mut g,
            if *selected { "Selected session" } else { "Another session" },
            Rect::new(r.x + 7, r.y + 7, d::layout::TITLE_WIDTH, 22),
            Text::Title,
            d::color::TEXT,
        );
        d::label(
            &mut g,
            "CODEX / feature/display",
            Rect::new(r.x + 7, r.y + 30, d::layout::TITLE_WIDTH, 22),
            Text::Caption,
            d::color::SECONDARY,
        );
    }
    for (i, icon) in [
        StatusIcon::Working,
        StatusIcon::NewOutput,
        StatusIcon::Input,
        StatusIcon::Approval,
        StatusIcon::Offline,
        StatusIcon::Idle,
    ]
    .iter()
    .enumerate()
    {
        let x = i as i32 * 80;
        icon.draw(&mut g, x + 30, 205);
        d::label(
            &mut g,
            ["Working", "New output", "Input", "Approval", "Offline", "Idle"][i],
            Rect::new(x + 4, 229, 72, 22),
            Text::Caption,
            d::color::TEXT,
        );
    }
    let rgb = frame.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect::<Vec<_>>();
    let path = "docs/controller-design-system.png";
    let mut encoder =
        png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).unwrap()), W as u32, H as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&rgb).unwrap();
    println!("{path}");
}
