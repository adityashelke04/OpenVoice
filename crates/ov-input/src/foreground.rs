//! Which application currently has focus.
//!
//! Used to select a formatting profile. Read on hotkey *press*, never on release:
//! by the time text is injected the user may have switched windows, and the profile
//! must reflect where they were speaking, not where they landed.

use ov_core::error::Result;
use ov_core::ports::AppContext;
use ov_core::types::ForegroundApp;

use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

/// `AppContext` for Windows.
#[derive(Default)]
pub struct WinForeground;

impl AppContext for WinForeground {
    fn foreground(&self) -> Result<ForegroundApp> {
        // SAFETY: GetForegroundWindow has no preconditions and may return null,
        // which is checked below.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            // No foreground window is a normal transient state, not an error: the
            // default profile applies.
            return Ok(ForegroundApp::default());
        }

        let mut pid = 0u32;
        // SAFETY: `hwnd` is non-null and `pid` is a valid out-param.
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }

        let mut title = [0u16; 512];
        // SAFETY: buffer is valid for the length passed.
        let len = unsafe { GetWindowTextW(hwnd, &mut title) };
        let title = String::from_utf16_lossy(&title[..len.max(0) as usize]);

        Ok(ForegroundApp {
            exe: exe_name(pid).unwrap_or_default(),
            title,
        })
    }
}

/// Executable file name for a process id, e.g. `Code.exe`.
///
/// `PROCESS_QUERY_LIMITED_INFORMATION` is used deliberately: it is the least
/// privilege that answers this question, and unlike the broader rights it also
/// works against elevated processes without needing elevation ourselves.
fn exe_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // SAFETY: OpenProcess validates the pid and returns an error handle on failure.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;

    let mut buf = [0u16; MAX_PATH as usize];
    let mut len = buf.len() as u32;
    // SAFETY: `handle` is live, and `buf`/`len` describe a valid writable buffer.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: `handle` came from a successful OpenProcess and is closed exactly once.
    unsafe {
        let _ = CloseHandle(handle);
    }
    ok.ok()?;

    let full = String::from_utf16_lossy(&buf[..len as usize]);
    full.rsplit(['\\', '/']).next().map(str::to_string)
}
