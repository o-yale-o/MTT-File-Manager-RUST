//! Read-only text preview inside the preview panel.
//!
//! Supports UTF-8 / UTF-16 (BOM) / ANSI (GBK via MultiByteToWideChar) and
//! caps how many bytes are shown so huge logs do not stall the UI. Content
//! is loaded once per path and cached in egui's temp memory.

use std::path::Path as StdPath;
use std::sync::Arc;

use eframe::egui;

/// Bytes we are willing to read & render. Larger files show a truncation note.


/// Extensions that always get a plain-text preview (read-only). On top of
/// this list, any extension syntect knows (via the embedded defaults plus the
/// MadEdit extras in `syntaxes/`) is previewed as text with highlighting.
pub fn is_text_previewable(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "txt" | "log" | "md" | "markdown" | "bat" | "cmd" | "ps1" | "psm1" | "vbs" | "ini"
            | "inf" | "cfg" | "conf" | "config" | "toml" | "ztrtoml" | "yaml" | "yml" | "json"
            | "xml" | "csv" | "tsv" | "reg" | "url" | "nfo" | "srt" | "ass" | "lrc" | "hosts"
            | "properties" | "env" | "gitignore" | "gitattributes" | "editorconfig" | "ignore"
            | "nut" | "rs" | "py" | "pyw" | "js" | "mjs" | "ts" | "c" | "h" | "cpp" | "hpp"
            | "cxx" | "cs" | "java" | "kt" | "kts" | "go" | "rb" | "php" | "sh" | "bash"
            | "zsh" | "html" | "htm" | "xhtml" | "css" | "scss" | "less" | "sql" | "lua"
            | "asm" | "s" | "diff" | "patch" | "sln" | "vcxproj" | "csproj" | "gradle"
            | "cmake" | "dockerfile" | "makefile" | "license" | "readme" | "changelog" | "todo"
    ) {
        return true;
    }
    // Fall back to syntect's knowledge (covers ~200 languages' extensions we
    // did not enumerate here).
    syntax_known_for_extension(&lower)
}

/// Files above this size use the virtualized viewer (render only visible
/// lines); below it the whole text is one styled widget.
const VIRTUAL_THRESHOLD_BYTES: usize = 2 * 1024 * 1024;

struct CachedText {
    text: Arc<String>,
    /// Total file size in bytes when it was read.
    total_size: u64,
    truncated: bool,
    error: Option<String>,
    /// Virtualization data for big files: byte offset of each line start
    /// (line i spans offsets[i]..offsets[i+1]) and the longest line in chars
    /// (drives the horizontal scroll extent).
    virtual_lines: Option<Arc<VirtualLines>>,
}

#[derive(Default)]
struct VirtualLines {
    /// Byte offsets of line starts, plus the end offset appended last.
    offsets: Vec<u32>,
    /// Longest line in characters (monospace approximation for h-scroll).
    max_line_chars: usize,
}

/// Shared loader result used by text and markdown previews.
pub(crate) struct SharedText {
    pub text: Arc<String>,
    pub truncated: bool,
    pub error: Option<String>,
    pub virtual_lines: Option<Arc<VirtualLines>>,
}

/// Load (with per-path cache) the displayable text of `path`.
pub(crate) fn cached_text(ctx: &egui::Context, path: &StdPath) -> SharedText {
    let id = egui::Id::new("text_preview_cache").with(path.as_os_str());
    ctx.data_mut(|d| {
        if let Some(hit) = d.get_temp::<Arc<CachedText>>(id) {
            return SharedText {
                text: hit.text.clone(),
                truncated: hit.truncated,
                error: hit.error.clone(),
                virtual_lines: hit.virtual_lines.clone(),
            };
        }
        let loaded: Arc<CachedText> = Arc::new(load_text(path));
        let shared = SharedText {
            text: loaded.text.clone(),
            truncated: loaded.truncated,
            error: loaded.error.clone(),
            virtual_lines: loaded.virtual_lines.clone(),
        };
        d.insert_temp(id, loaded);
        shared
    })
}

