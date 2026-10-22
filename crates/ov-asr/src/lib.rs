//! # ov-asr — speech recognition
//!
//! Implements [`ov_core::ports::Transcriber`] with NVIDIA Parakeet TDT 0.6B v2,
//! decoding in this process through k2-fsa's `sherpa-onnx` bindings.
//!
//! The desktop adapter uses [`worker::WorkerTranscriber`], which keeps native
//! inference in a restartable instance of the same executable. Audio crosses
//! bounded binary pipes in memory; weights stay warm between requests.
//! [`sherpa::SherpaTranscriber`] is the native implementation used inside that
//! worker and by explicit command-line benchmark tools.
//!
//! [`segment`] and [`incremental`] preserve sample positions while decoding
//! completed stretches early. [`recordings`] writes audio only when requested.

#![warn(missing_docs, clippy::all)]

pub mod catalog;
mod decode;
pub mod incremental;
pub mod locate;
pub mod recordings;
pub mod segment;
pub mod sherpa;
mod wav;

pub mod worker;
