//! Putting text where the caret is.
//!
//! Genuinely the hardest thing in the product to get right, because it fails
//! differently in every target application. Two strategies, chosen by length:
//!
//! - **Short text → synthesized Unicode keystrokes.** Works essentially everywhere,
//!   including terminals and Electron apps, and has no clipboard side effects. Too
//!   slow past a couple of hundred characters, and some applications drop input
//!   delivered that fast.
//! - **Long text → clipboard paste, with the clipboard restored afterwards.**
//!   Instant regardless of length, but it borrows a resource the user owns.
//!
//! Two details here are non-obvious and each costs an afternoon to rediscover:
//!
//! 1. **Restore *all* clipboard formats, not just text.** The common shortcut of
//!    saving `CF_UNICODETEXT` and putting it back destroys a copied image or
//!    copied rich text. Users do not connect the loss to the dictation tool.
//! 2. **Release the physical modifier before synthesizing `Ctrl+V`.** The
//!    push-to-talk key is itself a modifier (Right Ctrl by default). If it is still
//!    physically held, the synthetic paste lands as a different chord and silently
//!    does nothing.

use ov_core::error::{Error, Result};
use ov_core::ports::TextSink;
use ov_core::types::{InjectMode, InjectReceipt};

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
};

const CF_UNICODETEXT: u32 = 13;
const VK_CONTROL: u16 = 0x11;
const VK_LCONTROL: u16 = 0xA2;
const VK_RCONTROL: u16 = 0xA3;
const VK_V: u16 = 0x56;

/// Formats whose handles are not `HGLOBAL` and therefore cannot be snapshotted by
/// copying bytes. Attempting it would corrupt memory, so they are skipped and the
/// caller is told the snapshot is partial.
const NON_GLOBAL_FORMATS: &[u32] = &[
    2,  // CF_BITMAP        (HBITMAP)
    3,  // CF_METAFILEPICT  (HMETAFILEPICT)
    9,  // CF_PALETTE       (HPALETTE)
    14, // CF_ENHMETAFILE   (HENHMETAFILE)
];

/// `TextSink` for Windows.
pub struct WinTextSink {
    /// Above this many characters, injection switches to clipboard paste.
    paste_threshold: usize,
}

impl WinTextSink {
    /// Create a sink. `paste_threshold` comes from `Config::paste_threshold_chars`.
    #[must_use]
    pub fn new(paste_threshold: usize) -> Self {
        Self { paste_threshold }
    }
}

impl Default for WinTextSink {
    fn default() -> Self {
        Self::new(120)
    }
}

impl TextSink for WinTextSink {
    fn inject(&self, text: &str, mode: InjectMode) -> Result<InjectReceipt> {
        if text.is_empty() {
            return Ok(InjectReceipt { mode, chars: 0 });
        }
        let chars = text.chars().count();

        let chosen = mode;
        match chosen {
            InjectMode::ClipboardOnly => {
                set_clipboard_text(text)?;
                Ok(InjectReceipt {
                    mode: chosen,
                    chars,
                })
            }
            InjectMode::Keystrokes if chars <= self.paste_threshold => {
                send_unicode(text)?;
                Ok(InjectReceipt {
                    mode: InjectMode::Keystrokes,
                    chars,
                })
            }
            _ => {
                // Long text, or the caller explicitly asked to paste.
                match paste_with_restore(text) {
                    Ok(()) => Ok(InjectReceipt {
                        mode: InjectMode::ClipboardPaste,
                        chars,
                    }),
                    Err(e) => {
                        // Leave the text on the clipboard so nothing is lost, and
                        // let the caller tell the user to press Ctrl+V themselves.
                        let _ = set_clipboard_text(text);
                        Err(e)
                    }
                }
            }
        }
    }
}

/// Choose the mode for a given length.
///
/// Clipboard paste is the default for anything but the shortest text, which
/// reverses the original design. The reason is empirical: synthesized keystrokes
/// were corrupting real dictation (see `KEYSTROKE_CHUNK` in this module), and a
/// *atomic* — the target reads the whole string from the clipboard in one
/// operation, so there is no queue to overflow and no way to lose characters.
///
/// The cost is that the clipboard is briefly borrowed, which the snapshot and
/// restore in `paste_with_restore` already makes safe. Correct text is worth far
/// more than avoiding that.
#[must_use]
pub fn mode_for(text: &str, threshold: usize) -> InjectMode {
    if text.chars().count() <= threshold {
        InjectMode::Keystrokes
    } else {
        InjectMode::ClipboardPaste
    }
}

