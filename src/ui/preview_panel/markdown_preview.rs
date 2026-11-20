//! Markdown preview rendered with `egui_commonmark` (same engine family as
//! MadEdit's in-app preview). Relative image paths are rewritten to absolute
//! `file:///` URIs against the .md file's directory before rendering.

use std::path::Path as StdPath;
use std::sync::{Arc, Mutex};

use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use super::text_preview;

/// Extensions that get the rich Markdown preview.
pub fn is_markdown_previewable(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown" | "mkd")
}

/// Rewrite relative image URLs to absolute file:/// URIs so egui's file
/// image loader can find them (same approach as MadEdit's image_uri).
fn rewrite_image_paths(md: &str, base_dir: Option<&StdPath>) -> String {
    // Quick scan: only touch `](url)` image targets that are relative paths.
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(pos) = rest.find("](") {
        let (head, tail) = rest.split_at(pos + 2); // head ends with "]("
        let url_end = tail.find(')').unwrap_or(tail.len());
        let url = &tail[2..url_end.min(tail.len())];
        out.push_str(head);

        let abs = if url.is_empty()
            || url.starts_with('#')
            || url.contains("://")
            || url.starts_with("data:")
            || url.starts_with('/')
        {
            None
        } else {
            base_dir.map(|b| b.join(url.replace('/', "\\")))
        };

        if let Some(abs) = abs {
            let s = abs.to_string_lossy().replace('\\', "/");
            let uri = if let Some(stripped) = s.strip_prefix('/') {
                format!("file:///{stripped}")
            } else {
                format!("file://{}", s)
            };
            out.push_str(&uri);
        } else {
            out.push_str(url);
        }
        rest = &tail[url_end.min(tail.len())..];
    }
    out.push_str(rest);
    out
}

/// Convert raw HTML `<img src="...">` tags into Markdown `![](src)` images
/// (egui_commonmark never executes HTML; MadEdit does the same conversion).
fn convert_html_images(md: &str) -> String {
    if !md.contains("<img") {
        return md.to_string();
    }
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(start) = rest.find("<img") {
        // Only treat it as a tag if a tag end exists.
        let Some(tag_end_rel) = rest[start..].find('>') else {
            break;
        };
        let tag = &rest[start..start + tag_end_rel + 1];
        // Extract src="..." or src='...'.
        let src = (|| {
            for quote in ['"', '\''] {
                if let Some(p) = tag.find(&format!("src={quote}")) {
                    let after = &tag[p + 5..];
                    if let Some(end) = after.find(quote) {
                        return Some(after[..end].to_string());
                    }
                }
            }
            None
        })();
        match src {
            Some(src) => {
                out.push_str(&rest[..start]);
                out.push_str(&format!("\n![]({src})\n"));
                rest = &rest[start + tag_end_rel + 1..];
            }
            None => {
                // Not an image tag after all; keep scanning from past "<img".
                out.push_str(&rest[..start + 4]);
                rest = &rest[start + 4..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn render_markdown_preview(ui: &mut egui::Ui, path: &StdPath) {
    // egui_extras file/image loaders (required by the `load-images` feature);
    // installing repeatedly is a cheap no-op (loaders are deduplicated).
    egui_extras::install_image_loaders(ui.ctx());
    // Reuse the text preview loader (encoding detection + 256 KB cap + cache).
    let raw = text_preview::cached_text(ui.ctx(), path);
    let md = convert_html_images(&rewrite_image_paths(&raw.text, path.parent()));

    let id = egui::Id::new("md_preview_cache").with(path.as_os_str());
    let cache: Arc<Mutex<CommonMarkCache>> = ui
        .ctx()
        .data_mut(|d| d.get_temp::<Arc<Mutex<CommonMarkCache>>>(id))
        .unwrap_or_else(|| Arc::new(Mutex::new(CommonMarkCache::default())));
    // IMPORTANT: `ScrollArea::both()` — a fixed viewport. Markdown tables are
    // rendered as untruncated egui Grids whose minimum width can far exceed
    // the panel; without the horizontal clip that minimum propagates out and
    // pushes the whole preview panel off the window.
    egui::ScrollArea::both()
        .id_salt(("md_preview_scroll", path.as_os_str()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut guard = cache.lock().unwrap();
            CommonMarkViewer::new()
                .max_image_width(Some(ui.available_width() as usize))
                .show(ui, &mut guard, &md);
        });

    ui.ctx().data_mut(|d| d.insert_temp(id, cache));

    if raw.truncated {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(rust_i18n::t!("preview.text_truncated").to_string())
                .small()
                .color(egui::Color32::GRAY),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, RawInput, Vec2};

    /// Render the Markdown pipeline over a real file inside a headless egui
    /// context — any panic in egui_commonmark surfaces as a test failure.
    #[test]
    fn renders_real_world_markdown_without_panicking() {
        let path = StdPath::new(r"D:\OpenSource\sparrow\CADCraft人机双轨交互方案.md");
        let Ok(md) = std::fs::read_to_string(path) else {
            return; // file not present in CI
        };
        let _ = md;

        let ctx = Context::default();
        let mut t = 0.0f64;
        for _ in 0..3 {
            t += 0.05;
            let input = RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(400.0, 800.0),
                )),
                time: Some(t),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                render_markdown_preview(ui, path);
            });
        }
    }
}
