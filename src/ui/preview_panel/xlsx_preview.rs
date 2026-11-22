//! XLSX/XLS inline preview for the right preview panel: reads cell data with
//! calamine (xlsx/xlsm/xls/ods) and renders a native egui table — full CJK
//! support (the BetterOffice raster engine embeds only a Latin font).
//!
//! Preview limits: first 500 rows × 60 columns; a note is shown when the
//! sheet is larger (open the standalone viewer for the full sheet).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use betteroffice_xlsx as _; // keep the standalone viewer dependency linked
use calamine::{open_workbook_auto, Reader};
use eframe::egui;
use egui_extras::{Column, TableBuilder};

const MAX_ROWS: usize = 500;
const MAX_COLS: usize = 60;
const MAX_CELL_CHARS: usize = 120;

pub(crate) struct SheetData {
    pub(crate) sheet_index: usize,
    pub(crate) sheet_names: Vec<String>,
    /// Cell text grid (already truncated to MAX_ROWS × MAX_COLS).
    pub(crate) rows: Vec<Vec<String>>,
    pub(crate) max_cols: usize,
    /// Per-column longest cell text (chars) for width computation.
    pub(crate) col_chars: Vec<usize>,
    pub(crate) total_rows: usize,
    pub(crate) total_cols: usize,
}

fn data_to_string(data: &calamine::Data) -> String {
    match data {
        calamine::Data::Empty => String::new(),
        calamine::Data::Error(err) => format!("#{err}"),
        other => {
            let s = other.to_string();
            if s.chars().count() > MAX_CELL_CHARS {
                format!("{}…", s.chars().take(MAX_CELL_CHARS).collect::<String>())
            } else {
                s
            }
        }
    }
}

pub(crate) fn load_sheet(path: &Path, sheet_index: usize) -> Result<SheetData, String> {
    let mut workbook = open_workbook_auto(path)
        .map_err(|e| format!("open failed: {e}"))?;
    let sheet_names: Vec<String> = workbook.sheet_names().to_vec();
    let name = sheet_names
        .get(sheet_index)
        .ok_or_else(|| "sheet index out of range".to_string())?
        .clone();
    let range = workbook
        .worksheet_range(&name)
        .map_err(|e| format!("read sheet failed: {e}"))?;

    let (total_rows, total_cols) = range.get_size();
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(total_rows.min(MAX_ROWS));
    let mut max_cols = 0usize;
    let mut col_chars: Vec<usize> = Vec::new();
    for (row_idx, row) in range.rows().enumerate() {
        if row_idx >= MAX_ROWS {
            break;
        }
        let cols = row.len().min(MAX_COLS);
        max_cols = max_cols.max(cols);
        let cells = (0..cols)
            .map(|c| data_to_string(&row[c]))
            .collect::<Vec<_>>();
        if col_chars.len() < cols {
            col_chars.resize(cols, 0);
        }
        for (c, cell) in cells.iter().enumerate() {
            col_chars[c] = col_chars[c].max(cell.chars().count());
        }
        rows.push(cells);
    }
    for row in &rows {
        max_cols = max_cols.max(row.len());
    }
    col_chars.resize(max_cols, 0);

    Ok(SheetData {
        sheet_index,
        sheet_names,
        rows,
        max_cols,
        col_chars,
        total_rows,
        total_cols,
    })
}

#[derive(Default, Clone)]
struct XlsxPanelState {
    pub(crate) sheet_index: usize,
    pub(crate) sheet_names: Vec<String>,
    pub(crate) rows: Vec<Vec<String>>,
    pub(crate) max_cols: usize,
    pub(crate) col_chars: Vec<usize>,
    pub(crate) total_rows: usize,
    pub(crate) total_cols: usize,
    loaded: bool,
    in_flight: Option<Arc<Mutex<Option<Receiver<Result<SheetData, String>>>>>>,
    failed: Option<String>,
    /// Display zoom multiplier for the cell table (1.0 = 100%).
    zoom: f32,
}

impl XlsxPanelState {
    fn zoom(&self) -> f32 {
        if self.zoom.is_finite() && self.zoom >= 0.25 {
            self.zoom
        } else {
            1.0
        }
    }
}

