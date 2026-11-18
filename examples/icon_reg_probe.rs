//! Debug: HKCR\.md → ProgID → DefaultIcon registry chain.
#[cfg(windows)]
fn main() {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CLASSES_ROOT, RRF_RT_REG_SZ,
    };

    for ext in ["md", "toml", "gitignore", "rs", "lock"] {
        let read_sz = |subkey: &str| -> Option<String> {
            let subkey_wide: Vec<u16> =
                subkey.encode_utf16().chain(std::iter::once(0)).collect();
            let mut buf = [0u16; 512];
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
        let progid = read_sz(&format!(".{ext}")).unwrap_or_default();
        let di = if progid.is_empty() {
            read_sz(&format!(".{ext}\\DefaultIcon")).unwrap_or_default()
        } else {
            read_sz(&format!("{progid}\\DefaultIcon")).unwrap_or_default()
        };
        println!(".{ext}: progid={progid:?} default_icon={di:?}");
    }
}

#[cfg(not(windows))]
fn main() {}
