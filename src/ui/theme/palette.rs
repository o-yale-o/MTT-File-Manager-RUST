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
            surface: Color32::from_gray(38),
            input: Color32::from_gray(18),
            raised: Color32::from_gray(44),
            hover: Color32::from_gray(52),
            active: Color32::from_gray(62),
            border: Color32::from_gray(70),
            text: Color32::from_gray(230),
            secondary: Color32::from_gray(170),
            accent: Color32::from_rgb(96, 173, 255),
            selection: Color32::from_rgb(35, 62, 94),
            selection_text: Color32::from_rgb(225, 237, 255),
        }),
        ThemeMode::GitHubDark => Some(Palette {
            background: Color32::from_rgb(13, 17, 23),
            surface: Color32::from_rgb(31, 36, 42),
            input: Color32::from_rgb(13, 17, 23),
            raised: Color32::from_rgb(37, 42, 50),
            hover: Color32::from_rgb(44, 50, 58),
            active: Color32::from_rgb(52, 58, 66),
            border: Color32::from_rgb(60, 66, 74),
            text: Color32::from_rgb(230, 237, 243),
            secondary: Color32::from_rgb(157, 167, 179),
            accent: Color32::from_rgb(88, 166, 255),
            selection: Color32::from_rgb(31, 58, 96),
            selection_text: Color32::from_rgb(230, 237, 243),
        }),
        _ => None,
    }
}

/// Derives a palette from the canonical Windows theme colors
/// (`UISettings.GetColorValue`: Background/Foreground/Accent — the same
/// source WinUI apps use), layered with the **fixed** Fluent deltas:
/// dark `#202020` base → card `#2B2B2B` (+11), stroke `#383838` (+24);
/// light `#FFFFFF` base → card `#F3F3F3` (-12), stroke `#E5E5E5` (-26).
/// Fixed offsets keep the ladder stable no matter what the base color is,
/// unlike proportional mixing. Returns `None` when WinRT/uxtheme cannot
/// provide the base colors; the caller keeps its legacy fallback.
pub fn from_system(dark: bool) -> Option<Palette> {
    use crate::infrastructure::windows::app_theme as native_theme;
    // 1) Documented WinRT palette; 2) Explorer ItemsView sampling; 3) registry accent.
    let base = native_theme::read_system_theme_colors().or_else(|| {
        native_theme::read_system_items_colors(dark).map(|c| native_theme::SystemThemeColors {
            background: c.background,
            foreground: c.text,
            accent: native_theme::read_system_accent_color().unwrap_or([0, 120, 215]),
        })
    })?;
    let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    let background = rgb(base.background);
    let text = rgb(base.foreground);
    let accent = rgb(base.accent);

    // Fixed-step token ladder (Fluent "layer" offsets, in 8-bit gray steps).
    let (surface_d, raised_d, hover_d, active_d, border_d, input_d) = if dark {
        // #202020 → #2B2B2B card … #383838 stroke, input darker than base.
        (11, 16, 20, 32, 24, -8)
    } else {
        // #FFFFFF → #F3F3F3 card … #E5E5E5 stroke, input stays white.
        (-12, -8, -20, -30, -26, 0)
    };
    let step = |delta: i16| -> Color32 {
        let ch = |c: u8| (c as i16 + delta).clamp(0, 255) as u8;
        Color32::from_rgb(
            ch(background.r()),
            ch(background.g()),
            ch(background.b()),
        )
    };
    let surface = step(surface_d); // selected tab / window panels
    let input = step(input_d); // text fields
    let raised = step(raised_d); // inactive buttons
    let hover = step(hover_d); // list/item hover
    let active = step(active_d); // pressed / active header
    let border = step(border_d); // strokes
    // 78% primary / ~55% secondary over the base (Fluent text opacities).
    let mix = |a: Color32, b: Color32, t: f32| -> Color32 {
        let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
    };
    let secondary = mix(text, background, 0.22);

    let selection = mix(background, accent, if dark { 0.32 } else { 0.22 });
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
