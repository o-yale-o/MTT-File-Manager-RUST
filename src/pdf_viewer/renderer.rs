//! PDF page rendering and text extraction using Pdfium.

use std::path::PathBuf;
use std::sync::OnceLock;

use pdfium_render::prelude::*;

static PDFIUM_BIND_STATUS: OnceLock<Result<(), String>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfTextBounds {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl PdfTextBounds {
    pub fn from_points(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self {
            left: left.min(right),
            right: left.max(right),
            top: top.max(bottom),
            bottom: top.min(bottom),
        }
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.top - self.bottom
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.bottom < other.top
            && self.top > other.bottom
    }
}

#[derive(Clone, Debug)]
pub struct PdfTextSegment {
    pub bounds: PdfTextBounds,
}

/// Error returned when opening a PDF document.
#[derive(Debug)]
pub enum PdfOpenError {
    /// The document is password-protected and no (or wrong) password was provided.
    PasswordRequired,
    /// Any other failure (library load error, corrupt file, etc.).
    Other(String),
}

impl PdfOpenError {
    pub fn is_password_required(&self) -> bool {
        matches!(self, Self::PasswordRequired)
    }
}

impl std::fmt::Display for PdfOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PasswordRequired => write!(f, "PDF is password-protected"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

/// A rendered PDF page as raw RGBA pixels.
pub struct RenderedPage {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub(super) fn classify_open_error(error: PdfiumError) -> PdfOpenError {
    if matches!(
        error,
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)
    ) {
        PdfOpenError::PasswordRequired
    } else {
        PdfOpenError::Other(format!("LoadPdf: {error}"))
    }
}

/// Execute an operation with a timeout. Spawns a worker thread and waits
/// up to `timeout` for the result. Returns an error on timeout.
#[allow(dead_code)]
pub fn with_timeout<T: Send + 'static>(
    timeout: std::time::Duration,
    op: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("pdf-op-timeout".into())
        .spawn(move || {
            let _ = tx.send(op());
        })
        .map_err(|e| format!("Failed to spawn timeout thread: {e}"))?;

    rx.recv_timeout(timeout)
        .map_err(|_| format!("PDF operation timed out after {}s", timeout.as_secs()))?
}

pub(super) fn pdfium() -> Result<Pdfium, String> {
    match PDFIUM_BIND_STATUS.get_or_init(|| bind_pdfium().map(|_| ())) {
        Ok(()) => Ok(Pdfium::default()),
        Err(err) => Err(err.clone()),
    }
}

fn bind_pdfium() -> Result<Pdfium, String> {
    for candidate in pdfium_library_candidates() {
        match Pdfium::bind_to_library(&candidate) {
            Ok(bindings) => {
                let pdfium = Pdfium::new(bindings);
                return Ok(pdfium);
            }
            Err(PdfiumError::LoadLibraryError(_)) => continue,
            Err(err) => {
                return Err(format!(
                    "Failed to load Pdfium from {}: {err}",
                    candidate.display()
                ));
            }
        }
    }

    match Pdfium::bind_to_system_library() {
        Ok(bindings) => {
            let pdfium = Pdfium::new(bindings);
            Ok(pdfium)
        }
        Err(err) => Err(format!(
            "Failed to load pdfium.dll. Place a compatible Pdfium runtime next to the executable or install it system-wide. Details: {err}"
        )),
    }
}

fn pdfium_library_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(Pdfium::pdfium_platform_library_name_at_path(dir));
        }
    }

    // SEC: Do NOT search the current working directory ("./") for pdfium.dll.
    // A malicious DLL in CWD would be loaded and executed (DLL planting attack).
    candidates
}

pub(super) fn pdfium_rect_to_bounds(rect: PdfRect) -> PdfTextBounds {
    PdfTextBounds::from_points(
        rect.left().value,
        rect.right().value,
        rect.top().value,
        rect.bottom().value,
    )
}

// ── Preview-panel support (Win11-style inline PDF preview) ──────────────────

/// One rendered preview page plus document page count.
pub struct PdfPreviewPage {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub page_count: usize,
    /// Page index actually rendered (clamped).
    pub page_index: usize,
}

/// Render one page of `path` fit to `target_width` pixels.
///
/// Runs on a background thread (called via channel from the preview panel);
/// opens and closes the document per call — acceptable for preview usage.
pub fn render_preview_page(
    path: &std::path::Path,
    page_index: usize,
    target_width: u32,
) -> Result<PdfPreviewPage, String> {
    let pdfium = pdfium()?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| classify_open_error(e).to_string())?;

    let page_count = usize::from(document.pages().len());
    if page_count == 0 {
        return Err("PDF has no pages".into());
    }
    let page_index = page_index.min(page_count - 1);

    let page = document
        .pages()
        .get(page_index as pdfium_render::prelude::PdfPageIndex)
        .map_err(|e| e.to_string())?;

    // Fit-to-width: derive pixel height from the page's point aspect ratio.
    let (w_pts, h_pts) = {
        let rect = document
            .pages()
            .page_size(page_index as pdfium_render::prelude::PdfPageIndex)
            .map_err(|e| e.to_string())?;
        (rect.width().value.max(1.0), rect.height().value.max(1.0))
    };
    let width_px = target_width.max(64);
    let height_px = ((h_pts / w_pts) * width_px as f32).round().max(64.0) as u32;

    let bitmap = page
        .render(
            width_px as pdfium_render::prelude::Pixels,
            height_px as pdfium_render::prelude::Pixels,
            None,
        )
        .map_err(|e| format!("RenderPage: {e}"))?;

    Ok(PdfPreviewPage {
        width: bitmap.width() as u32,
        height: bitmap.height() as u32,
        pixels: bitmap.as_rgba_bytes(),
        page_count,
        page_index,
    })
}
