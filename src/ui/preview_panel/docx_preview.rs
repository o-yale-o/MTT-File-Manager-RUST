//! DOCX inline preview for the right preview panel: renders pages with the
//! BetterOffice raster pipeline (same engine as the standalone docx viewer)
//! on a background thread, with a Win11-style `page/total ◀▶` pager.
//!
//! The renderer (parse + fonts + layout) is expensive to build, so exactly
//! one open document is kept in a process-wide slot and reused across page
//! turns; switching files replaces it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::docx_viewer::{DocxPagePixels, DocxRenderer};

const TARGET_PAGE_WIDTH: u32 = 1240;

/// Process-wide cache of the most recently opened renderer (single slot).
static DOCX_RENDERER: Mutex<Option<(PathBuf, Arc<Mutex<DocxRenderer>>)>> =
    Mutex::new(None);

/// Returns a shared renderer for `path`, (re)opening when the slot holds a
/// different document. The returned guard serializes page renders.
fn shared_renderer(path: &Path) -> Result<Arc<Mutex<DocxRenderer>>, String> {
    let mut slot = DOCX_RENDERER.lock().unwrap();
    if let Some((cached_path, renderer)) = slot.as_ref() {
        if cached_path == path {
            return Ok(Arc::clone(renderer));
        }
    }
    let renderer = Arc::new(Mutex::new(
        DocxRenderer::open(path).map_err(|e| format!("open failed: {e}"))?,
    ));
    *slot = Some((path.to_path_buf(), Arc::clone(&renderer)));
    Ok(renderer)
}

struct PageResult {
    image: Arc<egui::ColorImage>,
    page_count: usize,
    page_index: usize,
}

fn render_page(
    path: &Path,
    page_index: usize,
) -> Result<PageResult, String> {
    let renderer = shared_renderer(path)?;
    let mut guard = renderer.lock().unwrap();
    let page_count = guard.page_sizes().len();
    let page: DocxPagePixels = guard.render_page(page_index)?;
    drop(guard);
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [page.width, page.height],
        &page.rgba,
    );
    Ok(PageResult {
        image: Arc::new(image),
        page_count,
        page_index: page.page_index,
    })
}

#[derive(Default, Clone)]
struct DocxPanelState {
    page_count: usize,
    page: usize,
    tex: Option<egui::TextureHandle>,
    tex_page: Option<usize>,
    in_flight: Option<Arc<Mutex<Option<Receiver<Result<PageResult, String>>>>>>,
    failed: Option<String>,
    /// Watchdog: when the current fetch started (None = idle).
    started_at: Option<std::time::Instant>,
}

use std::sync::mpsc::{channel, Receiver, TryRecvError};

impl DocxPanelState {
    fn request_page(&mut self, path: &Path, page: usize) {
        if self.in_flight.is_some() {
            return;
        }
        let path_buf = path.to_path_buf();
        let (tx, rx) = channel();
        self.in_flight = Some(Arc::new(Mutex::new(Some(rx))));
        self.failed = None;
        self.started_at = Some(std::time::Instant::now());
        log::info!("[DocxPreview] fetch started for {}", path.display());
        std::thread::Builder::new()
            .name("docx-preview".into())
            .spawn(move || {
                let _ = tx.send(render_page(&path_buf, page));
            })
            .ok();
    }
}

pub fn is_docx_previewable(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("docx")
}