/// Text preview renders with the embedded Fixedsys Excelsior 3.01 face at a
/// fixed 16 px size (see `fonts.rs`: fsex300 is first in the Monospace family).
const PREVIEW_FONT_SIZE: f32 = 16.0;

/// egui cannot shape U+0009 TAB: neither egui's embedded fonts nor the CJK
/// fallbacks carry a tab glyph, so every tab paints as a tofu box (MadEdit
/// implements tab stops itself, which is why it renders the same file fine).
/// Expand each tab to the next TAB_STOP_COLS column before rendering.
const TAB_STOP_COLS: usize = 4;

/// Expand tabs in one line to `TAB_STOP_COLS` column stops.
fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_owned();
    }
    let mut out = String::with_capacity(line.len() + 16);
    let mut col = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let next = (col / TAB_STOP_COLS + 1) * TAB_STOP_COLS;
            while col < next {
                out.push(' ');
                col += 1;
            }
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

/// Win11-Explorer-style: show the file content read-only, white-paper look.
pub fn render_text_preview(ui: &mut egui::Ui, path: &StdPath) {
    let cached = cached_text(ui.ctx(), path);

    let text = cached.text;
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
            // Fixed viewport with NO soft wrap: every file line stays exactly
            // one visual line; long lines are reached by horizontal scrolling.
            if let Some(virt) = cached.virtual_lines.clone() {
                // Big file: render only the visible lines (MadEdit-style
                // virtualization — file size no longer matters).
                render_virtual_text(ui, path, &text, &virt, text_color);
            } else {
                // Small file: expand tabs once per path and cache, then render
                // the whole text (highlighted or plain) as one widget.
                let text = cached_expanded_text(ui.ctx(), path, &text);
                let highlighted = highlighted_job(ui.ctx(), path, &text, dark_mode);
                egui::ScrollArea::both()
                    .id_salt(("text_preview_scroll", path.as_os_str()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(job) = highlighted.as_ref() {
                            // syntect colors carry the text color; TextFormat's
                            // font_id keeps the monospace look.
                            let text_widget = egui::WidgetText::from(job.clone());
                            ui.add(egui::Label::new(text_widget).extend().selectable(true));
                        } else {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&*text)
                                        .monospace()
                                        .size(PREVIEW_FONT_SIZE),
                                )
                                .extend()
                                .selectable(true),
                            );
                        }
                    });
            }
        });
}

/// Tab-expanded copy of the cached text, cached per path (the expansion is
/// deterministic, so it only runs on first render of each file).
fn cached_expanded_text(ctx: &egui::Context, path: &StdPath, text: &str) -> Arc<String> {
    let id = egui::Id::new("text_preview_expanded").with(path.as_os_str());
    ctx.data_mut(|d| {
        if let Some(hit) = d.get_temp::<Arc<String>>(id) {
            return hit;
        }
        let expanded: Arc<String> = Arc::new(expand_tabs(text));
        d.insert_temp(id, expanded.clone());
        expanded
    })
}

/// Virtualized viewer for big files: reserves the full scroll extent, then
/// lays out only the lines currently inside the viewport each frame.
fn render_virtual_text(
    ui: &mut egui::Ui,
    path: &StdPath,
    text: &str,
    virt: &VirtualLines,
    text_color: egui::Color32,
) {
    // Fixed size: the preview always renders at PREVIEW_FONT_SIZE regardless
    // of the global Monospace text style.
    let font_id = egui::FontId::monospace(PREVIEW_FONT_SIZE);
    let row_h = ui.fonts_mut(|f| f.row_height(&font_id)) + 4.0;

    let line_count = virt.offsets.len().saturating_sub(1);
    let content_w = ui.available_width().max(
        (virt.max_line_chars as f32)
            * ui.fonts_mut(|f| f.glyph_width(&font_id, 'W').max(1.0)),
    );

    egui::ScrollArea::both()
        .id_salt(("text_preview_virtual", path.as_os_str()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (content_rect, _) = ui.allocate_exact_size(
                egui::vec2(content_w, row_h * line_count as f32),
                egui::Sense::hover(),
            );
            let clip = ui.clip_rect();
            let first = (((clip.top() - content_rect.top()) / row_h).floor().max(0.0)) as usize;
            let last = (((clip.bottom() - content_rect.top()) / row_h).ceil() as usize + 1)
                .min(line_count);

            for i in first..last {
                let start = virt.offsets[i] as usize;
                let end = virt.offsets[i + 1] as usize;
                let line = &text[start..end];
                let line = line.strip_suffix('\n').unwrap_or(line);
                let line = line.strip_suffix('\r').unwrap_or(line);
                // Expand tabs per visible line (cheap: only what is on screen).
                let expanded;
                let line = if line.contains('\t') {
                    expanded = expand_tabs(line);
                    &expanded[..]
                } else {
                    line
                };
                let galley = ui.painter().layout_no_wrap(
                    line.to_owned(),
                    font_id.clone(),
                    text_color,
                );
                ui.painter().galley(
                    egui::pos2(content_rect.left(), content_rect.top() + i as f32 * row_h),
                    galley,
                    text_color,
                );
            }
        });
}

