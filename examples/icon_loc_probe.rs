//! Debug: SHGFI_ICONLOCATION for a dummy "file.md" + SHDefExtractIconW 256.
#[cfg(windows)]
fn main() {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::UI::Shell::{
        SHDefExtractIconW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICONLOCATION,
        SHGFI_USEFILEATTRIBUTES,
    };
    use windows::Win32::UI::WindowsAndMessaging::HICON;

    unsafe {
        let wide: Vec<u16> = "file.md"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut shfi = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICONLOCATION | SHGFI_USEFILEATTRIBUTES,
        );
        println!("SHGetFileInfoW result: {result}");
        let path: String = shfi
            .szDisplayName
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| char::from_u32(c as u32).unwrap_or('\0'))
            .collect();
        println!("icon location: {:?}, index: {}", path, shfi.iIcon);

        // Shell quirk: for generic icons it often fills only the index; the
        // resource lives in imageres.dll.
        let path = if path.is_empty() {
            "C:\\Windows\\System32\\imageres.dll".to_string()
        } else {
            path
        };

        if !path.is_empty() {
            let path_wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let mut hicon = HICON::default();
            let hr = SHDefExtractIconW(
                PCWSTR(path_wide.as_ptr()),
                shfi.iIcon,
                0,
                Some(&mut hicon),
                None,
                256,
            );
            println!("SHDefExtractIconW: {hr:?}, invalid={}", hicon.is_invalid());
            if hr.is_ok() && !hicon.is_invalid() {
                let rgba =
                    mtt_file_manager::infrastructure::windows::bitmap_conversion::hicon_to_rgba(
                        hicon,
                    );
                match rgba {
                    Ok((data, w, h)) => {
                        println!("converted {w}x{h}");
                        let mut img = image::RgbaImage::new(w, h);
                        for (x, y, p) in img.enumerate_pixels_mut() {
                            let (x, y) = (x as usize, y as usize);
                            let i = (y * w as usize + x) * 4;
                            *p = image::Rgba([
                                data[i],
                                data[i + 1],
                                data[i + 2],
                                data[i + 3],
                            ]);
                        }
                        img.save("icon_loc_256.png").unwrap();
                        println!("saved icon_loc_256.png");
                    }
                    Err(e) => println!("convert failed: {e:?}"),
                }
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(hicon);
            }
        }
    }
}

#[cfg(not(windows))]
fn main() {}
