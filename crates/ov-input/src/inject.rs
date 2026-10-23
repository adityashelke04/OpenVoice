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
//!   Most targets need one paste; verified Claude targets use paced pieces.
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
use ov_core::ports::AppContext;
use ov_core::ports::TextSink;
use ov_core::types::{ForegroundApp, InjectMode, InjectReceipt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
    GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
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
    2,    // CF_BITMAP        (HBITMAP)
    3,    // CF_METAFILEPICT  (HMETAFILEPICT)
    9,    // CF_PALETTE       (HPALETTE)
    14,   // CF_ENHMETAFILE   (HENHMETAFILE)
    0x80, // CF_OWNERDISPLAY
    0x82, // CF_DSPBITMAP
    0x83, // CF_DSPMETAFILEPICT
    0x8e, // CF_DSPENHMETAFILE
];

/// `TextSink` for Windows.
pub struct WinTextSink {
    /// Above this many characters, injection switches to clipboard paste.
    paste_threshold: usize,
    clipboard: Arc<Mutex<crate::ownership::Lease<ClipboardSnapshot>>>,
    serial: Mutex<()>,
}

impl WinTextSink {
    /// Create a sink. `paste_threshold` comes from `Config::paste_threshold_chars`.
    #[must_use]
    pub fn new(paste_threshold: usize) -> Self {
        Self {
            paste_threshold,
            clipboard: Arc::new(Mutex::new(crate::ownership::Lease {
                snapshot: None,
                sequence: 0,
                generation: 0,
            })),
            serial: Mutex::new(()),
        }
    }
}

impl Default for WinTextSink {
    fn default() -> Self {
        Self::new(120)
    }
}

impl TextSink for WinTextSink {
    fn inject(&self, text: &str, mode: InjectMode) -> Result<InjectReceipt> {
        let target = crate::WinForeground.foreground().unwrap_or_default();
        self.inject_guarded(text, mode, &target, &AtomicBool::new(false))
    }
}

impl WinTextSink {
    /// Deliver only to the captured window; Escape is checked before every piece.
    /// Windows cannot make focus validation and SendInput one atomic operation.
    /// Callers must retain `text` for manual Copy on error; clipboard recovery is
    /// never allowed to overwrite content another application has just copied.
    pub fn inject_guarded(
        &self,
        text: &str,
        mode: InjectMode,
        target: &ForegroundApp,
        cancelled: &AtomicBool,
    ) -> Result<InjectReceipt> {
        let _serial = self.serial.lock().expect("delivery mutex");
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Injection("delivery cancelled".into()));
        }
        if text.is_empty() {
            return Ok(InjectReceipt { mode, chars: 0 });
        }
        let chars = text.chars().count();

        if mode != InjectMode::ClipboardOnly {
            check_target(target, cancelled)?;
        }

        let chosen = mode;
        match chosen {
            InjectMode::ClipboardOnly => {
                self.copy_recovery(text, cancelled)?;
                Ok(InjectReceipt {
                    mode: chosen,
                    chars,
                })
            }
            InjectMode::Keystrokes if chars <= self.paste_threshold => {
                send_unicode(text, target, cancelled)?;
                Ok(InjectReceipt {
                    mode: InjectMode::Keystrokes,
                    chars,
                })
            }
            _ => {
                // Long text, or the caller explicitly asked to paste.
                paste_with_restore(text, target, cancelled, &self.clipboard)?;
                Ok(InjectReceipt {
                    mode: InjectMode::ClipboardPaste,
                    chars,
                })
            }
        }
    }

    fn copy_recovery(&self, text: &str, cancelled: &AtomicBool) -> Result<()> {
        let mut owner = self.clipboard.lock().expect("clipboard ownership");
        let mut guard = ClipboardGuard::open()?;
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Injection("copy cancelled".into()));
        }
        guard.write_text(text)?;
        owner.generation += 1;
        owner.snapshot = None;
        Ok(())
    }
}

fn check_target(target: &ForegroundApp, cancelled: &AtomicBool) -> Result<()> {
    let current = crate::WinForeground.foreground().unwrap_or_default();
    if crate::delivery::allowed(target, &current, cancelled.load(Ordering::Acquire)) {
        Ok(())
    } else {
        Err(Error::Injection("delivery cancelled or original window no longer focused; use Copy for the complete transcript".into()))
    }
}

