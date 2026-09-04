//! # ov-asr — speech recognition
//!
//! Implements [`ov_core::ports::Transcriber`] with NVIDIA Parakeet TDT 0.6B v2,
//! decoding in this process through k2-fsa's `sherpa-onnx` bindings.
//!
//! ## What this used to be
//!
//! Until v0.5.0 this crate was a process supervisor. It spawned a frozen Python
//! interpreter running faster-whisper, spoke newline-delimited JSON to it over a
//! pipe, passed audio as a temp WAV, health-checked it, restarted it with
//! backoff, and held it in a Windows job object so a crash could not strand a
//! child on 1.6 GB of VRAM. It also owned a Hugging Face download manager,
//! because the weights arrived over the network on first run.
//!
//! All of that is gone. ADR 0003 chose it deliberately and recorded the reason —
//! faster-whisper got CUDA working from a pip wheel with no compiler, at a time
//! when the in-process alternative meant shipping the CUDA Toolkit — and it
//! recorded the follow-up: revisit in-process before distribution. ADR 0008 is
//! that follow-up, cashed in.
//!
//! ## What it is now
//!
//! Three small modules and no processes:
//!
//! * [`catalog`] — the models this build can run.
//! * [`sherpa`] — the [`ov_core::ports::Transcriber`] implementation.
//! * [`locate`] — where a model's weights are on this machine.
//! * [`recordings`] — sweeping up audio the user asked us to keep.
//!
//! The port did not change, and neither did `ov-core`, `ov-format`, `ov-audio`,
//! `ov-input` or `ov-store`. A backend swap this complete touching nothing behind
//! the boundary is the strongest evidence available that ADR 0001's hexagonal
//! split was worth the ceremony.

#![warn(missing_docs, clippy::all)]

pub mod catalog;
pub mod locate;
pub mod parakeet;
pub mod recordings;
mod wav;
