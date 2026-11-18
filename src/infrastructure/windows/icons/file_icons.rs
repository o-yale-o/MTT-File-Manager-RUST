use crate::domain::file_entry::IconSize;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::Gdi::*, Win32::Storage::FileSystem::*,
    Win32::UI::Shell::*, Win32::UI::WindowsAndMessaging::*,
};

/// Extracts the native Windows icon for a file extension.
///
/// Uses FILE_ATTRIBUTE_NORMAL + SHGFI_USEFILEATTRIBUTES to get the default icon for the type.
///
/// # Safety
/// Uses SHGetFileInfoW with dummy path. HICON is always freed with DestroyIcon.
pub fn extract_file_icon(
    extension: &str, // ".pdf", ".exe", etc.
    size: IconSize,
) -> std::result::Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    unsafe {
        // Create dummy path with extension (e.g., "dummy.pdf")
        let dummy_path = format!("dummy{}", extension);
        let path_wide: Vec<u16> = dummy_path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // For Jumbo icons, use IShellItemImageFactory even with dummy path (if possible)
        // Note: SHCreateItemFromParsingName with dummy path rarely works for Jumbo.
        // We stick to SHGetFileInfo for dummy icons, but ensure we use Large if Jumbo requested.

        let mut shfi = SHFILEINFOW::default();

        // CORRECT FLAGS: USEFILEATTRIBUTES allows dummy path
        let flags = SHGFI_ICON
            | SHGFI_USEFILEATTRIBUTES
            | match size {
                IconSize::Small => SHGFI_SMALLICON,
                IconSize::Large | IconSize::Jumbo => SHGFI_LARGEICON,
            };

        let result = SHGetFileInfoW(
            PCWSTR(path_wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );

        if result == 0 || shfi.hIcon.is_invalid() {
            return Err("Failed to get file icon".into());
        }

        let hicon = shfi.hIcon;
        let conversion_result =
            crate::infrastructure::windows::bitmap_conversion::hicon_to_rgba(hicon);

        // SAFETY: Always free HICON
        let _ = DestroyIcon(hicon);

        conversion_result
    }
}

/// Extracts folder icon using DUMMY path.
///
/// Uses FILE_ATTRIBUTE_DIRECTORY + SHGFI_USEFILEATTRIBUTES to get the standard folder icon.
pub fn extract_folder_icon(
    size: IconSize,
) -> std::result::Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    unsafe {
        if matches!(size, IconSize::Jumbo) {
            // High quality folder icon using known folder ID if possible,
            // or just a common path that is guaranteed to be a directory.
            let windows_dir =
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
            let path_wide: Vec<u16> = windows_dir
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            if let Ok(shell_item) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(path_wide.as_ptr()), None)
            {
                if let Ok(image_factory) = shell_item.cast::<IShellItemImageFactory>() {
                    let size_factory = SIZE { cx: 256, cy: 256 };
                    if let Ok(hbitmap) = image_factory.GetImage(size_factory, SIIGBF_ICONONLY) {
                        let res =
                            crate::infrastructure::windows::bitmap_conversion::hbitmap_to_rgba(
                                hbitmap,
                            );
                        let _ = DeleteObject(hbitmap.into());
                        return res;
                    }
                }
            }
        }

        let dummy_path = "dummy_folder";
        let path_wide: Vec<u16> = dummy_path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let mut shfi = SHFILEINFOW::default();

        let flags = SHGFI_ICON
            | SHGFI_USEFILEATTRIBUTES
            | match size {
                IconSize::Small => SHGFI_SMALLICON,
                IconSize::Large | IconSize::Jumbo => SHGFI_LARGEICON,
            };

        let result = SHGetFileInfoW(
            PCWSTR(path_wide.as_ptr()),
            FILE_ATTRIBUTE_DIRECTORY,
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );

        if result == 0 || shfi.hIcon.is_invalid() {
            return Err("Failed to get folder icon".into());
        }

        let hicon = shfi.hIcon;
        let conversion_result =
            crate::infrastructure::windows::bitmap_conversion::hicon_to_rgba(hicon);

        let _ = DestroyIcon(hicon);

        conversion_result
    }
}