// == Syntax highlighting (syntect) 閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓閳光偓

/// Highlighted galley job, cached per path. `None` = no matching syntax.
#[allow(clippy::type_complexity)]
fn highlighted_job(
    ctx: &egui::Context,
    path: &StdPath,
    text: &str,
    dark_mode: bool,
) -> Option<Arc<egui::text::LayoutJob>> {
    // Dotfile-aware extension (mod.rs): `.gitignore` → "gitignore", etc.
    let ext = super::preview_ext(path);
    let id = egui::Id::new("text_highlight_cache")
        .with(path.as_os_str())
        .with(ext.as_str())
        .with(dark_mode);

    let cached = ctx.data_mut(|d| {
        let hit = d.get_temp::<Arc<egui::text::LayoutJob>>(id);
        if hit.is_some() {
            hit
        } else {
            let built = build_highlight_job(text, &ext, dark_mode).map(Arc::new);
            if let Some(job) = &built {
                d.insert_temp(id, job.clone());
            }
            built
        }
    });
    cached
}

/// Build a colored `LayoutJob` for `text` using syntect's default theme sets.
/// Returns `None` when no syntax definition matches the extension.
fn build_highlight_job(
    text: &str,
    ext: &str,
    dark_mode: bool,
) -> Option<egui::text::LayoutJob> {
    use syntect::easy::HighlightLines;
    use syntect::highlighting::{Theme, ThemeSet};
    use syntect::parsing::SyntaxSet;

    static SYNTAX_SET: std::sync::OnceLock<SyntaxSet> = std::sync::OnceLock::new();
    static THEME_LIGHT: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();
    static THEME_DARK: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();

    // One-time load: embedded default syntax definitions (~200 languages,
    // same set as MadEdit) merged with the MadEdit extras shipped in
    // `syntaxes/` next to the executable (INI, GitIgnore, Squirrel/.nut,
    // PowerShell).
    let _syntax_set = SYNTAX_SET.get_or_init(load_syntax_set);

    let theme = if dark_mode {
        THEME_DARK.get_or_init(|| ThemeSet::load_defaults().themes["base16-ocean.dark"].clone())
    } else {
        THEME_LIGHT
            .get_or_init(|| ThemeSet::load_defaults().themes["base16-ocean.light"].clone())
    };

    let syntax_set = SYNTAX_SET.get().expect("syntax set initialized");
    // Extensions whose syntax lives under a different name (MadEdit does the
    // same: TOML highlighting is provided by the YAML grammar). Makefile
    // variants resolve to the default "Makefile" grammar by name.
    const SYNTAX_ALIASES: &[(&str, &str)] = &[
        ("toml", "YAML"),
        ("ztrtoml", "YAML"),
        ("tml", "YAML"),
        ("makefile", "Makefile"),
        ("gnumakefile", "Makefile"),
        ("omakefile", "Makefile"),
        ("mkfile", "Makefile"),
        ("mk", "Makefile"),
        ("mak", "Makefile"),
        ("mf", "Makefile"),
    ];
    let syntax = syntax_set
        .find_syntax_by_extension(ext)
        .or_else(|| syntax_set.find_syntax_by_extension(ext.trim_start_matches('.')))
        .or_else(|| {
            SYNTAX_ALIASES
                .iter()
                .find(|(alias, _)| ext.eq_ignore_ascii_case(alias))
                .and_then(|(_, name)| syntax_set.find_syntax_by_name(name))
        })?;

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut job = egui::text::LayoutJob::default();
    let font_id = egui::FontId::monospace(PREVIEW_FONT_SIZE);

    for line in syntect::util::LinesWithEndings::from(text) {
        let ranges = highlighter
            .highlight_line(line, syntax_set)
            .ok()?;
        for (style, chunk) in ranges {
            // Strip trailing newline chunk text to keep galley tidy per line.
            let chunk = chunk.trim_end_matches(['\r', '\n']);
            if chunk.is_empty() {
                continue;
            }
            job.append(
                chunk,
                0.0,
                egui::TextFormat {
                    font_id: font_id.clone(),
                    color: egui::Color32::from_rgb(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    ),
                    ..Default::default()
                },
            );
        }
        // Re-add the newline the chunk trimming removed.
        job.append("\n", 0.0, egui::TextFormat { font_id: font_id.clone(), ..Default::default() });
    }
    Some(job)
}

