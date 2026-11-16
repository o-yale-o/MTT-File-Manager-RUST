use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{RedrawWindow, RDW_INTERNALPAINT};

static NEXT_SIZE_MOVE_IS_CAPTION: AtomicBool = AtomicBool::new(false);
static PAINT_SUPPRESSED: AtomicBool = AtomicBool::new(false);

pub(super) fn note_non_client_press(is_caption: bool) {
    NEXT_SIZE_MOVE_IS_CAPTION.store(is_caption, Ordering::Release);
}

pub(super) fn note_system_command(wparam: usize) {
    if let Some(is_caption_move) = caption_move_from_system_command(wparam) {
        NEXT_SIZE_MOVE_IS_CAPTION.store(is_caption_move, Ordering::Release);
    }
}

pub(super) fn begin_caption_move() {
    if NEXT_SIZE_MOVE_IS_CAPTION.swap(false, Ordering::AcqRel) {
        PAINT_SUPPRESSED.store(true, Ordering::Release);
    }
}

pub(super) fn end_caption_move(hwnd: HWND) {
    NEXT_SIZE_MOVE_IS_CAPTION.store(false, Ordering::Release);
    restore_redraw(hwnd, true);
}

pub(super) fn clear_pending_move() {
    NEXT_SIZE_MOVE_IS_CAPTION.store(false, Ordering::Release);
}

pub(super) fn is_active() -> bool {
    PAINT_SUPPRESSED.load(Ordering::Acquire)
}

pub(super) fn reset_for_remove(hwnd: HWND) {
    NEXT_SIZE_MOVE_IS_CAPTION.store(false, Ordering::Release);
    restore_redraw(hwnd, false);
}

pub(super) fn reset_for_destroy() {
    NEXT_SIZE_MOVE_IS_CAPTION.store(false, Ordering::Release);
    PAINT_SUPPRESSED.store(false, Ordering::Release);
}

fn restore_redraw(hwnd: HWND, invalidate: bool) {
    if !PAINT_SUPPRESSED.swap(false, Ordering::AcqRel) {
        return;
    }

    if invalidate {
        unsafe {
            let _ = RedrawWindow(Some(hwnd), None, None, RDW_INTERNALPAINT);
        }
    }
}

fn caption_move_from_system_command(wparam: usize) -> Option<bool> {
    const SC_COMMAND_MASK: usize = 0xFFF0;
    const SC_SIZE_CMD: usize = 0xF000;
    const SC_MOVE_CMD: usize = 0xF010;
    match wparam & SC_COMMAND_MASK {
        SC_MOVE_CMD => Some(true),
        SC_SIZE_CMD => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_commands_distinguish_caption_move_from_resize() {
        assert_eq!(caption_move_from_system_command(0xF010), Some(true));
        assert_eq!(caption_move_from_system_command(0xF000), Some(false));
    }

    #[test]
    fn system_command_classification_ignores_low_bits() {
        assert_eq!(caption_move_from_system_command(0xF010 | 0x0002), Some(true));
        assert_eq!(caption_move_from_system_command(0xF020), None);
    }
}
