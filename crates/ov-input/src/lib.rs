//! # ov-input — the OS adapters for keyboard and text
//!
//! Implements three of the six ports: [`ov_core::ports::HotkeyListener`],
//! [`ov_core::ports::TextSink`], and [`ov_core::ports::AppContext`].
//!
//! This is the crate with the largest blast radius in the project. It installs a
//! global keyboard hook and synthesizes keystrokes — the same two capabilities a
//! keylogger has. Everything here is written to be read by a sceptical stranger:
//! the hook stores nothing, compares one virtual key code, and discards the event.
//!
//! `unsafe` is unavoidable (it is all Win32 FFI) and is allowed here, unlike in the
//! pure crates where it is forbidden outright. Every `unsafe` block carries a
//! comment stating the invariant that makes it sound.

#![warn(missing_docs, clippy::all)]

#[cfg(windows)]
mod foreground;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod inject;

#[cfg(windows)]
pub use foreground::WinForeground;
#[cfg(windows)]
pub use hook::{enable_key_debug, WinHotkeyListener};
#[cfg(windows)]
pub use inject::{mode_for, WinTextSink};

/// Virtual key codes for the keys OpenVoice can bind.
///
/// Kept as a plain mapping rather than pulled from the `windows` crate at each call
/// site so the pure config enum stays free of platform types.
#[cfg(windows)]
pub(crate) fn vk_for(key: ov_core::config::Key) -> u32 {
    use ov_core::config::Key;
    match key {
        Key::RightCtrl => 0xA3,  // VK_RCONTROL
        Key::RightAlt => 0xA5,   // VK_RMENU
        Key::RightShift => 0xA1, // VK_RSHIFT
        Key::CapsLock => 0x14,   // VK_CAPITAL
        Key::F13 => 0x7C,
        Key::F14 => 0x7D,
        Key::ScrollLock => 0x91, // VK_SCROLL
    }
}

/// `VK_ESCAPE`. Cancels an in-flight session from anywhere.
#[cfg(windows)]
pub(crate) const VK_ESCAPE: u32 = 0x1B;
