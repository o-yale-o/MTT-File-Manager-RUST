//! Reads the Windows "app mode" (Settings > Personalization > Colors >
//! "Choose your default app mode"), which is what the `System` theme follows.
//!
//! The value lives in `HKCU\...\Themes\Personalize\AppsUseLightTheme`
//! (DWORD: `1` = light apps, `0` = dark apps). It is deliberately not cached so
//! `System` can react to OS changes at runtime.
//!
//! This module also opts the process into dark-mode support
//! (`init_windows_dark_mode`) so that apps launched by the file manager
//! inherit the correct app mode on Windows 10.

use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::FARPROC;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_DWORD,
};

const PERSONALIZE_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";
const UXTHEME_DLL: &[u8] = b"uxtheme.dll\0";

/// Reads whether Windows is configured to use light mode for apps.
///
/// Returns `None` when the value is missing or unreadable (e.g. portable or
/// non-Windows environments).
pub fn read_windows_apps_use_light_theme() -> Option<bool> {
    let key_wide: Vec<u16> = PERSONALIZE_KEY
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let value_wide: Vec<u16> = APPS_USE_LIGHT_THEME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_wide.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return None;
        }

        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let result = RegGetValueW(
            hkey,
            PCWSTR::null(),
            PCWSTR(value_wide.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);

        result.is_ok().then_some(value != 0)
    }
}

/// Whether the OS app mode is currently dark. Unknown values fall back to
/// light. This is the single source of truth for the `System` theme mode.
pub fn windows_app_is_dark() -> bool {
    read_windows_apps_use_light_theme() == Some(false)
}

/// `uxtheme.dll` ordinal 135: the undocumented dark-mode opt-in entry point
/// (`SetPreferredAppMode` / `AllowDarkModeForApp`, depending on the OS build).
/// Introduced in Windows 10 1809; accepts `1` as "allow dark" on every build
/// where it exists.
const UXTHEME_PREFERRED_APP_MODE_ORDINAL: usize = 135;
const PREFERRED_APP_MODE_ALLOW_DARK: i32 = 1;

/// Opts this process into dark mode support. Must run before any window is
/// created.
///
/// The preferred app mode is inherited by child processes on Windows 10, so
/// without this opt-in the apps launched by the file manager resolve
/// `ShouldAppsUseDarkMode()` (winit/egui's system-theme probe) as light even
/// when the user's app mode is dark. No-op where the ordinal is unavailable
/// (Windows 10 versions before 1809).
pub fn init_windows_dark_mode() {
    unsafe {
        let Ok(module) = LoadLibraryA(PCSTR(UXTHEME_DLL.as_ptr())) else {
            return;
        };
        let entry = GetProcAddress(
            module,
            PCSTR(UXTHEME_PREFERRED_APP_MODE_ORDINAL as *const u8),
        );
        if entry.is_none() {
            return;
        }
        let set_preferred_app_mode: unsafe extern "system" fn(i32) =
            std::mem::transmute::<FARPROC, unsafe extern "system" fn(i32)>(entry);
        set_preferred_app_mode(PREFERRED_APP_MODE_ALLOW_DARK);
    }
}

// === SYSTEM PALETTE (Explorer ItemsView theme colors) ===

use once_cell::sync::Lazy;
use windows::core::w;
use windows::Win32::UI::Controls::{
    CloseThemeData, GetThemeColor, OpenThemeData, SetWindowTheme, THEME_PROPERTY_SYMBOL_ID,
};

const TMT_EDGEFILLCOLOR: u32 = 3802; // ItemsView list background
const TMT_TEXTCOLOR: u32 = 3803; // ItemsView item text

/// Colors sampled from the Explorer "ItemsView" theme class, which is the
/// palette the Windows 11 file list actually paints with.
#[derive(Clone, Copy, Debug)]
pub struct SystemItemsColors {
    pub background: [u8; 3],
    pub text: [u8; 3],
}

