use crate::app::navigation_state::ThemeMode;
use eframe::egui::{Color32, Context, Stroke, Visuals};

pub fn background(ctx: &Context, legacy: Color32) -> Color32 {
    super::current_palette(ctx).map_or(legacy, |p| p.background)
}

pub fn surface(ctx: &Context, legacy: Color32) -> Color32 {
    super::current_palette(ctx).map_or(legacy, |p| p.surface)
}

pub fn raised(ctx: &Context, legacy: Color32) -> Color32 {
    super::current_palette(ctx).map_or(legacy, |p| p.raised)
}

pub fn hover(ctx: &Context, legacy: Color32) -> Color32 {
    super::current_palette(ctx).map_or(legacy, |p| p.hover)
}

pub fn border(ctx: &Context, legacy: Color32) -> Color32 {
    super::current_palette(ctx).map_or(legacy, |p| p.border)
}

/// Semantic colors shared by the file manager and standalone viewer chrome.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub input: Color32,
    pub raised: Color32,
    pub hover: Color32,
    pub active: Color32,
    pub border: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub selection_text: Color32,
}

pub fn for_mode(mode: ThemeMode) -> Option<Palette> {    match mode {
        ThemeMode::DarkDeep => Some(Palette {
            background: Color32::from_gray(24),
            surface: Color32::from_gray(30),
            input: Color32::from_gray(20),
            raised: Color32::from_gray(37),
            hover: Color32::from_gray(44),
            active: Color32::from_gray(54),
            border: Color32::from_gray(64),
            text: Color32::from_gray(230),
            secondary: Color32::from_gray(170),
            accent: Color32::from_rgb(96, 173, 255),
            selection: Color32::from_rgb(35, 62, 94),
            selection_text: Color32::from_rgb(225, 237, 255),
        }),
        ThemeMode::GitHubDark => Some(Palette {
            background: Color32::from_rgb(13, 17, 23),
            surface: Color32::from_rgb(22, 27, 34),
            input: Color32::from_rgb(13, 17, 23),
            raised: Color32::from_rgb(33, 38, 45),
            hover: Color32::from_rgb(41, 46, 54),
            active: Color32::from_rgb(48, 54, 61),
            border: Color32::from_rgb(48, 54, 61),
            text: Color32::from_rgb(230, 237, 243),
            secondary: Color32::from_rgb(157, 167, 179),
            accent: Color32::from_rgb(88, 166, 255),
            selection: Color32::from_rgb(31, 58, 96),
            selection_text: Color32::from_rgb(230, 237, 243),
        }),
        _ => None,
    }
}

/// Derives a palette from the colors Windows itself paints file lists with
/// (`DarkMode_Explorer` ItemsView + the user's accent color). Used by
/// `ThemeMode::System` so the app visually matches Explorer in both modes.
/// Returns `None` when uxtheme/registry cannot provide the base colors, and
/// the caller keeps its legacy fallback.
pub fn from_system(dark: bool) -> Option<Palette> {
    let colors =
        crate::infrastructure::windows::app_theme::read_system_items_colors(dark)?;
    let background = Color32::from_rgb(
        colors.background[0],
        colors.background[1],
        colors.background[2],
    );
    let text = Color32::from_rgb(colors.text[0], colors.text[1], colors.text[2]);

    // Blend helper in RGB; the system colors are near-grayscale so plain
    // mixing tracks the Explorer tones closely.
    let mix = |a: Color32, b: Color32, t: f32| -> Color32 {
        let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
    };
    let white = Color32::WHITE;
    let black = Color32::BLACK;
    // (delta color, amount) pairs flip direction with the mode so surfaces
    // stay layered like Explorer: panel < raised < hover < active.
    let step = |amount: f32| -> Color32 {
        if dark {
            mix(background, white, amount)
        } else {
            mix(background, black, amount)
        }
    };
    let surface = step(0.03);
    let input = if dark { mix(background, black, 0.10) } else { background };
    let raised = step(0.06);
    let hover = step(0.09);
    let active = step(0.13);
    let border = step(0.17);
    let secondary = mix(text, background, 0.42);

    let accent = crate::infrastructure::windows::app_theme::read_system_accent_color()
        .map(|c| Color32::from_rgb(c[0], c[1], c[2]))
        .unwrap_or_else(|| {
            if dark {
                Color32::from_rgb(96, 173, 255)
            } else {
                Color32::from_rgb(0, 120, 215)
            }
        });
    let selection = mix(background, accent, 0.35);
    let selection_text = mix(text, background, 0.12);

    Some(Palette {
        background,
        surface,
        input,
        raised,
        hover,
        active,
        border,
        text,
        secondary,
        accent,
        selection,
        selection_text,
    })
}

impl Palette {
    pub fn apply(self, visuals: &mut Visuals) {
        visuals.window_fill = self.surface;
        visuals.panel_fill = self.background;
        visuals.extreme_bg_color = self.input;
        visuals.faint_bg_color = self.surface;
        visuals.code_bg_color = self.surface;
        visuals.window_stroke = Stroke::new(1.0, self.border);
        visuals.override_text_color = Some(self.text);
        visuals.weak_text_color = Some(self.secondary);
        visuals.hyperlink_color = self.accent;
        visuals.selection.bg_fill = self.selection;
        visuals.selection.stroke = Stroke::new(1.0, self.selection_text);

        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = self.surface;
        widgets.noninteractive.weak_bg_fill = self.surface;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        for (widget, fill) in [
            (&mut widgets.inactive, self.raised),
            (&mut widgets.hovered, self.hover),
            (&mut widgets.active, self.active),
            (&mut widgets.open, self.raised),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = Stroke::new(1.0, self.border);
            widget.fg_stroke = Stroke::new(1.0, self.text);
        }
    }
}
