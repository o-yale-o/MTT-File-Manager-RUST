//! Interaction tests for the window tab-list overlay drawn inside the
//! window control buttons.
//!
//! egui resolves clicks against the *previous* frame's widget rects and its
//! double/triple-click detection runs on a frame clock, so every click helper
//! here moves the pointer (hover frame), presses, releases, idles — and the
//! harness advances an explicit `RawInput.time` by `DT` per frame.

use super::window_controls::render_window_controls;
use super::TabBarAction;
use eframe::egui::{self, pos2, vec2, Context, Event, Modifiers, PointerButton, RawInput};

const DT: f64 = 0.05;

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0))
}

fn plain(t: f64) -> RawInput {
    RawInput {
        screen_rect: Some(screen()),
        time: Some(t),
        ..Default::default()
    }
}

fn input(pos: egui::Pos2, pressed: bool, t: f64) -> RawInput {
    RawInput {
        screen_rect: Some(screen()),
        time: Some(t),
        events: vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            },
        ],
        ..Default::default()
    }
}

fn hover(pos: egui::Pos2, t: f64) -> RawInput {
    RawInput {
        screen_rect: Some(screen()),
        time: Some(t),
        events: vec![Event::PointerMoved(pos)],
        ..Default::default()
    }
}

fn render(ctx: &Context, titles: &[String], active: usize, input: RawInput, action: &mut TabBarAction) {
    let _ = ctx.run_ui(input, |ui| {
        render_window_controls(ui, 46.0, 36.0, 10.0, titles, active, action);
    });
}

/// Hover → press → release → idle, advancing the clock.
fn click(
    ctx: &Context,
    titles: &[String],
    active: usize,
    pos: egui::Pos2,
    t: &mut f64,
    action: &mut TabBarAction,
) {
    render(ctx, titles, active, hover(pos, *t), action);
    *t += DT;
    render(ctx, titles, active, input(pos, true, *t), action);
    *t += DT;
    render(ctx, titles, active, input(pos, false, *t), action);
    *t += DT;
    render(ctx, titles, active, plain(*t), action);
    *t += DT;
}

fn toggle_list(ctx: &Context, titles: &[String], active: usize, t: &mut f64, action: &mut TabBarAction) {
    render(ctx, titles, active, plain(*t), action);
    *t += DT;
    click(ctx, titles, active, pos2(18.0, 28.0), t, action);
}

// Row geometry (1000×800 test window): the list panel spans x 680..1000 with
// 4px inner padding; each row is 26px tall + 2px spacing. Grey "其它" first
// (y-center 65), then one row per tab (centers 93 / 121 / 149).
const LABEL_X: f32 = 700.0;
const X_X: f32 = 988.0;
const ROW_Y: [f32; 3] = [93.0, 121.0, 149.0];

fn row_label(index: usize) -> egui::Pos2 {
    pos2(LABEL_X, ROW_Y[index])
}

fn row_x(index: usize) -> egui::Pos2 {
    pos2(X_X, ROW_Y[index])
}

fn grey_x() -> egui::Pos2 {
    pos2(X_X, 65.0)
}

#[test]
fn clicking_a_row_switches_tabs_and_keeps_the_list_open() {
    let ctx = Context::default();
    let titles = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let mut action = TabBarAction::None;
    let mut t = 0.0;

    toggle_list(&ctx, &titles, 0, &mut t, &mut action);

    click(&ctx, &titles, 0, row_label(1), &mut t, &mut action);
    assert!(
        matches!(action, TabBarAction::SwitchTab(1)),
        "row click must switch tabs, got {action:?}"
    );

    click(&ctx, &titles, 0, row_label(2), &mut t, &mut action);
    assert!(
        matches!(action, TabBarAction::SwitchTab(2)),
        "second row click must switch too, got {action:?}"
    );
}

#[test]
fn clicking_the_active_tab_row_is_a_no_op() {
    let ctx = Context::default();
    let titles = vec!["alpha".to_string(), "beta".to_string()];
    let mut action = TabBarAction::None;
    let mut t = 0.0;

    toggle_list(&ctx, &titles, 0, &mut t, &mut action);
    click(&ctx, &titles, 0, row_label(0), &mut t, &mut action);
    assert!(
        matches!(action, TabBarAction::None),
        "clicking the already-active tab must not re-switch, got {action:?}"
    );
}

#[test]
fn rows_stay_clickable_after_the_tab_batch_is_replaced() {
    // Mirrors a quick-group switch: same overlay, brand-new titles/indexes.
    let ctx = Context::default();
    let old = vec!["old-a".to_string(), "old-b".to_string()];
    let new = vec!["new-a".to_string(), "new-b".to_string(), "new-c".to_string()];
    let mut action = TabBarAction::None;
    let mut t = 0.0;

    toggle_list(&ctx, &old, 0, &mut t, &mut action);

    // The batch is replaced; the list stays open and rebinds to the new tabs.
    render(&ctx, &new, 0, plain(t), &mut action);
    t += DT;

    click(&ctx, &new, 0, row_label(1), &mut t, &mut action);
    assert!(
        matches!(action, TabBarAction::SwitchTab(1)),
        "row click after a batch swap must switch tabs, got {action:?}"
    );
}

#[test]
fn grey_others_row_closes_all_but_the_active_tab() {
    let ctx = Context::default();
    let titles = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let mut action = TabBarAction::None;
    let mut t = 0.0;

    toggle_list(&ctx, &titles, 1, &mut t, &mut action);
    // The grey row's "×" sits at the right edge of the first row.
    click(&ctx, &titles, 1, grey_x(), &mut t, &mut action);
    assert!(
        matches!(action, TabBarAction::CloseOtherTabs(1)),
        "grey row's x must close other tabs, got {action:?}"
    );
}

#[test]
fn a_normal_row_x_closes_just_that_tab_and_keeps_the_list_open() {
    let ctx = Context::default();
    let titles = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let mut action = TabBarAction::None;
    let mut t = 0.0;

    toggle_list(&ctx, &titles, 1, &mut t, &mut action);

    // Row "alpha" (index 0) — its "×" is at the row's right edge.
    click(&ctx, &titles, 1, row_x(0), &mut t, &mut action);
    assert!(
        matches!(action, TabBarAction::CloseTab(0)),
        "row x must close that tab only, got {action:?}"
    );

    // The list stays open afterwards: close "beta" as well.
    click(&ctx, &titles, 1, row_x(1), &mut t, &mut action);
    assert!(
        matches!(action, TabBarAction::CloseTab(1)),
        "second row x must close too, got {action:?}"
    );
}

#[test]
fn double_clicking_a_row_closes_that_tab() {
    let ctx = Context::default();
    let titles = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let mut action = TabBarAction::None;
    let mut t = 0.0;

    toggle_list(&ctx, &titles, 1, &mut t, &mut action);

    // Double-click row "alpha" (index 0, the active one): the single click is
    // a designed no-op, the double-click closes it.
    click(&ctx, &titles, 1, row_label(0), &mut t, &mut action);
    click(&ctx, &titles, 1, row_label(0), &mut t, &mut action);
    assert!(
        matches!(action, TabBarAction::CloseTab(0)),
        "double-click must close that tab, got {action:?}"
    );
}
