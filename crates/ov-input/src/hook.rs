//! The global low-level keyboard hook.
//!
//! # Why a hook rather than `RegisterHotKey`
//!
//! `RegisterHotKey` delivers a single "pressed" notification with no key-up event,
//! so push-to-talk cannot be built on it at all. That forces `WH_KEYBOARD_LL`.
//!
//! # The rules this thread lives by
//!
//! 1. **The callback must return in well under 10 ms.** Windows silently evicts a
//!    hook whose procedure is too slow, and every hotkey in the app then stops
//!    working with no error anywhere. So the callback compares one integer and
//!    pushes to an unbounded channel. No locks, no allocation beyond the send, no
//!    logging, no I/O.
//! 2. **The hook needs its own thread with a message pump.** Low-level hooks are
//!    delivered to the installing thread's message queue; without `GetMessage` the
//!    callback is never invoked.
//! 3. **Injected input is ignored.** Our own `SendInput` calls come back through
//!    this hook. Acting on them would make the app respond to itself.
//! 4. **Keys are never swallowed unless the binding is exclusive.** Returning
//!    non-zero from the callback eats the keystroke for the whole system, so the
//!    default binding (Right Ctrl) must pass through or the user's own shortcuts
//!    break.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

use ov_core::config::Chord;
use ov_core::error::{Error, Result};
use ov_core::ports::{HotkeyEvent, HotkeyListener, HotkeySink};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::{vk_for, VK_ESCAPE};

/// The bound key's virtual code. Read by the hook callback on every keystroke, so
/// it is an atomic rather than anything that could block.
static BOUND_VK: AtomicU32 = AtomicU32::new(0xA3);
/// Whether the bound key is swallowed rather than passed through.
static EXCLUSIVE: AtomicBool = AtomicBool::new(false);
/// Set while the chord is physically down, so auto-repeat can be filtered here
/// rather than flooding the channel.
static CHORD_DOWN: AtomicBool = AtomicBool::new(false);

/// Where the callback posts events. `OnceLock` because the callback is an `extern
/// "system" fn` and cannot capture state.
static TX: OnceLock<Sender<HotkeyEvent>> = OnceLock::new();

/// Diagnostic tap: every key event, including injected ones, as
/// `(virtual_key, is_down, injected)`. Used by `ov keytest` to answer the single
/// most useful question when dictation "does nothing" — is the hook receiving
/// keystrokes at all, or is the problem further down the pipeline?
static DEBUG_TX: OnceLock<Sender<(u32, bool, bool)>> = OnceLock::new();

/// Handle to the installed hook, kept so it can be removed on shutdown.
static HOOK: Mutex<Option<isize>> = Mutex::new(None);

/// Enable the diagnostic tap before starting the listener.
///
/// Returns a receiver of `(virtual_key, is_down, injected)` for every keystroke.
/// Call this *before* [`HotkeyListener::start`].
pub fn enable_key_debug() -> std::sync::mpsc::Receiver<(u32, bool, bool)> {
    let (tx, rx) = channel();
    let _ = DEBUG_TX.set(tx);
    rx
}

/// `HotkeyListener` backed by `SetWindowsHookEx(WH_KEYBOARD_LL)`.
#[derive(Default)]
pub struct WinHotkeyListener;

impl WinHotkeyListener {
    /// Create a listener. Nothing is installed until [`HotkeyListener::start`].
    #[must_use]
    pub fn new(chord: Chord) -> Self {
        BOUND_VK.store(vk_for(chord.key), Ordering::Relaxed);
        EXCLUSIVE.store(chord.exclusive, Ordering::Relaxed);
        Self
    }
}