/// Characters per `SendInput` call, and the pause between calls.
///
/// # Why this is not one big call
///
/// Sending the whole utterance in a single `SendInput` corrupts it. Every
/// `KEYEVENTF_UNICODE` event carries `wVk = 0`, so Windows assigns them all the
/// same virtual key (`VK_PACKET`) and distinguishes the character only by
/// `wScan`. When events arrive faster than the target thread drains its input
/// queue, adjacent same-virtual-key entries are merged: the *repeat count*
/// survives but only one `wScan` does, and `TranslateMessage` then emits N copies
/// of that single character.
///
/// The signature is unmistakable and was observed in production:
///
/// ```text
///   "Are you the best?"       ->  "Are ?????????????"
///   "Be the best voice model" ->  "Be llllllllllllllllllll"
///   "Hello, check check check" -> "Hello,kkkkkkkkkkkkkkkkkk"
/// ```
///
/// Output length always matched the input exactly, a short prefix was correct,
/// and the rest was a run of the string's *last* character. Reproduced
/// independently in a standalone script with no audio and no model involved.
const KEYSTROKE_CHUNK: usize = 4;
const KEYSTROKE_GAP: std::time::Duration = std::time::Duration::from_millis(2);

/// Synthesize one Unicode key event per UTF-16 code unit, paced.
///
/// Surrogate pairs are sent as two events, which is what `KEYEVENTF_UNICODE`
/// expects; sending a `char` directly would drop anything outside the BMP.
///
/// Pacing reduces the corruption above but does not eliminate it for long strings,
/// which is why [`InjectMode::ClipboardPaste`] is the primary path and this is the
/// fallback for short text and for targets that refuse a paste.
fn send_unicode(text: &str) -> Result<()> {
    let units: Vec<u16> = text.encode_utf16().collect();

    for chunk in units.chunks(KEYSTROKE_CHUNK) {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(chunk.len() * 2);
        for &unit in chunk {
            inputs.push(key_input(0, unit, KEYEVENTF_UNICODE));
            inputs.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        send_inputs(&inputs)?;
        std::thread::sleep(KEYSTROKE_GAP);
    }
    Ok(())
}

/// Tag on our synthetic events.
///
/// Two purposes: it makes OpenVoice's own input identifiable, and a distinct
/// `dwExtraInfo` discourages the input queue from coalescing adjacent
/// `VK_PACKET` entries, which is the mechanism behind the corruption described on
/// [`KEYSTROKE_CHUNK`].
const OV_EXTRA_INFO: usize = 0x4F56_0001; // "OV\0\1"

fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: OV_EXTRA_INFO,
            },
        },
    }
}

fn send_inputs(inputs: &[INPUT]) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    // SAFETY: `inputs` is a valid slice of correctly initialised INPUT structs and
    // the size argument matches the struct Windows expects.
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(Error::Injection(format!(
            "SendInput accepted {sent} of {} events (input may be blocked by a \
             more-privileged window)",
            inputs.len()
        )));
    }
    Ok(())
}

/// How long to leave our text on the clipboard before handing it back.
///
/// The target reads the clipboard *asynchronously*, some time after receiving the
/// keystroke. A 140 ms delay was tried and lost the race against an Electron-based
/// editor: the old contents were restored before the app looked, so `Ctrl+V` pasted
/// the user's previous clipboard — or nothing — while the engine happily recorded
/// "delivered". A second is imperceptible and comfortably outlasts a slow app.
const CLIPBOARD_HOLD: std::time::Duration = std::time::Duration::from_millis(1000);

/// Snapshot the clipboard, paste, then put the clipboard back later.
fn paste_with_restore(text: &str) -> Result<()> {
    let snapshot = ClipboardSnapshot::capture()?;
    set_clipboard_text(text)?;
    let result = send_paste_chord();

    // Restore off-thread so injection returns immediately. Blocking here made every
    // dictation feel slower *and* still lost the race.
    let ours = text.to_string();
    std::thread::Builder::new()
        .name("ov-clipboard-restore".into())
        .spawn(move || {
            std::thread::sleep(CLIPBOARD_HOLD);
            // Only take the clipboard back if it is still ours. If the user copied
            // something in the meantime, restoring would silently destroy it —
            // and quietly eating someone's clipboard is far worse than leaving a
            // transcript on it.
            match read_clipboard_text() {
                Some(current) if current == ours => snapshot.restore(),
                Some(_) => tracing::debug!("clipboard changed since paste; not restoring"),
                None => snapshot.restore(),
            }
        })
        .map_err(|e| Error::Injection(format!("clipboard restore thread: {e}")))?;

    result
}