fn column_letter(mut col: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

impl XlsxPanelState {
    fn request_sheet(&mut self, path: &Path, sheet_index: usize) {
        if self.in_flight.is_some() {
            log::warn!("[XlsxPreview] request skipped: already in flight");
            return;
        }
        log::info!("[XlsxPreview] requesting sheet {sheet_index} of {}", path.display());
        let path_buf = path.to_path_buf();
        let (tx, rx) = channel();
        self.in_flight = Some(Arc::new(Mutex::new(Some(rx))));
        self.failed = None;
        std::thread::Builder::new()
            .name("xlsx-preview".into())
            .spawn(move || {
                let result = load_sheet(&path_buf, sheet_index);
                log::info!(
                    "[XlsxPreview] load thread done rows={} err={:?}",
                    result.as_ref().map(|d| d.rows.len()).unwrap_or(0),
                    result.as_ref().err()
                );
                let _ = tx.send(result);
            })
            .ok();
    }
}

// ── Zoom persistence (small file in the app data dir) ──────────────────────

pub(crate) mod zoom_store_probe_helpers {
    pub fn load_zoom_default() -> f32 {
        super::load_zoom_default()
    }
    pub fn save_zoom(zoom: f32) {
        super::save_zoom(zoom)
    }
}

fn zoom_store_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| {
        dir.join("mtt-file-manager")
            .join("xlsx_preview_zoom.txt")
    })
}

fn load_zoom_default() -> f32 {
    zoom_store_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|z| (0.25..=5.0).contains(z))
        .unwrap_or(1.0)
}

fn save_zoom(zoom: f32) {
    if let Some(path) = zoom_store_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, format!("{zoom:.3}"));
    }
}

pub fn is_xlsx_previewable(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "xlsx" | "xlsm" | "xls")
}

