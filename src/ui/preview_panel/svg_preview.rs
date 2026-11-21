//! Win11-style graphic SVG preview: rasterize the .svg with resvg and show
//! the bitmap in the panel (same engine as MadEdit's svgpreview). Rendering
//! runs on a background thread; the result is cached per path.

use std::path::Path as StdPath;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use eframe::egui;

/// Rasterize target: render at 2脳 the displayed width for crispness, capped
/// like image_viewer to guard against pathological viewBox values.
const TARGET_WIDTH: f32 = 1024.0;
const MAX_RENDER_SIDE: u32 = 4096;

pub fn is_svg_previewable(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("svg")
}

#[derive(Default, Clone)]
struct SvgPanelState {
    tex: Option<egui::TextureHandle>,
    in_flight: Option<Arc<Mutex<Option<Receiver<Result<Arc<egui::ColorImage>, String>>>>>>,
    failed: Option<String>,
}

fn request_render(state: &mut SvgPanelState, path: &StdPath) {
    let path_buf = path.to_path_buf();
    let (tx, rx) = channel();
    state.in_flight = Some(Arc::new(Mutex::new(Some(rx))));
    state.failed = None;
    std::thread::Builder::new()
        .name("svg-preview".into())
        .spawn(move || {
            let _ = tx.send(rasterize(&path_buf));
        })
        .ok();
}

fn rasterize(path: &StdPath) -> Result<Arc<egui::ColorImage>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default())
        .map_err(|e| format!("SVG parse: {e}"))?;

    let size = tree.size();
    let base_w = size.width().max(1.0);
    let base_h = size.height().max(1.0);

    // Fit to TARGET_WIDTH, never upscale small SVGs beyond 2脳, cap the side.
    let target_w = (TARGET_WIDTH.min(base_w * 2.0)).max(64.0);
    let scale = (target_w / base_w).min(MAX_RENDER_SIDE as f32 / base_w.max(base_h));
    let scale = scale.min(4.0);
    let w = ((base_w * scale).round().max(1.0) as u32).min(MAX_RENDER_SIDE);
    let h = ((base_h * scale).round().max(1.0) as u32).min(MAX_RENDER_SIDE);

    let mut pixmap = tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| "failed to allocate SVG render surface".to_string())?;
    pixmap.fill(tiny_skia::Color::from_rgba(1.0, 1.0, 1.0, 1.0).unwrap());
    let transform = tiny_skia::Transform::from_scale(
        w as f32 / base_w,
        h as f32 / base_h,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Premultiplied RGBA (tiny_skia) 鈫?straight RGBA.
    let mut rgba = Vec::with_capacity(pixmap.data().len());
    for px in pixmap.data().chunks_exact(4) {
        let a = px[3] as u32;
        let un = |c: u8| (((c as u32) * 255 + a / 2) / a.max(1)).min(255) as u8;
        rgba.extend_from_slice(&[un(px[0]), un(px[1]), un(px[2]), px[3]]);
    }
    Ok(Arc::new(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        &rgba,
    )))
}

pub fn render_svg_preview(ui: &mut egui::Ui, ctx: &egui::Context, path: &StdPath) {
    let id = egui::Id::new("svg_preview_cache").with(path.as_os_str());
    let mut state: SvgPanelState = ctx.data_mut(|d| d.get_temp(id).unwrap_or_default());

    // Harvest finished renders.
    let rx = state.in_flight.as_ref().and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()));
    if let Some(rx) = rx {
        match rx.try_recv() {
            Ok(Ok(image)) => {
                let tex = ctx.load_texture("svg_preview", (*image).clone(), egui::TextureOptions::LINEAR);
                state.tex = Some(tex);
                state.in_flight = None;
                state.failed = None;
            }
            Ok(Err(err)) => {
                state.failed = Some(err);
                state.in_flight = None;
            }
            Err(TryRecvError::Empty) => {
                if let Some(slot) = &state.in_flight {
                    if let Ok(mut g) = slot.lock() {
                        *g = Some(rx);
                    }
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(TryRecvError::Disconnected) => {
                state.failed = Some("SVG render worker died".into());
                state.in_flight = None;
            }
        }
    }

    if state.tex.is_none() && state.in_flight.is_none() && state.failed.is_none() {
        request_render(&mut state, path);
    }

    ui.vertical_centered(|ui| {
        if let Some(err) = &state.failed {
            ui.colored_label(egui::Color32::from_rgb(232, 17, 35), err);
        } else if let Some(tex) = &state.tex {
            egui::Frame::NONE
                .fill(egui::Color32::WHITE)
                .inner_margin(egui::Margin::same(6))
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Image::new(tex)
                            .max_width(ui.available_width())
                            .corner_radius(2.0),
                    );
                });
        } else {
            ui.add_space(30.0);
            ui.add(egui::Spinner::new().size(28.0));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
    });

    ctx.data_mut(|d| d.insert_temp(id, state));
}

#[cfg(test)]
mod tests {
    #[test]
    fn rasterize_simple_svg() {
        let dir = std::env::temp_dir().join("svg_preview_probe.svg");
        std::fs::write(&dir, r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="red"/></svg>"#).unwrap();
        match super::rasterize(&dir) {
            Ok(img) => println!("OK size {:?}", img.size),
            Err(e) => println!("ERR {e}"),
        }
        let _ = std::fs::remove_file(&dir);
    }
}