pub fn render_docx_preview(ui: &mut egui::Ui, ctx: &egui::Context, path: &Path) {
    let id = egui::Id::new("docx_preview_cache").with(path.as_os_str());
    let mut state: DocxPanelState = ctx.data_mut(|d| d.get_temp(id).unwrap_or_default());

    // Poll the in-flight render.
    let rx = state
        .in_flight
        .as_ref()
        .and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()));
    if let Some(rx) = rx {
        match rx.try_recv() {
            Ok(Ok(result)) => {
                let tex = ctx.load_texture("docx_preview_page", (*result.image).clone(), egui::TextureOptions::LINEAR);
                state.tex = Some(tex);
                state.tex_page = Some(result.page_index);
                state.page_count = result.page_count;
                state.page = result.page_index;
                state.failed = None;
                state.in_flight = None;
                state.started_at = None;
                log::info!(
                    "[DocxPreview] page {} ready in {:?}",
                    result.page_index + 1,
                    state.started_at.is_none()
                );
            }
            Ok(Err(err)) => {
                state.failed = Some(err);
                state.in_flight = None;
                state.started_at = None;
            }
            Err(TryRecvError::Empty) => {
                if let Some(slot) = &state.in_flight {
                    if let Ok(mut guard) = slot.lock() {
                        *guard = Some(rx);
                    }
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(TryRecvError::Disconnected) => {
                state.failed = Some("docx render worker died".into());
                state.in_flight = None;
                state.started_at = None;
            }
        }
    }

    // Watchdog: abandon a fetch stuck >20 s (pathological documents can hang
    // the BetterOffice layout indefinitely); the orphan thread finishes on
    // its own harmlessly and the panel recovers.
    const DOCX_RENDER_WATCHDOG: std::time::Duration = std::time::Duration::from_secs(20);
    if state.in_flight.is_some() {
        if let Some(started) = state.started_at {
            if started.elapsed() > DOCX_RENDER_WATCHDOG {
                log::warn!("[DocxPreview] render watchdog timeout for {}", path.display());
                state.failed = Some(
                    rust_i18n::t!("preview.docx_render_timeout").to_string(),
                );
                state.in_flight = None;
                state.started_at = None;
            }
        }
    }

    if state.tex.is_none() && state.in_flight.is_none() && state.failed.is_none() {
        state.request_page(path, state.page);
    }

    // Pager toolbar.
    ui.horizontal(|ui| {
        let prev_enabled = state.page > 0 && state.in_flight.is_none();
        let next_enabled =
            state.page + 1 < state.page_count.max(1) && state.in_flight.is_none();
        if ui
            .add_enabled(prev_enabled, egui::Button::new("◀").small())
            .on_hover_text(rust_i18n::t!("preview.pdf_prev_page").to_string())
            .clicked()
        {
            state.page = state.page.saturating_sub(1);
            state.request_page(path, state.page);
        }
        ui.label(
            egui::RichText::new(format!("{} / {}", state.page + 1, state.page_count)).size(12.0),
        );
        if ui
            .add_enabled(next_enabled, egui::Button::new("▶").small())
            .on_hover_text(rust_i18n::t!("preview.pdf_next_page").to_string())
            .clicked()
        {
            state.page += 1;
            state.request_page(path, state.page);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(rust_i18n::t!("preview.docx_open_viewer").to_string())
                .on_hover_text(rust_i18n::t!("preview.docx_open_viewer").to_string())
                .clicked()
            {
                crate::docx_viewer::open_docx_viewer(path.to_path_buf());
            }
        });
    });

    ui.add_space(6.0);

    if let Some(err) = &state.failed {
        ui.colored_label(egui::Color32::from_rgb(232, 17, 35), err);
        return;
    }

    match (&state.tex, state.tex_page) {
        (Some(tex), Some(tex_page)) if tex_page == state.page => {
            // Fixed-height inner scroll: the pager toolbar stays pinned at the
            // top while the (tall) page image scrolls inside its own area.
            let scroll_h = (ui.available_height() - 40.0).max(120.0);
            egui::ScrollArea::vertical()
                .id_salt(("docx_preview_scroll", path.as_os_str()))
                .auto_shrink([false, false])
                .max_height(scroll_h)
                .show(ui, |ui| {
                    ui.add(
                        egui::Image::new(tex)
                            .max_width(ui.available_width())
                            .corner_radius(2.0),
                    );
                });
        }
        _ => {
            ui.add_space(30.0);
            ui.add(egui::Spinner::new().size(28.0));
            ui.label(
                egui::RichText::new(rust_i18n::t!("preview.pdf_rendering").to_string()).small(),
            );
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    ctx.data_mut(|d| d.insert_temp(id, state));
}
