//! # ov-core — the OpenVoice domain
//!
//! The session lifecycle, the port definitions, the event vocabulary, and the
//! configuration schema. Nothing else.
//!
//! ## The one rule
//!
//! **This crate must not depend on the operating system, the filesystem, the
//! network, an audio library, a GUI toolkit, or an async runtime.**
//!
//! That is not a stylistic preference. Everything platform-specific in OpenVoice is
//! slow to test and flaky in CI: a low-level keyboard hook, WASAPI capture, GPU
//! inference, synthetic input. If the session logic and the formatting rules were
//! entangled with any of it, verifying a formatter tweak would mean launching a GUI
//! and speaking into a microphone. That feedback loop is too slow to sustain, and it
//! is the most likely way a project like this quietly dies.
//!
//! The rule is enforced mechanically, not by discipline: CI compiles this crate for
//! `wasm32-unknown-unknown`, a target where none of those dependencies can link. If
//! the boundary leaks, the build goes red.
//!
//! ```sh
//! cargo check -p ov-core --target wasm32-unknown-unknown
//! ```
//!
//! ## Shape
//!
//! [`session::SessionMachine`] is a pure function of its inputs:
//! `handle(Input) -> Vec<Effect>`. It reads no clock — time arrives as
//! [`types::Millis`] stamped by the caller — and performs no IO. The composition
//! root in `ov-app` owns every adapter, feeds inputs in, and executes effects out.
//!
//! See `docs/DESIGN.md` and `docs/adr/0001-hexagonal-architecture.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod config;
pub mod error;
pub mod event;
pub mod ports;
pub mod session;
pub mod types;

pub use error::{Error, Result};
pub use event::{Event, NoticeLevel, Stage};
pub use session::{Effect, Input, SessionMachine, SessionRecord};
pub use types::{ForegroundApp, InjectMode, Millis, Outcome, SessionId, Transcript};