/// Choose the mode for a given length.
///
/// Clipboard paste is the default for anything but the shortest text, which
/// reverses the original design. The reason is empirical: synthesized keystrokes
/// were corrupting real dictation (see the paced Unicode sender), and a paste is
/// atomic — the target reads the whole string from the clipboard in one
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
const KEYSTROKE_GAP: std::time::Duration = std::time::Duration::from_millis(2);

/// Synthesize one Unicode key event per UTF-16 code unit, paced.
///
/// Surrogate pairs are sent as two events, which is what `KEYEVENTF_UNICODE`
/// expects; sending a `char` directly would drop anything outside the BMP.
///
/// Pacing reduces the corruption above but does not eliminate it for long strings,
/// which is why [`InjectMode::ClipboardPaste`] is the primary path and this is the
/// fallback for short text and for targets that refuse a paste.
fn send_unicode(text: &str, target: &ForegroundApp, cancelled: &AtomicBool) -> Result<()> {
    crate::delivery::unicode(text, |chunk| {
        check_target(target, cancelled)?;
        let mut inputs: Vec<INPUT> = Vec::with_capacity(chunk.len() * 2);
        for &unit in chunk {
            inputs.push(key_input(0, unit, KEYEVENTF_UNICODE));
            inputs.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        send_inputs(&inputs)?;
        std::thread::sleep(KEYSTROKE_GAP);
        Ok(())
    })
}

/// Tag on our synthetic events.
///
/// Two purposes: it makes OpenVoice's own input identifiable, and a distinct
/// `dwExtraInfo` discourages the input queue from coalescing adjacent
/// `VK_PACKET` entries, which is the mechanism behind the corruption described on
/// [`send_unicode`].
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
/// This is not the same race it looks like at first. The *target* reading the
/// clipboard after a synthetic `Ctrl+V` happens within milliseconds — that race
/// was already fixed by raising this from 140 ms to what used to be 1 second.
/// The race that mattered was a different one, reported directly: if the paste
/// silently fails to land (wrong window focus, an app that swallows synthetic
/// input, anything we cannot detect from here), the *user* is the one who has to
/// notice and press `Ctrl+V` themselves — and a human noticing a paste didn't
/// happen and reacting to it takes far longer than a second. At 1 second, the
/// clipboard was already wiped back to whatever it held before by the time
/// anyone could react, which made the documented "nothing is lost" guarantee
/// false in exactly the case it exists for.
///
/// Fifteen seconds is long enough to notice and react, short enough that the
/// user's previous clipboard isn't gone for the rest of their session.
const CLIPBOARD_HOLD: std::time::Duration = std::time::Duration::from_secs(15);

/// Largest piece pasted in one `Ctrl+V`, in characters.
///
/// Claude Code hides any paste over 800 characters behind a `[Pasted text #N]`
/// placeholder, so a long dictation looked like it never arrived. See
/// [`crate::chunk`] for the measurement. 700 leaves headroom under that limit.
/// Claude Code also collapses pastes of more than three lines.
/// Pause between pieces, so the target finishes one paste before the next one
/// arrives. 250 ms is the gap verified end to end; pieces sent back to back can
/// be merged or reassembled out of order by a terminal agent's paste detector.
const PASTE_CHUNK_GAP: std::time::Duration = std::time::Duration::from_millis(250);

/// Snapshot the clipboard, paste, then put the clipboard back later.
fn paste_with_restore(
    text: &str,
    target: &ForegroundApp,
    cancelled: &AtomicBool,
    ownership: &Arc<Mutex<crate::ownership::Lease<ClipboardSnapshot>>>,
) -> Result<()> {
    let mut owner = ownership.lock().expect("clipboard ownership");
    {
        let _guard = ClipboardGuard::open()?;
        let snapshot = ClipboardSnapshot::capture_locked()?;
        snapshot.require_complete()?;
        owner.begin(unsafe { GetClipboardSequenceNumber() }, snapshot);
    }
    let generation = owner.generation;
    let started = std::time::Instant::now();
    let result = crate::delivery::paste(
        text,
        target,
        &mut NativePaste {
            target,
            cancelled,
            owner: &mut owner,
        },
    );
    // The engine keeps full text in memory for Copy. Never replace a user's
    // intervening clipboard copy in an attempt to recover automatically.
    result?;
    tracing::info!(
        chars = text.chars().count(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "pasted transcript"
    );
    drop(owner);
    let ownership = ownership.clone();
    let ours = text.to_owned();
    std::thread::Builder::new()
        .name("ov-clipboard-restore".into())
        .spawn(move || {
            std::thread::sleep(CLIPBOARD_HOLD);
            let mut owner = ownership.lock().expect("clipboard ownership");
            if generation != owner.generation {
                return;
            }
            let Ok(_guard) = ClipboardGuard::open() else {
                return;
            };
            let current = read_clipboard_text_locked();
            if should_restore(true, current.as_deref(), &ours) {
                if let Some(snapshot) =
                    owner.take_for_restore(generation, unsafe { GetClipboardSequenceNumber() })
                {
                    snapshot.restore_locked();
                }
            } else {
                owner.snapshot = None;
            }
        })
        .map_err(|e| Error::Injection(format!("clipboard restore thread: {e}")))?;

    Ok(())
}

struct NativePaste<'a> {
    target: &'a ForegroundApp,
    cancelled: &'a AtomicBool,
    owner: &'a mut crate::ownership::Lease<ClipboardSnapshot>,
}

impl crate::delivery::PasteIo for NativePaste<'_> {
    fn check(&mut self) -> Result<()> {
        check_target(self.target, self.cancelled)
    }

    fn write(&mut self, text: &str) -> Result<()> {
        let mut guard = ClipboardGuard::open()?;
        check_target(self.target, self.cancelled)?;
        // Validate and replace under one lock; finalize only after rechecking
        // the same owner and text so a concurrent copy cannot become our lease.
        self.owner
            .write(unsafe { GetClipboardSequenceNumber() }, || {
                guard.write_text(text)
            })
    }

    fn paste(&mut self) -> Result<()> {
        {
            let _guard = ClipboardGuard::open()?;
            if unsafe { GetClipboardSequenceNumber() } != self.owner.sequence {
                return Err(Error::Injection(
                    "clipboard changed before paste; use Copy for the complete transcript".into(),
                ));
            }
        }
        check_target(self.target, self.cancelled)?;
        // Windows cannot atomically bind SendInput to a window or clipboard
        // sequence. Validate as late as possible; never steal foreground focus.
        send_paste_chord()
    }

    fn wait(&mut self) {
        std::thread::sleep(PASTE_CHUNK_GAP);
    }
}