/// Current clipboard text, if any.
fn read_clipboard_text() -> Option<String> {
    let _guard = ClipboardGuard::open().ok()?;
    // SAFETY: the clipboard is open and the format is checked before the handle is
    // interpreted as UTF-16 text.
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.ok()?;
    if handle.is_invalid() {
        return None;
    }
    let bytes = read_global(handle)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// Send `Ctrl+V`, first clearing any physically-held Control key.
fn send_paste_chord() -> Result<()> {
    let mut inputs = Vec::with_capacity(8);

    // If the user is still holding the push-to-talk key and it is a Control, the
    // synthetic Ctrl+V arrives as a corrupted chord and does nothing at all.
    for vk in [VK_LCONTROL, VK_RCONTROL] {
        // SAFETY: GetAsyncKeyState takes a virtual key code and has no
        // preconditions beyond that.
        let down = unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000 != 0;
        if down {
            inputs.push(key_input(vk, 0, KEYEVENTF_KEYUP));
        }
    }

    inputs.push(key_input(VK_CONTROL, 0, KEYBD_EVENT_FLAGS(0)));
    inputs.push(key_input(VK_V, 0, KEYBD_EVENT_FLAGS(0)));
    inputs.push(key_input(VK_V, 0, KEYEVENTF_KEYUP));
    inputs.push(key_input(VK_CONTROL, 0, KEYEVENTF_KEYUP));

    send_inputs(&inputs)
}

/// RAII guard for the clipboard, so it is always closed even on an early return.
struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<Self> {
        // The clipboard is a shared, singly-owned system resource; another process
        // may hold it briefly. Retrying is normal, not exceptional.
        for attempt in 0..10 {
            // SAFETY: passing None requests association with the current task.
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Self);
            }
            std::thread::sleep(std::time::Duration::from_millis(10 * (attempt + 1)));
        }
        Err(Error::Injection(
            "another application is holding the clipboard".into(),
        ))
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: only constructed after a successful OpenClipboard.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Every `HGLOBAL`-backed clipboard format, copied out as raw bytes.
struct ClipboardSnapshot {
    items: Vec<(u32, Vec<u8>)>,
    partial: bool,
}

impl ClipboardSnapshot {
    fn capture() -> Result<Self> {
        let _guard = ClipboardGuard::open()?;
        let mut items = Vec::new();
        let mut partial = false;
        let mut format = 0u32;

        loop {
            // SAFETY: EnumClipboardFormats walks the format list; 0 starts it and 0
            // is returned when the list is exhausted. The clipboard is open.
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            if NON_GLOBAL_FORMATS.contains(&format) {
                partial = true;
                continue;
            }
            // SAFETY: the clipboard is open and `format` came from the enumeration.
            let Ok(handle) = (unsafe { GetClipboardData(format) }) else {
                // Formats using delayed rendering can legitimately fail here.
                partial = true;
                continue;
            };
            if handle.is_invalid() {
                partial = true;
                continue;
            }
            if let Some(bytes) = read_global(handle) {
                items.push((format, bytes));
            } else {
                partial = true;
            }
        }

        if partial {
            tracing::debug!("clipboard snapshot is partial; some formats cannot be preserved");
        }
        Ok(Self { items, partial })
    }

    fn restore(self) {
        if self.items.is_empty() && !self.partial {
            return;
        }
        let Ok(_guard) = ClipboardGuard::open() else {
            tracing::warn!("could not reopen clipboard to restore previous contents");
            return;
        };
        // SAFETY: the clipboard is open and owned by this task.
        unsafe {
            let _ = EmptyClipboard();
        }
        for (format, bytes) in self.items {
            if let Some(handle) = alloc_global(&bytes) {
                // SAFETY: `handle` is a fresh GMEM_MOVEABLE allocation of the right
                // size. On success the system takes ownership, so it is not freed
                // here.
                unsafe {
                    if SetClipboardData(format, HANDLE(handle.0)).is_err() {
                        tracing::debug!(format, "could not restore clipboard format");
                    }
                }
            }
        }
    }
}

/// Copy the bytes behind an `HGLOBAL` clipboard handle.
fn read_global(handle: HANDLE) -> Option<Vec<u8>> {
    let hglobal = HGLOBAL(handle.0);
    // SAFETY: `handle` is a live clipboard handle for an HGLOBAL-backed format.
    unsafe {
        let size = GlobalSize(hglobal);
        if size == 0 {
            return None;
        }
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr as *const u8, size).to_vec();
        let _ = GlobalUnlock(hglobal);
        Some(bytes)
    }
}

/// Allocate movable global memory and fill it.
fn alloc_global(bytes: &[u8]) -> Option<HGLOBAL> {
    // SAFETY: allocation size is non-zero; the pointer is locked before writing and
    // unlocked afterwards, and exactly `bytes.len()` bytes are written.
    unsafe {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).ok()?;
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(hglobal);
        Some(hglobal)
    }
}

/// Replace the clipboard with `text` as `CF_UNICODETEXT`.
fn set_clipboard_text(text: &str) -> Result<()> {
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0); // clipboard text must be NUL-terminated
    let bytes: Vec<u8> = utf16.iter().flat_map(|u| u.to_le_bytes()).collect();

    let _guard = ClipboardGuard::open()?;
    // SAFETY: the clipboard is open and owned by this task.
    unsafe {
        EmptyClipboard().map_err(|e| Error::Injection(format!("EmptyClipboard: {e}")))?;
    }
    let handle = alloc_global(&bytes)
        .ok_or_else(|| Error::Injection("could not allocate clipboard memory".into()))?;
    // SAFETY: fresh allocation of the correct size; the system takes ownership.
    unsafe {
        SetClipboardData(CF_UNICODETEXT, HANDLE(handle.0))
            .map_err(|e| Error::Injection(format!("SetClipboardData: {e}")))?;
    }
    Ok(())
}