/// Embedded defaults + MadEdit's extra syntax definitions compiled into the
/// binary (no external `syntaxes/` folder needed at runtime).
fn load_syntax_set() -> syntect::parsing::SyntaxSet {
    let mut builder =
        syntect::parsing::SyntaxSet::load_defaults_newlines().into_builder();

    const EXTRA_SYNTAXES: &[(&str, &str)] = &[
        ("INI.sublime-syntax", include_str!("../../../assets/syntaxes/INI.sublime-syntax")),
        ("GitIgnore.sublime-syntax", include_str!("../../../assets/syntaxes/GitIgnore.sublime-syntax")),
        ("Squirrel.sublime-syntax", include_str!("../../../assets/syntaxes/Squirrel.sublime-syntax")),
        ("PowerShell.sublime-syntax", include_str!("../../../assets/syntaxes/PowerShell.sublime-syntax")),
    ];
    for (name, data) in EXTRA_SYNTAXES {
        match syntect::parsing::SyntaxDefinition::load_from_str(data, true, None) {
            Ok(def) => builder.add(def),
            Err(err) => {
                log::warn!("[TEXT-PREVIEW] failed to parse embedded syntax {name}: {err}");
            }
        }
    }

    builder.build()
}

/// True when syntect's merged syntax set knows the extension. Used to decide
/// whether an unknown-to-us extension still deserves a text preview.
pub(crate) fn syntax_known_for_extension(ext: &str) -> bool {
    use std::sync::OnceLock;
    static SYNTAX_SET: OnceLock<syntect::parsing::SyntaxSet> = OnceLock::new();
    let set = SYNTAX_SET.get_or_init(load_syntax_set);
    set.find_syntax_by_extension(&ext.to_ascii_lowercase())
        .is_some()
}

fn load_text(path: &StdPath) -> CachedText {
    match std::fs::read(path) {
        Ok(bytes) => {
            let total_size = bytes.len() as u64;
            // No truncation: big files load in full and are rendered through
            // the virtualized viewer (only visible lines hit the layout pass).
            let take = bytes.len();
            // Back the cut off to the nearest multi-byte char boundary so the
            // strict UTF-8 check does not fail on a half character (a 600 KB
            // UTF-8 file cut at 256 KB would otherwise be misdetected).
            let mut end = take;
            while end > 0
                && end < bytes.len()
                && (bytes[end] & 0xC0) == 0x80
            {
                end -= 1;
            }
            let slice = &bytes[..end];
            // Align the cut to a UTF-16 boundary when the file has a BOM.
            let slice = if (slice.len() % 2 == 1) && has_utf16_bom(&bytes) {
                &slice[..slice.len() - 1]
            } else {
                slice
            };
            let text = decode(slice, &bytes);
            let virtual_lines = if total_size as usize > VIRTUAL_THRESHOLD_BYTES {
                Some(Arc::new(build_virtual_lines(&text)))
            } else {
                None
            };
            CachedText {
                text: Arc::new(text),
                total_size,
                truncated: false,
                error: None,
                virtual_lines,
            }
        }
        Err(e) => CachedText {
            text: Arc::new(String::new()),
            total_size: 0,
            truncated: false,
            error: Some(format!("{}: {}", path.display(), e)),
            virtual_lines: None,
        },
    }
}

