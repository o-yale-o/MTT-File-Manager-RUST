//! Read-only text preview inside the preview panel.
//!
//! Supports UTF-8 / UTF-16 (BOM) / ANSI (GBK via MultiByteToWideChar) and
//! caps how many bytes are shown so huge logs do not stall the UI. Content
//! is loaded once per path and cached in egui's temp memory.

use std::path::Path as StdPath;
use std::sync::Arc;

use eframe::egui;

/// Bytes we are willing to read & render. Larger files show a truncation note.
const MAX_PREVIEW_BYTES: usize = 256 * 1024;

/// Extensions that get a plain-text preview (read-only).
pub fn is_text_previewable(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "txt" | "md" | "log" | "bat" | "cmd" | "ps1" | "vbs" | "ini" | "cfg" | "conf" | "toml"
            | "yaml" | "yml" | "json" | "xml" | "csv" | "reg" | "url" | "nfo" | "srt" | "ass"
            | "lrc" | "hosts" | "properties" | "env" | "rs" | "py" | "js" | "ts" | "c" | "h"
            | "cpp" | "hpp" | "cs" | "java" | "kt" | "go" | "rb" | "php" | "sh" | "html" | "htm"
            | "css" | "sql" | "lua" | "asm" | "diff" | "patch"
    )
}

struct CachedText {
    text: Arc<String>,
    /// Total file size in bytes when it was read.
    total_size: u64,
    truncated: bool,
    error: Option<String>,
}

/// Win11-Explorer-style: show the file content read-only, white-paper look.
pub fn render_text_preview(ui: &mut egui::Ui, path: &StdPath) {
    let id = egui::Id::new("text_preview_cache").with(path.as_os_str());
    let cached = ui.ctx().data_mut(|d| {
        if let Some(hit) = d.get_temp::<Arc<CachedText>>(id) {
            hit
        } else {
            let loaded = Arc::new(load_text(path));
            d.insert_temp(id, loaded.clone());
            loaded
        }
    });

    if let Some(err) = &cached.error {
        ui.colored_label(egui::Color32::from_rgb(232, 17, 35), err);
        return;
    }

    let dark_mode = ui.visuals().dark_mode;
    // Paper-like background regardless of theme, like Explorer's text preview.
    let paper = if dark_mode {
        egui::Color32::from_rgb(24, 24, 24)
    } else {
        egui::Color32::WHITE
    };
    let text_color = if dark_mode {
        egui::Color32::from_rgb(212, 212, 212)
    } else {
        egui::Color32::from_rgb(32, 32, 32)
    };

    egui::Frame::NONE
        .fill(paper)
        .inner_margin(egui::Margin::same(8))
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.style_mut().visuals.override_text_color = Some(text_color);
            let mut shown = (*cached.text).clone();
            ui.add(
                egui::TextEdit::multiline(&mut shown)
                    .desired_rows(12)
                    .desired_width(ui.available_width())
                    .clip_text(true)
                    .interactive(false),
            );
        });

    if cached.truncated {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(rust_i18n::t!("preview.text_truncated").to_string())
                .small()
                .color(egui::Color32::GRAY),
        );
    }
}

fn load_text(path: &StdPath) -> CachedText {
    match std::fs::read(path) {
        Ok(bytes) => {
            let total_size = bytes.len() as u64;
            let truncated = bytes.len() > MAX_PREVIEW_BYTES;
            let take = bytes.len().min(MAX_PREVIEW_BYTES);
            let slice = &bytes[..take];
            // Align the cut to a UTF-16 boundary when the file has a BOM.
            let slice = if (slice.len() % 2 == 1) && has_utf16_bom(&bytes) {
                &slice[..take - 1]
            } else {
                slice
            };
            let text = decode(slice, &bytes);
            CachedText {
                text: Arc::new(text),
                total_size,
                truncated,
                error: None,
            }
        }
        Err(e) => CachedText {
            text: Arc::new(String::new()),
            total_size: 0,
            truncated: false,
            error: Some(format!("{}: {}", path.display(), e)),
        },
    }
}

fn has_utf16_bom(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF])
}

fn decode(slice: &[u8], all: &[u8]) -> String {
    // BOM detection first.
    if all.starts_with(&[0xFF, 0xFE]) {
        return String::from_utf16_lossy(
            &slice
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        );
    }
    if all.starts_with(&[0xFE, 0xFF]) {
        return String::from_utf16_lossy(
            &slice
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        );
    }
    if all.starts_with(&[0xEF, 0xBB, 0xBF]) || std::str::from_utf8(slice).is_ok() {
        // UTF-8 (BOM stripped) or pure-ASCII / valid UTF-8.
        return String::from_utf8_lossy(if all.starts_with(&[0xEF, 0xBB, 0xBF]) {
            slice.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(slice)
        } else {
            slice
        })
        .into_owned();
    }
    // Fall back to the ANSI code page (GBK on zh-CN systems).
    ansi_to_string(slice)
}

#[cfg(windows)]
fn ansi_to_string(bytes: &[u8]) -> String {
    use windows::Win32::Globalization::{
        MultiByteToWideChar, MULTI_BYTE_TO_WIDE_CHAR_FLAGS,
    };
    const CP_ACP: u32 = 0;
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    let len = unsafe { MultiByteToWideChar(CP_ACP, flags, bytes, None) };
    if len <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut buf = vec![0u16; len as usize];
    let written = unsafe { MultiByteToWideChar(CP_ACP, flags, bytes, Some(&mut buf)) };
    if written <= 0 {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        String::from_utf16_lossy(&buf[..written as usize])
    }
}

#[cfg(not(windows))]
fn ansi_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
