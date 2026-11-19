//! Interaction tests for the window tab-list overlay drawn inside the
//! window control buttons.
//!
//! egui resolves clicks against the *previous* frame's widget rects, so every
//! `click` helper here first moves the pointer (hover frame), then presses,
//! then releases — matching the rhythm of a real user.

use super::window_controls::render_window_controls;
use super::TabBarAction;
use eframe::egui::{self, pos2, vec2, Context, Event, Modifiers, PointerButton, RawInput};

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0))
}

fn plain() -> RawInput {
    RawInput {
        screen_rect: Some(screen()),
        ..Default::default()
    }
}

fn input(pos: egui::Pos2, pressed: bool) -> RawInput {
    RawInput {
        screen_rect: Some(screen()),
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

fn hover(pos: egui::Pos2) -> RawInput {
    RawInput {
        screen_rect: Some(screen()),
        events: vec![Event::PointerMoved(pos)],
        ..Default::default()
    }
}

fn click(ctx: &Context, titles: &[String], active: usize, pos: egui::Pos2, action: &mut TabBarAction) {
    render(ctx, titles, active, hover(pos), action);
    render(ctx, titles, active, input(pos, true), action);
    render(ctx, titles, active, input(pos, false), action);
    render(ctx, titles, active, plain(), action);
}

fn render(
    ctx: &Context,
    titles: &[String],
    active: usize,
    input: RawInput,
    action: &mut TabBarAction,
) {
    let _ = ctx.run_ui(input, |ui| {
        render_window_controls(ui, 46.0, 36.0, 10.0, titles, active, action);
    });
}

fn toggle_list(ctx: &Context, titles: &[String], active: usize, action: &mut TabBarAction) {
    render(ctx, titles, active, plain(), action);
    click(ctx, titles, active, pos2(18.0, 28.0), action);
}

// Row geometry (widths in the 1000×800 test window): the list panel spans
// x 680..1000 with 4px inner padding; each row is a 22px label + "×" button
// with 2px spacing. Grey "其它" first, then one row per tab.
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

    toggle_list(&ctx, &titles, 0, &mut action);

    click(&ctx, &titles, 0, row_label(1), &mut action);
    assert!(
        matches!(action, TabBarAction::SwitchTab(1)),
        "row click must switch tabs, got {action:?}"
    );

    click(&ctx, &titles, 0, row_label(2), &mut action);
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

    toggle_list(&ctx, &titles, 0, &mut action);
    click(&ctx, &titles, 0, row_label(0), &mut action);
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

    toggle_list(&ctx, &old, 0, &mut action);

    // The batch is replaced; the list stays open and rebinds to the new
    // tabs without any extra click.
    render(&ctx, &new, 0, plain(), &mut action);

    click(&ctx, &new, 0, row_label(1), &mut action);
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

    toggle_list(&ctx, &titles, 1, &mut action);
    // The grey row's "×" sits at the right edge of the first row (y=63).
    click(&ctx, &titles, 1, grey_x(), &mut action);
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

    toggle_list(&ctx, &titles, 1, &mut action);

    // Row "alpha" (index 0) — its "×" is at the row's right edge.
    click(&ctx, &titles, 1, row_x(0), &mut action);
    assert!(
        matches!(action, TabBarAction::CloseTab(0)),
        "row x must close that tab only, got {action:?}"
    );

    // The list stays open afterwards: close "beta" as well.
    click(&ctx, &titles, 1, row_x(1), &mut action);
    assert!(
        matches!(action, TabBarAction::CloseTab(1)),
        "second row x must close too, got {action:?}"
    );
}