/// Extracts icon from a REAL file using full path.
/// Used for executables (.exe, .lnk, .ico) that have unique icons.
pub fn extract_file_icon_by_path(
    path: &Path,
    size: IconSize,
) -> std::result::Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    unsafe {
        let path_wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // For Jumbo icons, use IShellItemImageFactory (higher quality)
        if matches!(size, IconSize::Jumbo) {
            if let Ok(shell_item) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(path_wide.as_ptr()), None)
            {
                if let Ok(image_factory) = shell_item.cast::<IShellItemImageFactory>() {
                    let size_factory = SIZE { cx: 256, cy: 256 };
                    // SIIGBF_ICONONLY to ensure we get the icon and not a thumbnail if it were a file
                    if let Ok(hbitmap) = image_factory.GetImage(size_factory, SIIGBF_ICONONLY) {
                        let res =
                            crate::infrastructure::windows::bitmap_conversion::hbitmap_to_rgba(
                                hbitmap,
                            );
                        let _ = DeleteObject(hbitmap.into());
                        return res;
                    }
                }
            }
        }

        let mut shfi = SHFILEINFOW::default();

        // WITHOUT USEFILEATTRIBUTES - uses real file
        let flags = SHGFI_ICON
            | match size {
                IconSize::Small => SHGFI_SMALLICON,
                IconSize::Large | IconSize::Jumbo => SHGFI_LARGEICON,
            };

        let result = SHGetFileInfoW(
            PCWSTR(path_wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );

        if result == 0 || shfi.hIcon.is_invalid() {
            return Err("Failed to get file icon".into());
        }

        let hicon = shfi.hIcon;
        let conversion_result =
            crate::infrastructure::windows::bitmap_conversion::hicon_to_rgba(hicon);

        let _ = DestroyIcon(hicon);

        conversion_result
    }
}

pub fn extract_icon_resource(
    resource: &str,
    size: IconSize,
) -> std::result::Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    let Some((path, index)) = parse_icon_resource(resource) else {
        return Err("invalid icon resource".into());
    };

    unsafe {
        let path_wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut h_large = HICON::default();
        let mut h_small = HICON::default();
        let icon_size = match size {
            IconSize::Small => 16u32,
            IconSize::Large => 32u32,
            IconSize::Jumbo => 256u32,
        };
        let size_param = icon_size | (icon_size << 16);

        SHDefExtractIconW(
            PCWSTR(path_wide.as_ptr()),
            index,
            0,
            Some(&mut h_large),
            Some(&mut h_small),
            size_param,
        )
        .ok()?;

        let hicon = if matches!(size, IconSize::Small) && !h_small.is_invalid() {
            h_small
        } else if !h_large.is_invalid() {
            h_large
        } else if !h_small.is_invalid() {
            h_small
        } else {
            return Err("failed to extract icon resource".into());
        };

        let conversion_result =
            crate::infrastructure::windows::bitmap_conversion::hicon_to_rgba(hicon);
        let _ = DestroyIcon(hicon);
        if h_small != hicon && !h_small.is_invalid() {
            let _ = DestroyIcon(h_small);
        }
        if h_large != hicon && !h_large.is_invalid() {
            let _ = DestroyIcon(h_large);
        }

        conversion_result
    }
}

fn parse_icon_resource(resource: &str) -> Option<(String, i32)> {
    let trimmed = resource.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }

    let (path, index) = if let Some(idx) = trimmed.rfind(',') {
        let path = trimmed[..idx].trim().trim_matches('"');
        let index = trimmed[idx + 1..].trim().parse::<i32>().unwrap_or(0);
        (path, index)
    } else {
        (trimmed, 0)
    };

    (!path.is_empty()).then(|| (path.to_string(), index))
}

