//! Which window material the Hub can use. Mica is Windows 11 only (build 22000+),
//! and pointless when the user has turned transparency effects off.
//!
//! The Hub window is created transparent with `windowEffects: ["mica"]` in
//! tauri.conf.json. On Windows 10 that request is silently ignored and the
//! transparent webview would show nothing behind the page, so the frontend has to
//! know which case it is in before it decides whether to paint its own backdrop.
//! It asks once, through `window_material`, and paints the ambient backdrop for
//! anything other than "mica".

/// Pure so it can be tested without a registry: `build` 0 means "could not read it".
pub fn decide(build: u32, transparency: bool) -> &'static str {
    if build >= 22000 && transparency {
        "mica"
    } else {
        "none"
    }
}

#[cfg(windows)]
mod reg {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};

    pub fn string(root: HKEY, key: &str, value: &str) -> Option<String> {
        let (key, value) = (HSTRING::from(key), HSTRING::from(value));
        // A build number is five digits; 64 UTF-16 units is ample. A value that
        // does not fit fails with ERROR_MORE_DATA and reads as "unknown", which
        // `decide` already treats as "no Mica".
        let mut buf = [0u16; 64];
        let mut bytes = std::mem::size_of_val(&buf) as u32;
        // SAFETY: `buf` outlives the call and `bytes` is its size in bytes, as
        // RegGetValueW requires; RRF_RT_REG_SZ guarantees a terminated string.
        let status = unsafe {
            RegGetValueW(
                root,
                &key,
                &value,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    pub fn dword(root: HKEY, key: &str, value: &str) -> Option<u32> {
        let (key, value) = (HSTRING::from(key), HSTRING::from(value));
        let mut data = 0u32;
        let mut bytes = std::mem::size_of::<u32>() as u32;
        // SAFETY: `data` is a live u32 and `bytes` is its size.
        let status = unsafe {
            RegGetValueW(
                root,
                &key,
                &value,
                RRF_RT_REG_DWORD,
                None,
                Some((&mut data as *mut u32).cast()),
                Some(&mut bytes),
            )
        };
        (status == ERROR_SUCCESS).then_some(data)
    }
}

/// HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\CurrentBuildNumber, or 0.
///
/// Read from the registry rather than GetVersionEx, which lies to any executable
/// without a Windows 10+ compatibility manifest entry.
pub fn windows_build() -> u32 {
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
        reg::string(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "CurrentBuildNumber",
        )
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Settings > Personalization > Colors > Transparency effects. A missing value
/// means the user never touched it, and the Windows default is on.
pub fn transparency_enabled() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::HKEY_CURRENT_USER;
        reg::dword(
            HKEY_CURRENT_USER,
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "EnableTransparency",
        )
        .map(|v| v != 0)
        .unwrap_or(true)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Make a transparent window repaint its own surface after it is first shown.
///
/// Tauri clears a transparent window's surface to alpha 0 (which is what lets Mica
/// through) when the window is created and again on WM_PAINT. For a window created
/// hidden, that first clear lands while it is invisible and does not stick, and
/// showing it does not produce a WM_PAINT: the webview child covers the whole
/// client area. Measured on the Hub: shown after load, the page sat on solid white
/// instead of Mica until the first resize. Invalidating the window forces the
/// WM_PAINT that runs the clear.
pub fn repaint(win: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Gdi::{
            RedrawWindow, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RDW_UPDATENOW,
        };
        let Ok(raw) = win.hwnd() else { return };
        // SAFETY: a live window handle owned by this process; no rect or region.
        unsafe {
            let _ = RedrawWindow(
                HWND(raw.0),
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_UPDATENOW,
            );
        }
    }
    #[cfg(not(windows))]
    let _ = win;
}

#[cfg(test)]
mod tests {
    use super::decide;
    #[test]
    fn mica_needs_windows_11_and_transparency() {
        assert_eq!(decide(22000, true), "mica");
        assert_eq!(decide(26100, true), "mica");
        assert_eq!(decide(19045, true), "none");
        assert_eq!(decide(26100, false), "none");
        assert_eq!(decide(0, true), "none"); // unreadable build number
    }

    /// Not an assertion about the machine, just that the reads work at all: a
    /// broken RegGetValueW call returns 0 and would quietly disable Mica forever.
    #[cfg(windows)]
    #[test]
    fn reads_a_real_build_number() {
        assert!(
            super::windows_build() >= 10240,
            "got {}",
            super::windows_build()
        );
    }
}