impl HotkeyListener for WinHotkeyListener {
    fn start(&self, sink: HotkeySink) -> Result<()> {
        let (tx, rx) = channel::<HotkeyEvent>();
        TX.set(tx)
            .map_err(|_| Error::Hotkey("hotkey listener already started".into()))?;

        // Fan-out thread. Kept separate from the hook thread so that a slow
        // consumer can never stall the hook callback -- which would get the hook
        // evicted and silently break every hotkey in the app.
        std::thread::Builder::new()
            .name("ov-hotkey-dispatch".into())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    sink(event);
                }
            })
            .map_err(|e| Error::Hotkey(format!("dispatch thread: {e}")))?;

        let (ready_tx, ready_rx) = channel::<Result<()>>();

        std::thread::Builder::new()
            .name("ov-hotkey-hook".into())
            .spawn(move || {
                // SAFETY: `hook_proc` is a valid `extern "system"` callback with the
                // signature Windows expects. Passing a null module handle with a
                // non-zero thread id is required for a thread-local low-level hook.
                let installed =
                    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) };

                match installed {
                    Ok(h) => {
                        *HOOK.lock().expect("hook mutex poisoned") = Some(h.0 as isize);
                        let _ = ready_tx.send(Ok(()));
                        pump_messages();
                    }
                    Err(e) => {
                        let _ = ready_tx
                            .send(Err(Error::Hotkey(format!("SetWindowsHookExW failed: {e}"))));
                    }
                }
            })
            .map_err(|e| Error::Hotkey(format!("hook thread: {e}")))?;

        ready_rx
            .recv()
            .map_err(|_| Error::Hotkey("hook thread died during startup".into()))?
    }

    fn rebind(&self, chord: &Chord) -> Result<()> {
        BOUND_VK.store(vk_for(chord.key), Ordering::Relaxed);
        EXCLUSIVE.store(chord.exclusive, Ordering::Relaxed);
        CHORD_DOWN.store(false, Ordering::Relaxed);
        tracing::info!(key = ?chord.key, exclusive = chord.exclusive, "hotkey rebound");
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        if let Some(h) = HOOK.lock().expect("hook mutex poisoned").take() {
            // SAFETY: `h` came from a successful SetWindowsHookExW and has not been
            // unhooked before -- `take()` guarantees this runs at most once.
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(h as *mut _));
            }
        }
        Ok(())
    }
}

/// Message pump for the hook thread.
///
/// Low-level hooks are dispatched through the installing thread's message queue.
/// Without this loop the callback is simply never called, which presents as "the
/// hotkey does nothing" with no error to go on.
fn pump_messages() {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid, properly initialised MSG for the duration of each
    // call. A null HWND asks for messages belonging to this thread.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// The hook callback.
///
/// Runs on every keystroke system-wide. Everything here is on the critical path of
/// the user's typing, so it does the minimum possible: one integer comparison and a
/// channel send. It deliberately stores nothing.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Negative code means "pass through without inspecting", per the Win32 contract.
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // SAFETY: for code >= 0 in a WH_KEYBOARD_LL hook, Windows guarantees lparam
    // points to a valid KBDLLHOOKSTRUCT for the duration of this call.
    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };

    let injected = info.flags
        & windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT_FLAGS(LLKHF_INJECTED.0)
        != windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT_FLAGS(0);

    let vk = info.vkCode;
    let bound = BOUND_VK.load(Ordering::Relaxed);
    let msg = wparam.0 as u32;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

    if let Some(tx) = DEBUG_TX.get() {
        if is_down || is_up {
            let _ = tx.send((vk, is_down, injected));
        }
    }

    // Our own SendInput comes back through here. Responding to it would make the
    // app react to its own typing.
    if injected {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    if vk == bound {
        if is_down {
            // Filter auto-repeat at the source. Holding a key emits key-down every
            // ~30 ms; forwarding all of them would flood the channel for the whole
            // duration of an utterance.
            if !CHORD_DOWN.swap(true, Ordering::Relaxed) {
                send(HotkeyEvent::Pressed);
            }
        } else if is_up {
            CHORD_DOWN.store(false, Ordering::Relaxed);
            send(HotkeyEvent::Released);
        }

        if EXCLUSIVE.load(Ordering::Relaxed) {
            // Swallow the key entirely. Only safe for a key with no other purpose,
            // which is why it is off by default.
            return LRESULT(1);
        }
    } else if vk == VK_ESCAPE && is_down {
        send(HotkeyEvent::Cancelled);
    }

    CallNextHookEx(None, code, wparam, lparam)
}

/// Non-blocking hand-off. A failed send means the dispatcher is gone, which is not
/// worth stalling the user's keyboard over.
fn send(event: HotkeyEvent) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(event);
    }
}
