//! Win11-Explorer-style inline PDF preview for the right preview panel.
//!
//! Toolbar shows `page / count` with prev/next buttons; the page body is
//! rendered by Pdfium on a background thread and uploaded as an egui texture
//! on completion. State is cached per file path in egui's temp memory.

use std::path::Path as StdPath;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::pdf_viewer::{render_preview_page, PdfPreviewPage};

/// Fixed render width in pixels — crisp enough for a ~1000px panel and for
/// zoomed viewing while keeping render time low.
const RENDER_WIDTH_PX: u32 = 1240;

/// Render width shown while waiting for the background job.
const LOADING_HINT_MIN_HEIGHT: f32 = 200.0;

pub fn is_pdf_previewable(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("pdf")
}

#[derive(Default, Clone)]
struct PdfPanelState {
    page_count: usize,
    /// Zero-based page currently requested/displayed.
    page: usize,
    tex: Option<egui::TextureHandle>,
    /// Page index the cached texture belongs to.
    tex_page: Option<usize>,
    /// Render job in flight (Mutex makes the Receiver Sync for egui storage).
    in_flight: Option<Arc<Mutex<Option<Receiver<Result<PdfPreviewPage, String>>>>>>,
    failed: Option<String>,
    /// Width of the panel at the time the texture was made (for refit).
    tex_panel_width: f32,
}

impl PdfPanelState {
    fn request_page(&mut self, path: &StdPath, page: usize) {
        if self.in_flight.is_some() {
            return;
        }
        let path_buf = path.to_path_buf();
        let (tx, rx) = channel();
        self.in_flight = Some(Arc::new(Mutex::new(Some(rx))));
        self.failed = None;
        std::thread::Builder::new()
            .name("pdf-preview".into())
            .spawn(move || {
                let _ = tx.send(render_preview_page(&path_buf, page, RENDER_WIDTH_PX));
            })
            .ok();
    }
}

pub fn render_pdf_preview(ui: &mut egui::Ui, ctx: &egui::Context, path: &StdPath) {
    let id = egui::Id::new("pdf_preview_cache").with(path.as_os_str());
    let mut state: PdfPanelState = ctx.data_mut(|d| d.get_temp(id).unwrap_or_default());

    // Harvest finished render jobs.
    let rx = state.in_flight.as_ref().and_then(|slot| {
        slot.lock().ok().and_then(|mut guard| guard.take())
    });
    if let Some(rx) = rx {
        match rx.try_recv() {
            Ok(Ok(result)) => {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [result.width as usize, result.height as usize],
                    &result.pixels,
                );
                let tex = ctx.load_texture("pdf_preview_page", image, egui::TextureOptions::LINEAR);
                state.tex = Some(tex);
                state.tex_page = Some(result.page_index);
                state.page_count = result.page_count;
                state.page = result.page_index;
                state.tex_panel_width = panel_width(ui);
                state.failed = None;
                state.in_flight = None;
            }
            Ok(Err(err)) => {
                state.failed = Some(err);
                state.in_flight = None;
            }
            Err(TryRecvError::Empty) => {
                // Job still running: put the receiver back and poll later.
                if let Some(slot) = &state.in_flight {
                    if let Ok(mut guard) = slot.lock() {
                        *guard = Some(rx);
                    }
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(TryRecvError::Disconnected) => {
                state.failed = Some("PDF render worker died".into());
                state.in_flight = None;
            }
        }
    }

    // Kick off the initial render.
    if state.tex.is_none() && state.in_flight.is_none() && state.failed.is_none() {
        state.request_page(path, state.page);
    }

    render_toolbar(ui, ctx, &mut state, path);

    ui.add_space(6.0);

    if let Some(err) = &state.failed {
        ui.colored_label(egui::Color32::from_rgb(232, 17, 35), err);
        return;
    }

    match (&state.tex, state.tex_page) {
        (Some(tex), Some(tex_page)) if tex_page == state.page => {
            ui.add(
                egui::Image::new(tex)
                    .max_width(ui.available_width())
                    .corner_radius(2.0),
            );
        }
        _ => {
            let min = egui::vec2(ui.available_width(), LOADING_HINT_MIN_HEIGHT);
            ui.allocate_exact_size(min, egui::Sense::hover());
            ui.add(egui::Spinner::new().size(28.0));
            ui.label(
                egui::RichText::new(rust_i18n::t!("preview.pdf_rendering").to_string()).small(),
            );
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    // Persist back.
    ctx.data_mut(|d| d.insert_temp(id, state));
}

fn panel_width(ui: &egui::Ui) -> f32 {
    ui.available_width()
}

fn render_toolbar(ui: &mut egui::Ui, ctx: &egui::Context, state: &mut PdfPanelState, path: &StdPath) {
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
            egui::RichText::new(format!(
                "{} / {}",
                state.page + 1,
                state.page_count
            ))
            .size(12.0),
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
                .small_button(rust_i18n::t!("preview.pdf_open_viewer").to_string())
                .on_hover_text(rust_i18n::t!("preview.pdf_open_viewer").to_string())
                .clicked()
            {
                crate::pdf_viewer::open_pdf_viewer(path.to_path_buf());
            }
        });
    });
    let _ = ctx;
}
