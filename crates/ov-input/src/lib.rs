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
        Key::ScrollLock => 0x91, // VK_SCROLL
        Key::Pause => 0x13,      // VK_PAUSE
        // VK_F13..VK_F24 are contiguous from 0x7C.
        Key::F13 => 0x7C,
        Key::F14 => 0x7D,
        Key::F15 => 0x7E,
        Key::F16 => 0x7F,
        Key::F17 => 0x80,
        Key::F18 => 0x81,
        Key::F19 => 0x82,
        Key::F20 => 0x83,
        Key::F21 => 0x84,
        Key::F22 => 0x85,
        Key::F23 => 0x86,
        Key::F24 => 0x87,
    }
}

/// `VK_ESCAPE`. Cancels an in-flight session from anywhere.
#[cfg(windows)]
pub(crate) const VK_ESCAPE: u32 = 0x1B;

#[cfg(test)]
#[cfg(windows)]
mod vk_tests {
    use super::vk_for;
    use ov_core::config::Key;

    #[test]
    fn every_bindable_key_maps_to_a_distinct_virtual_key() {
        // Two keys sharing a code would make one of them silently bind the other,
        // which is invisible until a user picks the wrong one and reports that
        // their hotkey does nothing.
        let mut seen = std::collections::HashMap::new();
        for k in Key::ALL {
            let vk = vk_for(*k);
            assert!(vk != 0, "{k:?} has no virtual-key code");
            if let Some(other) = seen.insert(vk, *k) {
                panic!("{k:?} and {other:?} both map to {vk:#04x}");
            }
        }
    }

    #[test]
    fn no_bindable_key_collides_with_escape() {
        // Escape cancels an in-flight session from anywhere. Binding dictation to
        // it would make the cancel and the trigger the same keystroke.
        for k in Key::ALL {
            assert_ne!(vk_for(*k), super::VK_ESCAPE, "{k:?} is Escape");
        }
    }
}