/// Whether the background thread should hand the clipboard back to whatever
/// held it before the paste.
///
/// A failed paste must not schedule a restore over recovery text. Text equality
/// is only an extra check: the caller also checks the clipboard sequence and
/// lease generation while holding the clipboard open.
#[must_use]
fn should_restore(paste_sent: bool, current: Option<&str>, ours: &str) -> bool {
    if !paste_sent {
        return false;
    }
    // Only take the clipboard back if it is still ours. If the user copied
    // something in the meantime, restoring would silently destroy it — and
    // quietly eating someone's clipboard is far worse than leaving a
    // transcript on it.
    match current {
        Some(c) => c == ours,
        None => false,
    }
}

/// Current clipboard text, if any.
fn read_clipboard_text_locked() -> Option<String> {
    // SAFETY: the clipboard is open and the format is checked before the handle is
    // interpreted as UTF-16 text.
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.ok()?;
    if handle.is_invalid() {
        return None;
    }
    let bytes = read_global(handle)?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
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
struct ClipboardGuard {
    window: HWND,
    open: bool,
}

fn clipboard_window() -> Result<HWND> {
    // OpenClipboard(NULL) followed by EmptyClipboard leaves no owner, which
    // violates SetClipboardData's contract. A message-only window never takes focus.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::w!("STATIC"),
            windows::core::w!("OpenVoice clipboard"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            None,
            None,
        )
    }
    .map_err(|e| Error::Injection(format!("clipboard owner window: {e}")))
}

impl ClipboardGuard {
    fn write_text(&mut self, text: &str) -> Result<u32> {
        set_clipboard_text_locked(text)?;
        // Windows adds synthesized text formats on close, changing the sequence.
        unsafe { CloseClipboard() }
            .map_err(|e| Error::Injection(format!("CloseClipboard: {e}")))?;
        self.open = false;
        self.acquire()?;
        self.written_sequence(text)
    }