pub fn render_xlsx_preview(ui: &mut egui::Ui, ctx: &egui::Context, path: &Path) {

    let id = egui::Id::new("xlsx_preview_cache").with(path.as_os_str());
    let mut state: XlsxPanelState = ctx.data_mut(|d| d.get_temp(id).unwrap_or_default());
    if state.zoom < 0.25 {
        // Fresh state: start from the persisted zoom preference.
        state.zoom = load_zoom_default();
    }
    let mut zoom_saved = state.zoom;

    // Poll the in-flight sheet load.
    let rx = state
        .in_flight
        .as_ref()
        .and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()));
    if let Some(rx) = rx {
        match rx.try_recv() {
            Ok(Ok(data)) => {
                state.sheet_index = data.sheet_index;
                state.sheet_names = data.sheet_names;
                state.rows = data.rows;
                state.max_cols = data.max_cols;
                state.total_rows = data.total_rows;
                state.total_cols = data.total_cols;
                state.loaded = true;
                state.failed = None;
                state.in_flight = None;
            }
            Ok(Err(err)) => {
                state.failed = Some(err);
                state.in_flight = None;
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
                state.failed = Some("xlsx render worker died".into());
                state.in_flight = None;
            }
        }
    }

    if !state.loaded && state.in_flight.is_none() && state.failed.is_none() {
        state.request_sheet(path, state.sheet_index);
    }

    let mut show_spinner = false;
    let mut switch_to: Option<usize> = None;
    if let Some(err) = &state.failed {
        ui.colored_label(egui::Color32::from_rgb(232, 17, 35), err);
    } else if !state.loaded {
        show_spinner = true;
        ui.add_space(30.0);
        ui.add(egui::Spinner::new().size(28.0));
        ui.label(egui::RichText::new(rust_i18n::t!("preview.pdf_rendering").to_string()).small());
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    // NOTE: no early `return` above — the state (in-flight receiver included)
    // MUST be persisted every frame or the fetch would restart forever.
    if !show_spinner {
        // Data table in a fixed-height viewport so the sheet tabs below stay
        // pinned at the bottom of the panel (Office-style).
        let table_height = (ui.available_height() - 70.0).max(120.0);
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        let zoom = state.zoom();
        egui::ScrollArea::both()
            .id_salt(("xlsx_preview_table", path.as_os_str()))
            .auto_shrink([false, false])
            .max_height(table_height)
            .show(ui, |ui| {
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .vscroll(false)
                    .min_scrolled_height(0.0);
                table = table.column(Column::auto().resizable(false));
                // Column width = longest content scaled by zoom (fonts and
                // widths scale together); resizable lets the user adjust.
                let zoom = state.zoom();
                for c in 0..state.max_cols {
                    let chars = *state.col_chars.get(c).unwrap_or(&8) as f32;
                    let w = (chars * 9.0 * zoom + 18.0).clamp(36.0, 1200.0);
                    table = table.column(Column::initial(w).resizable(true));
                }
                table
                    .header((24.0 * state.zoom()).max(18.0), |mut header| {
                        header.col(|ui| {
                            ui.strong("行");
                        });
                        for c in 0..state.max_cols {
                            header.col(|ui| {
                                ui.strong(column_letter(c));
                            });
                        }
                    })
                    .body(|mut body| {
                        for (row_idx, row) in state.rows.iter().enumerate() {
                            body.row((24.0 * state.zoom()).max(18.0), |mut row| {
                                row.col(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{}", row_idx + 1))
                                            .small()
                                            .weak(),
                                    );
                                });
                                for c in 0..state.max_cols {
                                    row.col(|ui| {
                                        let text = state
                                            .rows
                                            .get(row_idx)
                                            .and_then(|r| r.get(c))
                                            .cloned()
                                            .unwrap_or_default();
                                        // Visible cell grid: a boxed frame per
                                        // cell (egui tables draw no lines).
                                        egui::Frame::new()
                                            .stroke(stroke)
                                            .inner_margin(egui::Margin::symmetric(4, 3))
                                            .show(ui, |ui| {
                                                if text.is_empty() {
                                                    ui.label(" ");
                                                } else {
                                                    ui.label(
                                                        egui::RichText::new(text)
                                                            .size((13.0 * zoom).max(9.0)),
                                                    );
                                                }
                                            });
                                    });
                                }
                            });
                        }
                    });
            });
    }

    if state.total_rows > state.rows.len() || state.total_cols > state.max_cols {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!(
                "预览仅显示前 {} 行 × {} 列（共 {} 行 × {} 列），完整内容请打开阅读器",
                state.rows.len(),
                state.max_cols,
                state.total_rows,
                state.total_cols
            ))
            .small()
            .color(egui::Color32::GRAY),
        );
    }

    ui.add_space(6.0);
    // Bottom sheet tabs (Office-style). Returns the clicked sheet index.
    switch_to = render_sheet_tabs(ui, &state.sheet_names, state.sheet_index, state.in_flight.is_some(), &mut state.zoom);

    // Persist the zoom once it settles (small file in the app data dir).
    if (state.zoom - zoom_saved).abs() > 0.001 {
        save_zoom(state.zoom);
        zoom_saved = state.zoom;
    }

    if let Some(idx) = switch_to {
        if idx != state.sheet_index {
            state.sheet_index = idx;
            state.loaded = false;
            state.rows = Vec::new();
            state.failed = None;
            state.request_sheet(path, idx);
            ctx.request_repaint();
        }
    }

    // Persist the panel state EVERY frame — losing this (e.g. via an early
    // return) makes the next frame start from a fresh state and re-request
    // the sheet forever (spinner never resolves).
    ctx.data_mut(|d| d.insert_temp(id, state));
}

/// Bottom sheet tab bar (Office-style). Returns the clicked sheet index.
fn render_sheet_tabs(
    ui: &mut egui::Ui,
    sheet_names: &[String],
    sheet_index: usize,
    busy: bool,
    zoom: &mut f32,
) -> Option<usize> {
    if sheet_names.is_empty() {
        return None;
    }
    ui.separator();
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        for (idx, name) in sheet_names.iter().enumerate() {
            let selected = idx == sheet_index;
            if ui
                .add_enabled(!busy, egui::Button::selectable(selected, name))
                .clicked()
            {
                clicked = Some(idx);
            }
        }
        // Excel-style zoom slider, pinned to the right edge of the tab row.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!("{:.0}%", *zoom * 100.0))
                    .small()
                    .weak(),
            );
            ui.add(egui::Slider::new(zoom, 0.5..=3.0).show_value(false));
        });
    });
    clicked
}