/// Extracts REAL icon from a drive (C:\, D:\, etc.).
///
/// Uses real path (not dummy) and WITHOUT SHGFI_USEFILEATTRIBUTES.
/// This forces Windows to return the specific drive icon (HD, SSD, USB, etc.).
pub fn extract_drive_icon(
    drive_path: &str,
    size: IconSize,
) -> std::result::Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    unsafe {
        let path_wide: Vec<u16> = drive_path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // For Jumbo icons, try IShellItemImageFactory first (higher quality),
        // then fall back to SHGetFileInfoW if it fails (common for USB drives).
        if matches!(size, IconSize::Jumbo) {
            let jumbo_result =
                (|| -> std::result::Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
                    let shell_item: IShellItem =
                        SHCreateItemFromParsingName(PCWSTR(path_wide.as_ptr()), None)?;

                    let image_factory: IShellItemImageFactory = shell_item.cast()?;

                    let size_factory = SIZE { cx: 256, cy: 256 };

                    let hbitmap: HBITMAP = image_factory.GetImage(size_factory, SIIGBF_ICONONLY)?;

                    let result =
                        crate::infrastructure::windows::bitmap_conversion::hbitmap_to_rgba(hbitmap);
                    let _ = DeleteObject(hbitmap.into());
                    result
                })();

            if let Ok(result) = jumbo_result {
                return Ok(result);
            }
            // Jumbo failed - fall through to SHGetFileInfoW below
        }

        // For Small/Large, use legacy but fast SHGetFileInfo
        let mut shfi = SHFILEINFOW::default();

        let flags = SHGFI_ICON
            | match size {
                IconSize::Small => SHGFI_SMALLICON,
                IconSize::Large | IconSize::Jumbo => SHGFI_LARGEICON, // Fallback (Jumbo handled above)
            };

        let result = SHGetFileInfoW(
            PCWSTR(path_wide.as_ptr()),
            FILE_ATTRIBUTE_DIRECTORY,
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );

        if result == 0 || shfi.hIcon.is_invalid() {
            return Err("Failed to get drive icon".into());
        }

        let hicon = shfi.hIcon;
        let conversion_result =
            crate::infrastructure::windows::bitmap_conversion::hicon_to_rgba(hicon);
        let _ = DestroyIcon(hicon);

        conversion_result
    }
}