    fn written_sequence(&self, text: &str) -> Result<u32> {
        // Keep this HWND alive across close/reopen so a new copy cannot be adopted.
        if unsafe { GetClipboardOwner() }.ok() != Some(self.window)
            || read_clipboard_text_locked().as_deref() != Some(text)
        {
            return Err(Error::Injection(
                "clipboard changed while finalizing paste; use Copy for the complete transcript"
                    .into(),
            ));
        }
        Ok(unsafe { GetClipboardSequenceNumber() })
    }

    fn open() -> Result<Self> {
        let mut guard = Self {
            window: clipboard_window()?,
            open: false,
        };
        guard.acquire()?;
        Ok(guard)
    }

    fn acquire(&mut self) -> Result<()> {
        // The clipboard is a shared, singly-owned system resource; another process
        // may hold it briefly. Retrying is normal, not exceptional.
        for attempt in 0..10 {
            // SAFETY: this thread owns a live message-only window.
            if unsafe { OpenClipboard(self.window) }.is_ok() {
                self.open = true;
                return Ok(());
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
            if self.open {
                let _ = CloseClipboard();
            }
            let _ = DestroyWindow(self.window);
        }
    }
}

/// Every `HGLOBAL`-backed clipboard format, copied out as raw bytes.
struct ClipboardSnapshot {
    items: Vec<(u32, Vec<u8>)>,
    partial: bool,
}

impl ClipboardSnapshot {
    fn require_complete(&self) -> Result<()> {
        if self.partial {
            return Err(Error::Injection("clipboard has formats that cannot be preserved; clipboard unchanged, use Copy for the complete transcript".into()));
        }
        Ok(())
    }
    fn capture_locked() -> Result<Self> {
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
            if NON_GLOBAL_FORMATS.contains(&format) || (0x200..=0x2ff).contains(&format) {
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

    fn restore_locked(self) {
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
            let _ = GlobalFree(hglobal);
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(hglobal);
        Some(hglobal)
    }
}

/// Replace the clipboard with `text` as `CF_UNICODETEXT`.
fn set_clipboard_text_locked(text: &str) -> Result<()> {
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0); // clipboard text must be NUL-terminated
    let bytes: Vec<u8> = utf16.iter().flat_map(|u| u.to_le_bytes()).collect();

    let handle = alloc_global(&bytes)
        .ok_or_else(|| Error::Injection("could not allocate clipboard memory".into()))?;
    // Allocate before EmptyClipboard so allocation failure preserves user data.
    // SAFETY: clipboard is open; the system owns the fresh handle only on success.
    unsafe {
        if let Err(error) = EmptyClipboard() {
            let _ = GlobalFree(handle);
            return Err(Error::Injection(format!("EmptyClipboard: {error}")));
        }
        if let Err(error) = SetClipboardData(CF_UNICODETEXT, HANDLE(handle.0)) {
            let _ = GlobalFree(handle);
            return Err(Error::Injection(format!("SetClipboardData: {error}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated_clipboard(test: &str, run: impl FnOnce()) {
        const CHILD: &str = "OV_ISOLATED_CLIPBOARD_TEST";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test, "--nocapture", "--test-threads=1"])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "isolated native clipboard regression failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        // A separate station has its own clipboard: never inspect or change the user's.
        use windows::Win32::System::StationsAndDesktops::{
            CreateDesktopW, CreateWindowStationW, SetProcessWindowStation, SetThreadDesktop,
            DESKTOP_CONTROL_FLAGS,
        };
        unsafe {
            let station = CreateWindowStationW(None, 0, 0x1000_0000, None).unwrap();
            SetProcessWindowStation(station).unwrap();
            let desktop = CreateDesktopW(
                windows::core::w!("OpenVoiceClipboardRegression"),
                None,
                None,
                DESKTOP_CONTROL_FLAGS(0),
                0x1000_0000,
                None,
            )
            .unwrap();
            SetThreadDesktop(desktop).unwrap();
        }
        run();
    }

    #[test]
    fn clipboard_write_sequence_survives_close_and_owner_cleanup() {
        isolated_clipboard(
            "inject::tests::clipboard_write_sequence_survives_close_and_owner_cleanup",
            || {
                let mut guard = ClipboardGuard::open().unwrap();
                let written = guard.write_text("public clipboard regression").unwrap();
                drop(guard);
                let _guard = ClipboardGuard::open().unwrap();
                assert_eq!(
                    read_clipboard_text_locked().as_deref(),
                    Some("public clipboard regression")
                );
                assert_eq!(
                    unsafe { GetClipboardSequenceNumber() },
                    written,
                    "closing our own write invalidates the lease before paste"
                );
            },
        );
    }

    #[test]
    fn finalized_write_rejects_identical_text_copied_by_another_owner() {
        isolated_clipboard(
            "inject::tests::finalized_write_rejects_identical_text_copied_by_another_owner",
            || {
                let mut original = ClipboardGuard::open().unwrap();
                original.write_text("identical public text").unwrap();
                unsafe { CloseClipboard().unwrap() };
                original.open = false;
                let mut other = ClipboardGuard::open().unwrap();
                other.write_text("identical public text").unwrap();
                unsafe { CloseClipboard().unwrap() };
                other.open = false;
                original.acquire().unwrap();
                assert!(original.written_sequence("identical public text").is_err());
                assert_eq!(
                    read_clipboard_text_locked().as_deref(),
                    Some("identical public text")
                );
            },
        );
    }

    #[test]
    fn finalized_consecutive_writes_restore_original_native_clipboard() {
        isolated_clipboard(
            "inject::tests::finalized_consecutive_writes_restore_original_native_clipboard",
            || {
                let mut guard = ClipboardGuard::open().unwrap();
                guard.write_text("original public text").unwrap();
                drop(guard);
                let mut lease = crate::ownership::Lease {
                    snapshot: None,
                    sequence: 0,
                    generation: 0,
                };
                for text in ["first dictation", "second dictation"] {
                    let mut guard = ClipboardGuard::open().unwrap();
                    let snapshot = ClipboardSnapshot::capture_locked().unwrap();
                    snapshot.require_complete().unwrap();
                    lease.begin(unsafe { GetClipboardSequenceNumber() }, snapshot);
                    lease
                        .write(unsafe { GetClipboardSequenceNumber() }, || {
                            guard.write_text(text)
                        })
                        .unwrap();
                    drop(guard);
                    let _guard = ClipboardGuard::open().unwrap();
                    assert_eq!(unsafe { GetClipboardSequenceNumber() }, lease.sequence);
                    assert_eq!(read_clipboard_text_locked().as_deref(), Some(text));
                }
                let _guard = ClipboardGuard::open().unwrap();
                lease
                    .take_for_restore(lease.generation, unsafe { GetClipboardSequenceNumber() })
                    .unwrap()
                    .restore_locked();
                drop(_guard);
                let _guard = ClipboardGuard::open().unwrap();
                assert_eq!(
                    read_clipboard_text_locked().as_deref(),
                    Some("original public text")
                );
            },
        );
    }

    #[test]
    fn partial_snapshot_cannot_authorize_replacing_original_image() {
        let snapshot = ClipboardSnapshot {
            items: vec![],
            partial: true,
        };
        assert!(snapshot.require_complete().is_err());
    }

    #[test]
    fn clipboard_writes_have_a_valid_private_window_owner() {
        let hwnd = clipboard_window().unwrap();
        assert!(unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd) }.as_bool());
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd).unwrap();
        }
    }

    #[test]
    fn already_cancelled_short_unicode_never_reaches_send_input() {
        assert!(send_unicode("abc😀", &ForegroundApp::default(), &AtomicBool::new(true)).is_err());
    }

    #[test]
    fn never_restores_when_the_paste_never_sent() {
        // A failed paste leaves its text available until an explicit user copy.
        assert!(!should_restore(false, Some("our text"), "our text"));
        assert!(!should_restore(false, None, "our text"));
        assert!(!should_restore(false, Some("something else"), "our text"));
    }

    #[test]
    fn restores_after_a_successful_paste_if_the_clipboard_is_still_ours() {
        assert!(should_restore(true, Some("our text"), "our text"));
    }

    #[test]
    fn does_not_restore_after_success_if_the_user_copied_something_else() {
        assert!(!should_restore(true, Some("something else"), "our text"));
    }

    #[test]
    fn non_text_clipboard_is_not_ours_to_restore() {
        assert!(!should_restore(true, None, "our text"));
    }
}