/// Split `text` into line-start byte offsets (1 per line, plus the end).
fn build_virtual_lines(text: &str) -> VirtualLines {
    let mut offsets = vec![0u32];
    let mut max_line_chars = 0usize;
    let mut line_start = 0usize;
    let mut line_tabs = 0usize;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            let len = idx - line_start;
            // Tabs expand to TAB_STOP_COLS columns at render time; account for
            // the extra width in the horizontal scroll extent.
            max_line_chars = max_line_chars.max(len + line_tabs * (TAB_STOP_COLS - 1));
            offsets.push((idx + 1) as u32);
            line_start = idx + 1;
            line_tabs = 0;
        } else if ch == '\t' {
            line_tabs += 1;
        }
    }
    // Final line without trailing newline.
    if line_start < text.len() {
        let len = text.len() - line_start;
        max_line_chars = max_line_chars.max(len + line_tabs * (TAB_STOP_COLS - 1));
    }
    offsets.push(text.len() as u32);
    VirtualLines {
        offsets,
        // Rough cap: multibyte chars overcount slightly; harmless for scroll extent.
        max_line_chars: max_line_chars.min(u32::MAX as usize),
    }
}

fn has_utf16_bom(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF])
}

/// Auto-detect encoding and decode, following the same chain as MadEdit
/// (`encoding_rs` + `chardetng`):
/// BOM → BOM-less UTF-16 guess → strict UTF-8 → content sniffing
/// (GBK/Big5/Shift_JIS/…) → East-Asian candidate encodings → lossy GBK.
fn decode(slice: &[u8], _all: &[u8]) -> String {
    use encoding_rs::{GB18030, BIG5, EUC_JP, EUC_KR, SHIFT_JIS, UTF_8};

    // 1. BOM: handle the three BOMs explicitly.
    if slice.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let (text, _, _) = UTF_8.decode(&slice[3..]);
        return text.into_owned();
    }
    if slice.starts_with(&[0xFF, 0xFE]) || slice.starts_with(&[0xFE, 0xFF]) {
        let enc = if slice[0] == 0xFF { encoding_rs::UTF_16LE } else { encoding_rs::UTF_16BE };
        let (text, _, _) = enc.decode(&slice[2..]);
        return text.into_owned();
    }

    // 2. BOM-less UTF-16 guess (many zeros on the odd/even lanes).
    if let Some(enc) = guess_utf16(slice) {
        let (text, _, had_errors) = enc.decode(slice);
        if !had_errors {
            return text.into_owned();
        }
    }

    // 3. Strict UTF-8 (no replacement chars).
    if let Ok(text) = std::str::from_utf8(slice) {
        return text.to_owned();
    }

    // 4. Content sniffing — avoids the classic GBK-decoded-as-Big5 mojibake.
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(slice, true);
    let guessed = detector.guess(None, false);
    if guessed != UTF_8
        && guessed != encoding_rs::UTF_16LE
        && guessed != encoding_rs::UTF_16BE
    {
        let (text, _, had_errors) = guessed.decode(slice);
        if !had_errors {
            return text.into_owned();
        }
    }

    // 5. Candidate encodings: first one that decodes without errors wins.
    for enc in [GB18030, BIG5, SHIFT_JIS, EUC_KR, EUC_JP] {
        let (text, _, had_errors) = enc.decode(slice);
        if !had_errors {
            return text.into_owned();
        }
    }

    // 6. Last resort: ANSI code page (GBK on zh-CN systems), lossy.
    ansi_to_string(slice)
}

