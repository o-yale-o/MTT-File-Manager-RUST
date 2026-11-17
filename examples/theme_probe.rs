//! Probe: read Windows 11 system palette (ItemsView theme colors, DWM
//! colorization, accent registry) and print them, to verify which API values
//! match Explorer's real look before wiring them into the theme engine.
#[cfg(windows)]
fn main() {
    use windows::core::{w, BOOL};
    use windows::Win32::Foundation::{COLORREF, HWND, LRESULT, LPARAM, WPARAM};
    use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Controls::{
        GetThemeColor, OpenThemeData, SetWindowTheme, THEME_PROPERTY_SYMBOL_ID,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, CW_USEDEFAULT, HWND_MESSAGE,
        WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    };

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    unsafe {
        // 1) Message-only window so theme data has a context.
        let class_name = w!("MTTThemeProbeWnd");
        let hinstance = GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            lpszClassName: class_name,
            hInstance: hinstance.into(),
            ..Default::default()
        };
        let atom = RegisterClassW(&wc);
        assert_ne!(atom, 0, "RegisterClassW failed");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("probe"),
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
        .unwrap();
        assert!(!hwnd.is_invalid(), "CreateWindowExW failed");

        // 2) Default theme contexts.
        for (label, cls) in [
            ("ItemsView (default)", w!("ItemsView")),
            ("DarkMode_Explorer", w!("DarkMode_Explorer")),
            ("DarkMode_ItemsView", w!("DarkMode_ItemsView")),
        ] {
            let theme = OpenThemeData(Some(hwnd), cls);
            if theme.is_invalid() {
                println!("{label}: OpenThemeData failed");
                continue;
            }
            println!("== {label} (part 0, state 0) ==");
            for prop in [3801u32, 3802, 3803, 3808, 3810, 3817, 3818, 3819] {
                match GetThemeColor(theme, 0, 0, THEME_PROPERTY_SYMBOL_ID(prop)) {
                    Ok(colorref) => println!("  prop {prop}: {}", fmt(colorref)),
                    Err(e) => println!("  prop {prop}: not set ({e})"),
                }
            }
            drop(theme);
        }

        // 3) Dark-mode window context.
        for theme_class in [w!("DarkMode_Explorer"), w!("ItemsView")] {
            let _ = SetWindowTheme(hwnd, theme_class, None);
            let theme = OpenThemeData(Some(hwnd), w!("ItemsView"));
            if !theme.is_invalid() {
                println!(
                    "== ItemsView after SetWindowTheme({}) ==",
                    theme_class.to_string().unwrap_or_default()
                );
                for prop in [3801u32, 3802, 3803, 3808, 3810, 3817, 3818, 3819] {
                    match GetThemeColor(theme, 0, 0, THEME_PROPERTY_SYMBOL_ID(prop)) {
                        Ok(colorref) => println!("  prop {prop}: {}", fmt(colorref)),
                        Err(e) => println!("  prop {prop}: not set ({e})"),
                    }
                }
                drop(theme);
            }
        }

        println!("\n== DWM colorization ==");
        let mut color = 0u32;
        let mut blend = BOOL(0);
        let _ = DwmGetColorizationColor(&mut color, &mut blend);
        println!(
            "DwmGetColorizationColor: #{:02X}{:02X}{:02X}",
            (color >> 16) & 0xFF,
            (color >> 8) & 0xFF,
            color & 0xFF
        );

        println!("\n== Registry ==");
        for name in ["AppsUseLightTheme", "SystemUsesLightTheme"] {
            let mut value = 0u32;
            let mut size = 4u32;
            let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
            let hr = RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                windows::core::PCWSTR(wide.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut u32 as _),
                Some(&mut size),
            );
            if hr.is_ok() {
                println!("{name} = {value}");
            } else {
                println!("{name}: read failed");
            }
        }
        let mut value = 0u32;
        let mut size = 4u32;
        let hr = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent"),
            w!("AccentColorMenu"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as _),
            Some(&mut size),
        );
        if hr.is_ok() {
            println!("AccentColorMenu: {}", fmt(COLORREF(value)));
        } else {
            println!("AccentColorMenu: read failed");
        }
    }
}

fn fmt(colorref: windows::Win32::Foundation::COLORREF) -> String {
    let v = colorref.0;
    format!(
        "#{:02X}{:02X}{:02X}  (r={} g={} b={})",
        v & 0xFF,
        (v >> 8) & 0xFF,
        (v >> 16) & 0xFF,
        v & 0xFF,
        (v >> 8) & 0xFF,
        (v >> 16) & 0xFF
    )
}

#[cfg(not(windows))]
fn main() {}
