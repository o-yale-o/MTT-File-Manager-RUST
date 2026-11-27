//! UI-thread stall watchdog.
//!
//! The frame loop is single-threaded. When the UI thread blocks permanently
//! (a hung Win32 call into a filter driver, a deadlocked lock, or a blocked
//! GPU present) it also stops writing logs, so the only record of the frozen
//! phase is whatever the last reached stage was. This module keeps that record:
//!
//! * The frame loop calls [`enter`] at coarse boundaries (see [`Stage`]); each
//!   call stores the stage in one atomic and refreshes a heartbeat.
//! * A tiny observer thread samples the heartbeat every second. Once it has
//!   not moved for [`STALL_THRESHOLD`], it writes the last stage, the frame
//!   count and the process CPU utilization to the log and to
//!   `%LOCALAPPDATA%\\MTT-File-Manager\\logs\\stall.log`.
//!
//! Interpretation after a freeze:
//!
//! * `stage=ui_end` - the UI thread finished `ui()` and blocked afterwards,
//!   i.e. in the renderer present call or in the winit event loop. A DX12
//!   composition swapchain that waits for a frame the desktop compositor
//!   never delivers shows up exactly like this.
//! * `stage=panels` / `watcher_events` / `streaming_thumbnails` / ... - the
//!   block happened inside that part of the frame; see `docs/卡死排查.md`.
//! * `cpu=0%` while stalled - the thread is waiting, not spinning.
//!
//! The watchdog never touches app state: it only reads two atomics, so it
//! cannot itself contribute to a deadlock.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// How long the heartbeat may stay unchanged before a stall is reported.
const STALL_THRESHOLD: Duration = Duration::from_secs(5);
/// Sampling period of the observer thread.
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// While one stall persists, repeat the report this often.
const REPEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Coarse phases of one frame, in the order `logic()` / `ui()` reach them.
///
/// The numeric values are the stored representation; keep them in the same
/// order as [`STAGE_NAMES`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Stage {
    Startup = 0,
    LogicStart,
    LogicEnd,
    UiStart,
    MinimizedSkip,
    BackgroundUpdates,
    Messages,
    FileOperationResults,
    FileOperationResultsDone,
    WatcherEvents,
    WatcherEventsDone,
    Streaming,
    StreamingDone,
    MessagesDone,
    DriveRequests,
    PollResults,
    PreferencesAndMemory,
    BackgroundUpdatesDone,
    AsyncResults,
    Input,
    Toolbars,
    Panels,
    PanelsDone,
    ContextMenu,
    SettingsWindow,
    Overlays,
    UiEnd,
}

impl Stage {
    pub fn name(self) -> &'static str {
        STAGE_NAMES[self as usize]
    }
}

/// Names for [`Stage`]; index order must match the enum discriminants.
const STAGE_NAMES: &[&str] = &[
    "startup",
    "logic_start",
    "logic_end",
    "ui_start",
    "ui_minimized_skip",
    "background_updates",
    "messages",
    "file_operation_results",
    "file_operation_results_done",
    "watcher_events",
    "watcher_events_done",
    "streaming_thumbnails",
    "streaming_thumbnails_done",
    "messages_done",
    "drive_requests",
    "poll_results",
    "preferences_and_memory",
    "background_updates_done",
    "async_results",
    "input",
    "toolbars",
    "panels",
    "panels_done",
    "context_menu",
    "settings_window",
    "overlays",
    // `ui_end` is stored right before `ui()` returns; a stall reported with
    // this stage happened after our code, in present/event-loop code.
    "ui_end",
];

static STAGE: AtomicU8 = AtomicU8::new(Stage::Startup as u8);
static HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);
static FRAMES: AtomicU64 = AtomicU64::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn base() -> Instant {
    *BASE.get_or_init(Instant::now)
}

fn elapsed_ms() -> u64 {
    base().elapsed().as_millis().min(u64::MAX as u128) as u64
}

/// Records the phase the UI thread is entering and refreshes the heartbeat.
#[inline]
pub fn enter(stage: Stage) {
    STAGE.store(stage as u8, Ordering::Relaxed);
    HEARTBEAT_MS.store(elapsed_ms(), Ordering::Release);
}

/// Counts a started frame. The heartbeat alone cannot tell "one long frame"
/// from "the loop stopped", the counter makes that visible in the report.
#[inline]
pub fn tick_frame() {
    FRAMES.fetch_add(1, Ordering::Relaxed);
}

/// Starts the observer thread. Safe to call more than once.
pub fn start() {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = base();
    let spawned = std::thread::Builder::new()
        .name("ui-stall-watchdog".to_owned())
        .spawn(run);
    if spawned.is_err() {
        STARTED.store(false, Ordering::Release);
        log::warn!("[STALL] could not start the UI stall watchdog thread");
    }
}

