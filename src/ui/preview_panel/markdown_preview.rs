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
        // NOTE: `tail` starts at the URL's first character (head already
        // consumed "](") — do NOT skip further, or `ht` of `https` gets eaten.
        let url_end = tail.find(')').unwrap_or(tail.len());
        let url = &tail[..url_end.min(tail.len())];
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
                format!("file:///{}", s)
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
        // Height attr (坑 8): `height=80`, `height="80"`, `height=80px/` —
        // leading digits only, tolerant of dirty tails.
        let height = (|| {
            let lower = tag.to_ascii_lowercase();
            let p = lower.find("height")?;
            let digits: String = lower[p + 6..]
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            digits.parse::<u32>().ok().filter(|h| *h > 0)
        })();
        match src {
            Some(src) => {
                out.push_str(&rest[..start]);
                // Height travels to the image loader via a URI fragment
                // (方案 B has no height attr in markdown syntax).
                match height {
                    Some(h) => out.push_str(&format!("\n![]({src}#h={h})\n")),
                    None => out.push_str(&format!("\n![]({src})\n")),
                }
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

/// Remove leftover `<p ...>` / `</p>` wrapper tags (only as standalone lines
/// or whole-line tags) that egui_commonmark would render as literal text.
fn strip_stray_html_paragraph_tags(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    for line in md.split_inclusive('\n') {
        let trimmed = line.trim();
        if (trimmed.starts_with("<p") || trimmed.starts_with("</p"))
            && trimmed.ends_with('>')
            && !trimmed[1..].contains('<')
        {
            continue; // whole-line paragraph tag (open or close) — drop
        }
        out.push_str(line);
    }
    out
}

/// Collapse runs of image-only lines (`![alt](uri)` possibly with a `#h=NN`
/// fragment) into a single line joined by spaces so egui lays them out
/// side-by-side, matching the original inline HTML flow.
fn merge_consecutive_image_lines(md: &str) -> String {
    fn is_pure_image_line(line: &str) -> bool {
        let t = line.trim();
        t.starts_with("![") && t.ends_with(')') && t.contains("](")
    }

    let mut out = String::with_capacity(md.len());
    let mut run: Vec<String> = Vec::new();
    for line in md.split_inclusive('\n') {
        if is_pure_image_line(line) {
            run.push(line.trim().to_string());
            continue;
        }
        // Blank/whitespace-only lines INSIDE a run must not break it — the
        // original `<img>` indentation leaves such lines between images.
        if !run.is_empty() && line.trim().is_empty() {
            continue;
        }
        if !run.is_empty() {
            out.push_str(&run.join(" "));
            out.push('\n');
            run.clear();
        }
        out.push_str(line);
    }
    if !run.is_empty() {
        out.push_str(&run.join(" "));
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

pub fn render_markdown_preview(ui: &mut egui::Ui, path: &StdPath) {
    // Reuse the text preview loader (encoding detection + cap + cache).
    let raw = text_preview::cached_text(ui.ctx(), path);
    // ORDER: (1) convert `<img src=...>` tags into markdown images, THEN
    // (2) rewrite every image URL (including the freshly converted ones) to
    // an absolute file:/// URI against the .md's directory. Rewriting first
    // would leave the HTML-born images relative and they would never load.
    let md = rewrite_image_paths(&convert_html_images(&raw.text), path.parent());
    // egui_commonmark renders unknown HTML as literal text; drop the leftover
    // paragraph wrappers that surrounded converted `<img>` tags.
    let md = strip_stray_html_paragraph_tags(&md);

    // Merge consecutive image-only lines into one line (space separated):
    // egui_commonmark renders each soft line break as a hard break, so the
    // converted `<img>` lines would stack vertically instead of flowing
    // side-by-side like the original HTML (e.g. "Nested by sparrow" grids).
    let md = merge_consecutive_image_lines(&md);

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
    // CRITICAL ORDER (MadEdit: "載入器要後裝，egui 會先問後裝的載入器"):
    // egui_commonmark installs the built-in egui_extras loaders INSIDE its
    // render call, so our universal loader must be installed AFTER `show` to
    // become the most recently added — egui asks most-recent loaders first.
    // Installed before, it would never be consulted and every image with a
    // `#fragment` or query string would show as a broken-image warning.
    super::remote_svg_loader::PreviewImageLoader::install(ui.ctx());

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

#[cfg(test)]
mod rewrite_probe {
    #[test]
    fn probe_readme_badge_lines() {
        let path = std::path::Path::new(r"G:\back\share\sparrow\里程碑备份\sparrow\README.md");
        let Ok(raw) = std::fs::read_to_string(path) else { return };
        let md = super::rewrite_image_paths(&super::convert_html_images(&raw), path.parent());
        for line in md.lines() {
            if line.contains("badge.svg") || line.contains("zenodo") {
                println!("LINE: {line}");
            }
        }
    }
}

#[cfg(test)]
mod rewrite_isolate {
    #[test]
    fn isolate_h_loss() {
        let md = "[![CI](https://github.com/x/badge.svg?branch=main)](https://github.com/x/ci)";
        let after_rewrite = super::rewrite_image_paths(md, None);
        println!("AFTER_REWRITE: {after_rewrite}");
        let after_html = super::convert_html_images(&after_rewrite);
        println!("AFTER_HTML: {after_html}");
        assert_eq!(after_rewrite, md, "rewrite must not modify absolute URLs");
    }
}

#[cfg(test)]
mod md_probe {
    #[test]
    fn probe_readme_processed() {
        std::env::set_var("PROBE_MD", "1");
        let path = std::path::Path::new(r"G:\back\share\sparrow\里程碑备份\sparrow\README.md");
        let raw = match std::fs::read_to_string(path) { Ok(r) => r, Err(_) => return };
        let md = super::rewrite_image_paths(&super::convert_html_images(&raw), path.parent());
        let md = super::strip_stray_html_paragraph_tags(&md);
        for line in md.lines() {
            if line.contains("<p") || line.contains("<img") || line.contains("![]") {
                println!("MDLINE: {line}");
            }
        }
    }
}

#[cfg(test)]
mod demo_gif_probe {
    #[test]
    fn probe_demo_gif_line() {
        let path = std::path::Path::new(r"G:\back\share\sparrow\里程碑备份\sparrow\README.md");
        let raw = match std::fs::read_to_string(path) { Ok(r) => r, Err(_) => return };
        let step1 = super::convert_html_images(&raw);
        let step2 = super::rewrite_image_paths(&step1, path.parent());
        let md = super::strip_stray_html_paragraph_tags(&step2);
        for (label, text) in [("RAW", &raw), ("S1", &step1), ("S2", &step2), ("MD", &md)] {
            let hit = text.lines().find(|l| l.contains("demo.gif"));
            println!("{label}: {:?}", hit.map(|l| l.trim_start()).unwrap_or("<LINE LOST>"));
        }
    }
}

#[cfg(test)]
mod pipeline_probe2 {
    #[test]
    fn probe_merged_output() {
        let path = std::path::Path::new(r"G:\back\share\sparrow\里程碑备份\sparrow\README.md");
        let raw = match std::fs::read_to_string(path) { Ok(r) => r, Err(_) => return };
        let md = super::rewrite_image_paths(&super::convert_html_images(&raw), path.parent());
        let md = super::strip_stray_html_paragraph_tags(&md);
        let md = super::merge_consecutive_image_lines(&md);
        for line in md.lines() {
            if line.contains("![](") || line.contains("<p") {
                println!("MLINE: {line}");
            }
        }
    }
}

#[cfg(test)]
mod e2e_gif_probe {
    use eframe::egui::load::{ImagePoll, SizeHint};

    /// End-to-end: render the processed README through CommonMarkViewer with
    /// the app's loader chain, then poll egui for the demo.gif image result.
    #[test]
    fn probe_egui_render_of_demo_gif() {
        let path = std::path::Path::new(r"G:\back\share\sparrow\里程碑备份\sparrow\README.md");
        let raw = match std::fs::read_to_string(path) { Ok(r) => r, Err(_) => return };
        let ctx = eframe::egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        crate::ui::preview_panel::remote_svg_loader::PreviewImageLoader::install(&ctx);
        let md = super::rewrite_image_paths(&super::convert_html_images(&raw), path.parent());
        let md = super::strip_stray_html_paragraph_tags(&md);
        let md = super::merge_consecutive_image_lines(&md);

        let cache = std::sync::Arc::new(std::sync::Mutex::new(
            egui_commonmark::CommonMarkCache::default(),
        ));
        let mut t = 0.0f64;
        for i in 0..20 {
            t += 0.1;
            let input = eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(400.0, 800.0),
                )),
                time: Some(t),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                let mut guard = cache.lock().unwrap();
                egui_commonmark::CommonMarkViewer::new()
                    .max_image_width(Some(ui.available_width() as usize))
                    .show(ui, &mut guard, &md);
            });
            let _ = i;
        }
        let uri = "file://G:/back/share/sparrow/里程碑备份/sparrow/data/demo.gif";
        match ctx.try_load_image(uri, SizeHint::default()) {
            Ok(ImagePoll::Ready { image }) => println!("GIF READY {:?}", image.size),
            Ok(ImagePoll::Pending { .. }) => println!("GIF PENDING"),
            Err(e) => println!("GIF ERR {e:?}"),
        }
    }
}
