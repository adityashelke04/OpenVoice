//! Fetching speech models, and nothing else.
//!
//! # Why this is its own crate
//!
//! `scripts/check-no-network.sh` proves that every crate touching the
//! microphone, the transcript, the keyboard or the history database has no path
//! to an HTTP client, a TLS stack or a socket — anywhere in its dependency
//! graph. `ov-asr` is in that sealed set, and it is the crate holding the
//! microphone, so it is the last one that should gain the ability to phone home.
//!
//! Downloading a model needs a socket. Putting that in `ov-asr` would trade a
//! tested guarantee for a convenience. Putting it here keeps the guarantee and
//! makes "which code can reach the internet" a question you answer by reading
//! one `Cargo.toml`. See ADR 0009.
//!
//! # What it guarantees
//!
//! * **Nothing is trusted before it is verified.** The archive is checked
//!   against a SHA-256 pinned in `ov_asr::catalog` before a single byte is
//!   extracted. These files become weights the app executes.
//! * **A failure leaves no trace.** Extraction happens in a staging directory
//!   and is moved into place only once every expected file is present, so an
//!   interrupted or corrupt download cannot leave a partial model that
//!   `ov_asr::locate::is_installed` would later report as ready.

#![warn(missing_docs, clippy::all)]

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// How many bytes to read from the socket at a time.
///
/// A megabyte: large enough that progress callbacks are not the bottleneck on a
/// fast link, small enough that the bar still moves on a slow one.
const CHUNK: usize = 1 << 20;

/// Check `bytes` against a lowercase hex SHA-256.
fn verify(bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "checksum mismatch: the download does not match what this build expects.\n  \
             expected {expected}\n  actual   {actual}"
        ))
    }
}

/// Download `url`, verify it against `sha256`, and extract `files` into `dest`.
///
/// `on_progress(downloaded, total)` is called as the transfer advances. `total`
/// is 0 when the server sends no `Content-Length`; callers should show an
/// indeterminate bar rather than a percentage of nothing.
///
/// # Errors
///
/// Returns a message fit to show a user on any of: the request failing, the
/// checksum not matching, the archive not containing an expected file, or the
/// extracted directory failing to move into place.
pub fn download_and_extract(
    url: &str,
    sha256: &str,
    files: &[&str],
    dest: &Path,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    tracing::info!(url, "downloading model");

    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("could not reach {url}: {e}"))?;
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // Held in memory rather than streamed to a file: the checksum has to cover
    // the whole archive before anything is written, and the largest of these is
    // 465 MB against the ~750 MB the model itself occupies once loaded.
    let mut buf: Vec<u8> = Vec::with_capacity(total as usize);
    let mut reader = resp.into_reader();
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("the download was interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        on_progress(buf.len() as u64, total);
    }

    verify(&buf, sha256)?;
    tracing::info!(bytes = buf.len(), "checksum ok");

    let staging = dest.with_extension("partial");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("could not create {}: {e}", staging.display()))?;

    let extracted = extract(&buf, files, &staging);
    if let Err(e) = extracted {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    // Only now does anything appear where the app will look for it.
    let _ = std::fs::remove_dir_all(dest);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::rename(&staging, dest)
        .map_err(|e| format!("could not install to {}: {e}", dest.display()))?;

    tracing::info!(dest = %dest.display(), "model installed");
    Ok(())
}

/// Unpack the wanted files from a bzip2 tar archive into `staging`.
fn extract(archive: &[u8], files: &[&str], staging: &Path) -> Result<(), String> {
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(archive));
    for entry in tar
        .entries()
        .map_err(|e| format!("the archive could not be read: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("the archive could not be read: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("the archive has an unreadable path: {e}"))?
            .into_owned();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Only the files the catalogue asks for, matched on the bare filename.
        //
        // Two reasons. Whisper ships fp32 and int8 weights side by side, and
        // keeping both would waste 146 MB of someone's disk for files that are
        // never loaded. And matching the leaf name rather than joining the
        // archive's own path is what stops a crafted entry like `../../foo`
        // writing outside the staging directory.
        if !files.contains(&name) {
            continue;
        }
        entry
            .unpack(staging.join(name))
            .map_err(|e| format!("could not extract {name}: {e}"))?;
    }

    for f in files {
        if !staging.join(f).is_file() {
            return Err(format!(
                "the download did not contain {f}. The model archive may have changed upstream."
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_checksum_is_accepted() {
        // SHA-256 of the empty string, so this expectation is checkable by hand
        // rather than by running the code it is testing.
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(verify(b"", empty).is_ok());
    }

    #[test]
    fn a_mismatched_checksum_is_refused_and_says_both_values() {
        let err = verify(b"not the bytes you wanted", &"0".repeat(64)).expect_err("must reject");
        assert!(err.contains("checksum mismatch"), "{err}");
        // Both halves, so whoever reads the message can tell a corrupted
        // download from a catalogue entry that was pinned wrong.
        assert!(err.contains("expected"), "{err}");
        assert!(err.contains("actual"), "{err}");
    }

    #[test]
    fn extraction_keeps_only_the_wanted_files() {
        let staging = std::env::temp_dir().join("ov-fetch-subset");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).unwrap();

        let archive = tar_bz2(&[("model/wanted.onnx", b"a"), ("model/unwanted.onnx", b"b")]);
        extract(&archive, &["wanted.onnx"], &staging).expect("extract");

        assert!(staging.join("wanted.onnx").is_file());
        assert!(
            !staging.join("unwanted.onnx").exists(),
            "a file the catalogue did not ask for must not be kept"
        );
        let _ = std::fs::remove_dir_all(&staging);
    }

    #[test]
    fn a_missing_expected_file_is_an_error() {
        let staging = std::env::temp_dir().join("ov-fetch-missing");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).unwrap();

        let archive = tar_bz2(&[("model/present.onnx", b"a")]);
        let err = extract(&archive, &["present.onnx", "absent.onnx"], &staging)
            .expect_err("must report the missing file");
        assert!(err.contains("absent.onnx"), "{err}");
        let _ = std::fs::remove_dir_all(&staging);
    }

    #[test]
    fn an_entrys_directories_are_discarded_and_only_the_leaf_is_written() {
        // This is the mechanism that makes extraction safe: the archive's own
        // path is never joined onto the destination, only its final component.
        // A crafted entry therefore has nowhere to point.
        //
        // The obvious test — an entry literally named `../escaped.onnx` — cannot
        // be written here, because the `tar` crate refuses to *build* one
        // ("paths in archives must not have `..`"). So this asserts the property
        // that does the protecting, on the deepest path the builder allows.
        let staging = std::env::temp_dir().join("ov-fetch-leaf");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).unwrap();

        let archive = tar_bz2(&[("a/b/c/d/wanted.onnx", b"x")]);
        extract(&archive, &["wanted.onnx"], &staging).expect("extract");

        assert!(
            staging.join("wanted.onnx").is_file(),
            "must land directly in staging, flattened"
        );
        assert!(
            !staging.join("a").exists(),
            "the archive's directory structure must not be recreated"
        );
        let _ = std::fs::remove_dir_all(&staging);
    }

    /// Build a bzip2-compressed tar in memory, for the extraction tests.
    fn tar_bz2(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (name, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, name, *body).unwrap();
        }
        let raw = tar.into_inner().unwrap();

        use std::io::Write;
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        enc.write_all(&raw).unwrap();
        enc.finish().unwrap()
    }
}
