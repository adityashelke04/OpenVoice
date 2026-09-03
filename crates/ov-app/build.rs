use std::path::{Path, PathBuf};

fn main() {
    generate_installer_hooks();
    tauri_build::build();
}

/// Name of the model directory, mirroring `ov_asr::locate::MODEL_DIR_NAME`.
///
/// Duplicated rather than imported: a build script cannot depend on a crate in
/// the same workspace without a build-dependency cycle. The `installs_the_model`
/// test below fails if this ever drifts from what the app looks for.
const MODEL_DIR_NAME: &str = "parakeet-tdt-0.6b-v2";

/// The four files that make up a loadable model.
const MODEL_FILES: [&str; 4] = [
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
];

/// Write `installer-hooks.nsh` from its template, with the model paths baked in.
///
/// Baked in rather than passed as `MODEL_SOURCE_DIR`, because NSIS `!ifdef` tests
/// an NSIS define and Tauri does not turn environment variables into defines. The
/// first version of this did exactly that and produced a 9 MB installer with no
/// speech model, silently — the guard was never true, and nothing said so.
///
/// A missing model is a warning, not a panic: building the packaging path without
/// a 482 MB download is a legitimate thing to want, and `release.yml` asserts the
/// model exists before it invokes `tauri build`. The guarantee belongs there,
/// where it can be enforced without blocking a contributor.
fn generate_installer_hooks() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let template = dir.join("installer-hooks.nsh.in");
    let out = dir.join("installer-hooks.nsh");

    println!("cargo:rerun-if-changed={}", template.display());
    println!("cargo:rerun-if-env-changed=MODEL_SOURCE_DIR");

    let model = model_dir();
    if let Some(m) = &model {
        println!("cargo:rerun-if-changed={}", m.join("tokens.txt").display());
    }

    let body = match &model {
        Some(m) => {
            let files: Vec<String> = MODEL_FILES
                .iter()
                .map(|f| format!("      File \"{}\"", m.join(f).display()))
                .collect();
            format!(
                "  ; Skip when this version's weights are already here. Re-installing or\n\
                 \x20 ; repairing should not rewrite 631 MB to produce byte-identical files.\n\
                 \x20 IfFileExists \"$INSTDIR\\models\\{MODEL_DIR_NAME}\\tokens.txt\" model_present 0\n\
                 \x20   DetailPrint \"Installing the speech model (631 MB)...\"\n\
                 \x20   SetOutPath \"$INSTDIR\\models\\{MODEL_DIR_NAME}\"\n\
                 {}\n\
                 \x20 model_present:",
                files.join("\n")
            )
        }
        None => {
            println!(
                "cargo:warning=no speech model found; the installer will ship without one. \
                 Run scripts/fetch-model.ps1, or set MODEL_SOURCE_DIR."
            );
            "  ; No model was present at build time.".to_string()
        }
    };

    let rendered = std::fs::read_to_string(&template)
        .unwrap_or_else(|e| panic!("reading {}: {e}", template.display()))
        .replace("@MODEL_FILES@", &body);

    // Only write when the content actually changes: an unconditional write
    // updates the mtime every build, and `rerun-if-changed` on a file this script
    // rewrites would rebuild forever.
    if std::fs::read_to_string(&out).ok().as_deref() != Some(rendered.as_str()) {
        std::fs::write(&out, rendered)
            .unwrap_or_else(|e| panic!("writing {}: {e}", out.display()));
    }
}

/// The model directory, from `MODEL_SOURCE_DIR` or the checkout, if it is complete.
fn model_dir() -> Option<PathBuf> {
    let candidate = match std::env::var_os("MODEL_SOURCE_DIR") {
        Some(d) => PathBuf::from(d),
        // The workspace root by walking up, not by joining "../..": NSIS is given
        // these paths verbatim, and a `C:\../..\models` with mixed
        // separators is not something to hand a 1990s installer compiler.
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/ov-app is two levels below the workspace root")
            .join("models")
            .join(MODEL_DIR_NAME),
    };
    MODEL_FILES
        .iter()
        .all(|f| candidate.join(f).exists())
        .then_some(candidate)
}
