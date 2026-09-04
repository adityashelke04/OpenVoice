//! End-to-end: fetch a catalogued model from its real URL and decode with it.
//!
//! `#[ignore]` by default, and deliberately so. It downloads 112 MB, which is
//! not something to do on every `cargo test`, on a contributor's tethered
//! connection, or in CI on every push. But it is the only test that exercises
//! the whole chain a user actually walks — catalogue entry, real URL, real
//! checksum, extraction, and a model that then loads and transcribes.
//!
//! Run it deliberately:
//!
//! ```text
//! cargo test -p ov-asr --test download_and_load -- --ignored --nocapture
//! ```
//!
//! Whisper tiny.en is the subject because it is the smallest catalogue entry and
//! the only one of a different `ModelKind` from the bundled default — so it
//! covers the loader branch that the unit tests can only reach with a fake
//! directory.

use std::path::PathBuf;

use ov_core::ports::{DecodeHint, Pcm16k, Transcriber};

fn scratch() -> PathBuf {
    std::env::temp_dir().join("ov-asr-download-test")
}

#[test]
#[ignore = "downloads 112 MB from GitHub"]
fn a_catalogued_model_downloads_verifies_extracts_and_transcribes() {
    let spec = ov_asr::catalog::find("whisper-tiny.en").expect("in the catalogue");
    let root = scratch();
    let _ = std::fs::remove_dir_all(&root);

    // 1. It is not installed to begin with.
    assert!(
        !ov_asr::locate::is_installed(spec, &root),
        "a clean directory must not report the model as installed"
    );

    // 2. Fetch it, exactly as the Models screen does.
    let mut last = 0u64;
    ov_fetch::download_and_extract(
        &spec.url(),
        spec.sha256,
        spec.files,
        &root.join(spec.id),
        &mut |done, total| {
            if done - last > 20_000_000 {
                eprintln!("  {} MB of {} MB", done / 1_000_000, total / 1_000_000);
                last = done;
            }
        },
    )
    .expect("download, checksum and extract");

    // 3. It is installed now, and only the files the catalogue asked for exist.
    assert!(ov_asr::locate::is_installed(spec, &root));
    let kept: Vec<String> = std::fs::read_dir(root.join(spec.id))
        .expect("read the model directory")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        kept.len(),
        spec.files.len(),
        "the archive's fp32 weights and test wavs must not have been kept: {kept:?}"
    );

    // 4. It loads and transcribes. This is the assertion that matters: a model
    //    that downloads perfectly and cannot be loaded is not a working feature.
    let dir = ov_asr::locate::model_dir(spec, &root).expect("resolve the downloaded model");
    let t = ov_asr::sherpa::SherpaTranscriber::new(spec, dir).expect("load the downloaded model");
    assert_eq!(t.model_id(), "whisper-tiny.en");

    let wav = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../models")
        .join(ov_asr::catalog::DEFAULT_MODEL)
        .join("test.wav");
    if !wav.is_file() {
        eprintln!("skipping the decode: no test.wav; run scripts/fetch-model.ps1");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    let mut r = hound::WavReader::open(&wav).expect("open the fixture");
    let audio = Pcm16k {
        samples: r
            .samples::<i16>()
            .map(|s| f32::from(s.expect("sample")) / 32768.0)
            .collect(),
    };

    let out = t.transcribe(&audio, &DecodeHint::default()).expect("decode");
    assert!(
        out.text.to_lowercase().contains("portrait"),
        "expected the spoken words, got {:?}",
        out.text
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "downloads 112 MB from GitHub"]
fn a_wrong_checksum_leaves_nothing_installed() {
    // The failure path matters more than the happy one. A rejected download must
    // not leave a directory that `is_installed` would later call ready.
    let spec = ov_asr::catalog::find("whisper-tiny.en").expect("in the catalogue");
    let root = scratch().join("badsum");
    let _ = std::fs::remove_dir_all(&root);

    let err = ov_fetch::download_and_extract(
        &spec.url(),
        &"0".repeat(64),
        spec.files,
        &root.join(spec.id),
        &mut |_, _| {},
    )
    .expect_err("a wrong checksum must be refused");
    assert!(err.contains("checksum mismatch"), "{err}");

    assert!(
        !ov_asr::locate::is_installed(spec, &root),
        "a refused download must leave nothing behind"
    );
    let _ = std::fs::remove_dir_all(&root);
}
