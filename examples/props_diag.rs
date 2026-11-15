//! Standalone repro for the multi-select properties dialog.
//! Usage: cargo run --example props_diag
//! Opens ONE combined properties window for three temp files, then exits.

use std::path::PathBuf;

fn main() {
    unsafe {
        let _hr = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let hwnd = if let Some(arg) = std::env::args().nth(1) {
        let value: isize = arg.parse().expect("hwnd must be an integer");
        windows::Win32::Foundation::HWND(value as *mut _)
    } else {
        windows::Win32::Foundation::HWND::default()
    };
    println!("hwnd = {hwnd:?}");

    let dir = std::env::temp_dir().join("mtt_props_diag");
    std::fs::create_dir_all(&dir).expect("create dir");
    let mut paths: Vec<PathBuf> = Vec::new();
    for i in 1..=3 {
        let path = dir.join(format!("diag_props_{i}.txt"));
        std::fs::write(&path, format!("content {i}")).expect("write file");
        paths.push(path);
    }
    println!("selection: {paths:?}");

    let result = mtt_file_manager::infrastructure::windows::native_menu::
        show_properties_for_selection(hwnd, &paths);
    println!("show_properties_for_selection result: {result:?}");

    println!("(sleeping 10s so you can see the dialog...)");
    std::thread::sleep(std::time::Duration::from_secs(10));
}