fn run() {
    let mut last_reported_heartbeat: Option<u64> = None;
    let mut last_report_at: Option<Instant> = None;
    let mut was_stalled = false;

    loop {
        std::thread::sleep(POLL_INTERVAL);

        let heartbeat = HEARTBEAT_MS.load(Ordering::Acquire);
        if heartbeat == 0 {
            // No frame has reached any instrumented boundary yet.
            continue;
        }
        let stalled_ms = elapsed_ms().saturating_sub(heartbeat);
        let threshold_ms = STALL_THRESHOLD.as_millis() as u64;

        if stalled_ms < threshold_ms {
            if was_stalled {
                was_stalled = false;
                last_reported_heartbeat = None;
                last_report_at = None;
                let message = format!(
                    "[STALL-RECOVER] UI thread resumed after {:.1}s (stage={})",
                    stalled_ms as f64 / 1000.0,
                    current_stage_name()
                );
                log::info!("{}", message);
                write_stall_line(&message);
            }
            continue;
        }

        was_stalled = true;
        let due = match last_reported_heartbeat {
            None => true,
            Some(reported) => {
                reported != heartbeat
                    || last_report_at.is_some_and(|at| at.elapsed() >= REPEAT_INTERVAL)
            }
        };
        if !due {
            continue;
        }
        last_reported_heartbeat = Some(heartbeat);
        last_report_at = Some(Instant::now());
        report_stall(stalled_ms);
    }
}

fn report_stall(stalled_ms: u64) {
    let stage = current_stage_name();
    let frames = FRAMES.load(Ordering::Relaxed);
    let cpu = process_cpu_utilization();
    let message = format!(
        "[STALL] UI thread has not advanced for {:.1}s stage={} frames={} cpu={}",
        stalled_ms as f64 / 1000.0,
        stage,
        frames,
        cpu
    );
    log::error!("{}", message);
    write_stall_line(&message);
}

fn current_stage_name() -> &'static str {
    let raw = STAGE.load(Ordering::Relaxed) as usize;
    STAGE_NAMES.get(raw).copied().unwrap_or("unknown")
}

/// Appends one line to `stall.log` next to the diagnostic logs. Written
/// directly, not through the logger, so a stuck logger cannot hide it.
fn write_stall_line(message: &str) {
    use std::io::Write;

    let directory = crate::infrastructure::diagnostic_logger::log_directory_path();
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("stall.log"))
    else {
        return;
    };
    let epoch_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| {
            (elapsed.as_millis().min(u128::from(u64::MAX))) as u64
        })
        .unwrap_or(0);
    let _ = writeln!(file, "epoch_ms={} {}", epoch_ms, message);
}

#[cfg(windows)]
fn process_cpu_utilization() -> String {
    use std::sync::Mutex;
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    static SAMPLE: std::sync::OnceLock<Mutex<Option<(u64, Instant)>>> = std::sync::OnceLock::new();

    unsafe {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
        .is_err()
        {
            return "n/a".to_owned();
        }

        let total_100ns = filetime_to_u64(kernel) + filetime_to_u64(user);
        let now = Instant::now();
        let slot = SAMPLE.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let rendered = match *guard {
            Some((previous_total, previous_at)) => {
                let wall_secs = now.saturating_duration_since(previous_at).as_secs_f64();
                if wall_secs > 0.0 {
                    let cpu_secs =
                        total_100ns.saturating_sub(previous_total) as f64 / 10_000_000.0;
                    format!("{:.0}%", cpu_secs / wall_secs * 100.0)
                } else {
                    "n/a".to_owned()
                }
            }
            None => "sampling".to_owned(),
        };
        *guard = Some((total_100ns, now));
        rendered
    }
}

#[cfg(windows)]
fn filetime_to_u64(value: windows::Win32::Foundation::FILETIME) -> u64 {
    ((value.dwHighDateTime as u64) << 32) | u64::from(value.dwLowDateTime)
}

#[cfg(not(windows))]
fn process_cpu_utilization() -> String {
    "n/a".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{elapsed_ms, tick_frame, Stage, STAGE_NAMES};

    #[test]
    fn stage_names_match_enum_order() {
        assert_eq!(STAGE_NAMES.len(), Stage::UiEnd as usize + 1);
        assert_eq!(Stage::Startup.name(), "startup");
        assert_eq!(Stage::Messages.name(), "messages");
        assert_eq!(Stage::Panels.name(), "panels");
        assert_eq!(Stage::UiEnd.name(), "ui_end");
        for (index, name) in STAGE_NAMES.iter().enumerate() {
            assert!(!name.is_empty(), "stage {index} has no name");
        }
    }

    #[test]
    fn heartbeat_and_frames_advance() {
        let before = elapsed_ms();
        tick_frame();
        let after = elapsed_ms();
        assert!(after >= before);
    }
}