/// Returns the default Windows icon for a file type (by extension), without requiring the file to exist.
/// Initializes COM for proper Shell integration on secondary threads.
///
/// Resolves the (icon file, index) a ProgID's `DefaultIcon` points at for the
/// given extension. Mirrors how Explorer resolves extension icons:
/// `HKCR\<ext>` → default ProgID → `HKCR\<ProgID>\DefaultIcon`
/// (`"path",index` — quotes optional, index optional, negative = resource id).
fn registered_icon_location(extension: &str) -> Option<(String, i32)> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, RRF_RT_REG_SZ,
    };

    let ext = extension.trim_start_matches('.');
    if ext.is_empty() {
        return None;
    }

    let read_sz = |subkey: &str| -> Option<String> {
        let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
            unsafe {
                if RegGetValueW(
                    HKEY_CLASSES_ROOT,
                    PCWSTR(subkey_wide.as_ptr()),
                    PCWSTR::null(),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr() as *mut _),
                    Some(&mut size),
                )
                .is_err()
                {
                    return None;
                }
            }
        let len = (size as usize / 2).min(buf.len());
        Some(
            buf[..len]
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| char::from_u32(c as u32).unwrap_or('\0'))
                .collect(),
        )
    };
    let read_hkcu = |subkey: &str| -> Option<String> {
        let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
            unsafe {
                if RegGetValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(subkey_wide.as_ptr()),
                    PCWSTR::null(),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr() as *mut _),
                    Some(&mut size),
                )
                .is_err()
                {
                    return None;
                }
            }
        let len = (size as usize / 2).min(buf.len());
        Some(
            buf[..len]
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| char::from_u32(c as u32).unwrap_or('\0'))
                .collect(),
        )
    };

    // Per-user association (Explorer's UserChoice) wins, then machine-wide.
    let progid = read_hkcu(&format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.{ext}\\UserChoice"
    ))
    .or_else(|| read_sz(&format!(".{ext}")))?;
    let default_icon = read_sz(&format!("{progid}\\DefaultIcon"))?;
    let default_icon = default_icon.trim().trim_matches('"').to_string();

    // Split trailing ",<index>" (paths may contain colons/spaces; the comma
    // before the index is the last one that parses as an integer).
    let (path, index) = match default_icon.rfind(',') {
        Some(pos) => {
            let idx = default_icon[pos + 1..].trim();
            match idx.parse::<i32>() {
                Ok(i) => (default_icon[..pos].trim().to_string(), i),
                Err(_) => (default_icon.as_str().to_string(), 0),
            }
        }
        None => (default_icon.clone(), 0),
    };
    if path.is_empty() {
        return None;
    }
    // Expand REG_EXPAND_SZ-style environment references the shell would too.
    let path = std::env::var("SystemRoot")
        .map(|root| path.replacen("%SystemRoot%", &root, 1))
        .unwrap_or_else(|_| path);
    Some((path, index))
}
/// Resolves the extension icon like Explorer: dummy path +
/// SHGFI_USEFILEATTRIBUTES, upgrading Jumbo requests to the registered icon
/// file's 256px variant via SHDefExtractIconW.
pub fn get_file_type_icon(
    is_folder: bool,
    extension: &str,
    size: IconSize,
) -> std::result::Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        SHDefExtractIconW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_ICONLOCATION,
        SHGFI_LARGEICON, SHGFI_SMALLICON, SHGFI_USEFILEATTRIBUTES,
    };
    use windows::Win32::UI::WindowsAndMessaging::HICON;

    // RAII guard: ensures CoUninitialize is always called to prevent COM refcount leaks.
    // Previous code called CoInitialize without CoUninitialize, leaking kernel resources
    // (Non-Paged Pool, User handles) on every call — a major source of system-wide
    // unresponsiveness after prolonged use.
    struct ComGuard(bool);
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }

    unsafe {
        let _com = ComGuard(CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok());

        if matches!(size, IconSize::Jumbo) && is_folder {
            if let Ok(res) = extract_folder_icon(IconSize::Jumbo) {
                return Ok(res);
            }
        }

        let mut shfi = SHFILEINFOW::default();

        // Jumbo for files: resolve the extension's registered icon file, then
        // extract the 256px variant directly. Going through
        // IShellItemImageFactory::GetImage(256, ICONONLY) instead would make
        // Windows composite registered-but-small icons onto a themed
        // backplate (tiny glyph on a large tile), which never matches
        // Explorer's list rendering.
        if matches!(size, IconSize::Jumbo) && !is_folder {
            // Preferred: SHGFI_ICONLOCATION. The shell frequently fills only
            // iIcon and leaves the path empty, so fall back to reading the
            // ProgID's DefaultIcon from the registry the same way Explorer does.
            let loc_result = SHGetFileInfoW(
                PCWSTR(
                    format!(
                        "file.{}",
                        super::canonical_icon_ext(extension.trim_start_matches('.'))
                    )
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
                ),
                windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
                Some(&mut shfi),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICONLOCATION | SHGFI_USEFILEATTRIBUTES,
            );
            let mut location: Option<(String, i32)> = None;
            if loc_result != 0 {
                let icon_path: String = shfi
                    .szDisplayName
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| char::from_u32(c as u32).unwrap_or('\0'))
                    .collect();
                if !icon_path.is_empty() {
                    location = Some((icon_path, shfi.iIcon));
                }
            }
            let location = location.or_else(|| {
                registered_icon_location(super::canonical_icon_ext(
                    extension.trim_start_matches('.'),
                ))
            });

            if let Some((icon_path, index)) = location {
                let mut hicon = HICON::default();
                let extract = SHDefExtractIconW(
                    PCWSTR(
                        icon_path
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect::<Vec<u16>>()
                            .as_ptr(),
                    ),
                    index,
                    0,
                    Some(&mut hicon),
                    None,
                    256,
                );
                if extract.is_ok() && !hicon.is_invalid() {
                    let conversion =
                        crate::infrastructure::windows::bitmap_conversion::hicon_to_rgba(hicon);
                    let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(hicon);
                    if let Ok(res) = conversion {
                        return Ok(res);
                    }
                }
            }
            // Fall through to the 48px SHGFI path if the 256px extraction
            // fails; a slightly soft icon beats a wrong one.
        }

        let flags = SHGFI_ICON
            | SHGFI_USEFILEATTRIBUTES
            | match size {
                IconSize::Small => SHGFI_SMALLICON,
                IconSize::Large | IconSize::Jumbo => SHGFI_LARGEICON,
            };

        let file_attributes = if is_folder {
            windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
        } else {
            windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL
        };

        // CORRECTED STRATEGY:
        // Do not use absolute paths (C:\...). Use only a simple name.
        let dummy_name = if is_folder {
            "folder".to_string()
        } else {
            let clean_ext = extension.trim_start_matches('.');
            let mapped_ext = super::canonical_icon_ext(clean_ext);
            if mapped_ext.is_empty() {
                "file".to_string()
            } else {
                format!("file.{}", mapped_ext) // ex: "file.rar"
            }
        };

        let wide_path: Vec<u16> = dummy_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let result = SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            file_attributes,
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );

        if result == 0 || shfi.hIcon.is_invalid() {
            return Err(format!("Falha SHGetFileInfoW para: {}", dummy_name).into());
        }

        let hicon = shfi.hIcon;

        // Reuse the hicon_to_rgba function we already implemented
        let conversion_result =
            crate::infrastructure::windows::bitmap_conversion::hicon_to_rgba(hicon);

        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(hicon);

        conversion_result
    }
}
