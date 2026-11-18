//! Dump the extracted shell icon for a given extension/size to a PNG and
//! print corner/center alpha values, to diagnose the "washed-out tile" look.
//!
//! Usage: cargo run --example icon_probe -- md 256 out.png
#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ext = args.get(1).map(|s| s.as_str()).unwrap_or("md");
    let size: u32 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    let out = args.get(3).map(|s| s.as_str()).unwrap_or("icon_dump.png");

    // Verify the extension channel's new Jumbo extraction (256px from the
    // registered icon file via SHDefExtractIconW).
    let icon = mtt_file_manager::infrastructure::windows::icons::get_file_type_icon(
        false,
        ext,
        mtt_file_manager::domain::file_entry::IconSize::Jumbo,
    );
    let (rgba, w, h) = match icon {
        Ok(icon) => icon,
        Err(e) => {
            println!("extract failed: {e:?}");
            return;
        }
    };
    println!("size: {w}x{h}, bytes: {}", rgba.len());
    let px = |x: usize, y: usize| -> [u8; 4] {
        let i = (y * w as usize + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    let corners = [
        ("top-left", px(2, 2)),
        ("top-right", px(w as usize - 3, 2)),
        ("bottom-left", px(2, h as usize - 3)),
        ("bottom-right", px(w as usize - 3, h as usize - 3)),
        ("center", px(w as usize / 2, h as usize / 2)),
        ("mid-top", px(w as usize / 2, 2)),
    ];
    for (label, [r, g, b, a]) in corners {
        println!("{label}: rgba({r},{g},{b},{a})");
    }
    // Save as PNG on a magenta background so transparency is obvious.
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let (x, y) = (x as usize, y as usize);
        let i = (y * w as usize + x) * 4;
        let a = rgba[i + 3] as f32 / 255.0;
        let blend = |fg: u8, bg: u8| (fg as f32 * a + bg as f32 * (1.0 - a)).round() as u8;
        *p = image::Rgba([
            blend(rgba[i], 255),
            blend(rgba[i + 1], 0),
            blend(rgba[i + 2], 255),
            255,
        ]);
    }
    img.save(out).expect("save png");
    println!("saved {out}");
}

#[cfg(not(windows))]
fn main() {}