fn guess_utf16(sample: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    if sample.len() < 4 {
        return None;
    }
    let pairs = sample.len() / 2;
    let even_zero = sample.iter().step_by(2).filter(|&&b| b == 0).count();
    let odd_zero = sample.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
    if odd_zero * 10 > pairs * 4 && even_zero * 50 < pairs {
        Some(encoding_rs::UTF_16LE)
    } else if even_zero * 10 > pairs * 4 && odd_zero * 50 < pairs {
        Some(encoding_rs::UTF_16BE)
    } else {
        None
    }
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

#[cfg(test)]
mod encoding_tests {
    /// A >256KB UTF-8 file cut mid-character must still decode as UTF-8
    /// (regression: the raw 256KB cut broke a char → misdetected as GBK).
    #[test]
    fn large_utf8_cut_mid_char_decodes_cleanly() {
        let text = "中文内容测试 English words too.\n".repeat(12000);
        let bytes = text.into_bytes();
        assert!(bytes.len() > 256 * 1024);
        let take = 256 * 1024;
        let mut end = take;
        while (bytes[end] & 0xC0) == 0x80 {
            end -= 1;
        }
        let decoded = super::decode(&bytes[..end], &bytes);
        assert!(decoded.contains("中文内容测试"));
        assert!(!decoded.contains('\u{FFFD}'), "replacement char found — misdecoded");
    }

    /// GBK content (invalid as UTF-8) must be content-sniffed, not lossy-UTF8.
    #[test]
    fn gbk_content_decodes() {
        let (gbk, _, _) = encoding_rs::GBK.encode("D:项目路径 简体中文测试 [INFO] ok");
        let decoded = super::decode(&gbk, &gbk);
        assert_eq!(decoded, "D:项目路径 简体中文测试 [INFO] ok");
    }

    /// Regression: full-file load where `end == len` must not index out of
    /// bounds in the char-boundary backoff (crashed the app on click).
    #[test]
    fn full_load_of_virtual_file_does_not_panic() {
        let dir = std::env::temp_dir().join("text_preview_virtual_probe.txt");
        // >2MB, ends with a multi-byte char and no trailing newline.
        let content = "中文内容测试 English.\n".repeat(140000);
        std::fs::write(&dir, &content).unwrap();
        let loaded = super::load_text(&dir);
        assert!(loaded.error.is_none());
        let lines = loaded.virtual_lines.expect("virtual path engaged");
        assert!(lines.offsets.len() > 100000);
        assert_eq!(*loaded.text, content);
        let _ = std::fs::remove_file(&dir);
    }
}

#[cfg(test)]
mod toml_probe {
    #[test]
    fn probe_toml_syntax() {
        let set = super::load_syntax_set();
        for ext in ["toml", "ztrtoml", "nut", "ini", "ignore"] {
            let found = set
                .find_syntax_by_extension(ext)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "<none>".into());
            println!("ext {ext} -> {found}");
        }
        let toml_like: Vec<String> = set
            .syntaxes()
            .iter()
            .filter(|s| s.file_extensions.iter().any(|e| e.contains("toml")))
            .map(|s| format!("{} {:?}", s.name, s.file_extensions))
            .collect();
        println!("toml-like syntaxes: {toml_like:#?}");
    }
}

#[cfg(test)]
mod gitignore_probe {
    #[test]
    fn probe_gitignore_syntax() {
        let set = super::load_syntax_set();
        println!("by ext 'gitignore': {:?}", set.find_syntax_by_extension("gitignore").map(|s| s.name.clone()));
        println!("by name 'Git Ignore': {:?}", set.find_syntax_by_name("Git Ignore").map(|s| s.name.clone()));
    }
}

#[cfg(test)]
mod makefile_tests {
    #[test]
    fn makefile_syntax_resolves_for_extensionless_names_and_make_extensions() {
        let set = super::load_syntax_set();
        assert!(set.find_syntax_by_name("Makefile").is_some(), "syntect defaults must ship the Makefile grammar");
        for ext in ["makefile", "gnumakefile", "mk", "mak"] {
            let syntax = set
                .find_syntax_by_extension(ext)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| format!("<none:{ext}>"));
            assert_eq!(syntax, "Makefile", "ext {ext}");
        }
        // Full extensionless highlight job builds without error.
        let job = super::build_highlight_job(
            "CC = gcc\n\nall: app\n\n# comment\napp: main.o\n\t$(CC) -o $@ $<\n",
            "makefile",
            true,
        );
        assert!(job.is_some(), "makefile highlight job must build");
    }
}