/// Message-only window reused for all theme queries: `OpenThemeData` resolves
/// against a window's theme context, and the dark variant only exists after
/// `SetWindowTheme(hwnd, "DarkMode_Explorer")` (verified via
/// `examples/theme_probe.rs`: ItemsView 3802 → `#191919`, 3803 → `#FFFFFF`).
static PROBE_WINDOW: Lazy<Option<isize>> = Lazy::new(|| unsafe {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, CW_USEDEFAULT, HWND_MESSAGE,
        WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    };

    unsafe extern "system" fn wnd_proc(
        hwnd: windows::Win32::Foundation::HWND,
        msg: u32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    let Ok(hinstance) = GetModuleHandleW(None) else {
        return None;
    };
    let class_name = w!("MTTSystemThemeProbeWnd");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        lpszClassName: class_name,
        hInstance: hinstance.into(),
        ..Default::default()
    };
    if RegisterClassW(&wc) == 0 {
        return None;
    }
    CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class_name,
        w!("MTT theme probe"),
        WINDOW_STYLE::default(),
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        Some(HWND_MESSAGE),
        None,
        Some(hinstance.into()),
        None,
    )
    .ok()
    .map(|hwnd| hwnd.0 as isize)
});

/// Samples the background/text colors Windows itself uses for file lists in
/// the given mode. Returns `None` when uxtheme cannot provide them (very old
/// Windows, theme service unavailable), so callers keep their fallbacks.
pub fn read_system_items_colors(dark: bool) -> Option<SystemItemsColors> {
    // HWND stored as isize so the lazy static stays Send/Sync.
    let hwnd = windows::Win32::Foundation::HWND(
        (*PROBE_WINDOW.as_ref()?) as *mut core::ffi::c_void,
    );
    unsafe {
        // `DarkMode_Explorer` switches the window's theme context to the dark
        // variant; opening plain `ItemsView` from it then yields the dark
        // palette. Without this the light values are always returned.
        let context = if dark {
            w!("DarkMode_Explorer")
        } else {
            w!("ItemsView")
        };
        if SetWindowTheme(hwnd, context, None).is_err() {
            return None;
        }
        let theme = OpenThemeData(Some(hwnd), w!("ItemsView"));
        if theme.is_invalid() {
            return None;
        }
        let background = GetThemeColor(theme, 0, 0, THEME_PROPERTY_SYMBOL_ID(TMT_EDGEFILLCOLOR));
        let text = GetThemeColor(theme, 0, 0, THEME_PROPERTY_SYMBOL_ID(TMT_TEXTCOLOR));
        let _ = CloseThemeData(theme);

        let to_rgb = |colorref: windows::Win32::Foundation::COLORREF| -> [u8; 3] {
            [(colorref.0 & 0xFF) as u8, ((colorref.0 >> 8) & 0xFF) as u8, ((colorref.0 >> 16) & 0xFF) as u8]
        };
        let (Ok(background), Ok(text)) = (background, text) else {
            return None;
        };
        let (background, text) = (to_rgb(background), to_rgb(text));
        // Sanity check: a "palette" where text equals background is unusable.
        if background == text {
            return None;
        }
        Some(SystemItemsColors { background, text })
    }
}

/// The user's accent color as configured in Settings > Personalization >
/// Colors (`AccentColorMenu`, COLORREF in
/// `HKCU\...\Explorer\Accent`). Returns `None` when unset.
pub fn read_system_accent_color() -> Option<[u8; 3]> {
    const ACCENT_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent";
    const ACCENT_COLOR_MENU: &str = "AccentColorMenu";

    let key_wide: Vec<u16> = ACCENT_KEY.encode_utf16().chain(std::iter::once(0)).collect();
    let value_wide: Vec<u16> = ACCENT_COLOR_MENU
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        if RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key_wide.as_ptr()),
            PCWSTR(value_wide.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
        .is_err()
        {
            return None;
        }
        Some([(value & 0xFF) as u8, ((value >> 8) & 0xFF) as u8, ((value >> 16) & 0xFF) as u8])
    }
}
