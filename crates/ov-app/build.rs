use std::path::Path;

fn main() {
    ensure_sidecar_resource_present();
    tauri_build::build();
}

/// `tauri_build::build()` validates every path in `tauri.conf.json`'s
/// `bundle.resources` against the filesystem *unconditionally* — even for a
/// plain `cargo check` / `clippy` / `test`, which never bundle anything. It is
/// not gated on `tauri build` or on release profile; `tauri-utils`' resource
/// walker hard-errors ("resource path ... doesn't exist") the moment the
/// configured path is missing.
///
/// `sidecar/dist/openvoice-asr/` is the frozen ASR engine produced by
/// `scripts/build-sidecar.ps1` (see ADR 0003) and is deliberately gitignored —
/// it is ~200 MB of PyInstaller output that must never be committed. That
/// means a fresh checkout (a new contributor's machine, or CI's `rust` job,
/// which runs `cargo fmt`/`clippy`/`test` with no sidecar-freezing step at
/// all) can never satisfy this check without first standing up a Python
/// environment and running PyInstaller — something `cargo test` has no
/// business requiring.
///
/// Fix: an empty directory satisfies tauri-build's resource walker (it walks
/// and silently skips directories with nothing in them), so create one when
/// missing rather than requiring the real frozen binary. This is safe for
/// ordinary dev builds. For a genuine release-profile build (what `tauri
/// build` runs by default) we still hard-fail if the frozen executable itself
/// is absent, so `cargo tauri build --release` cannot silently produce an
/// installer with no speech engine — the exact failure mode called out in
/// the CHANGELOG. `release.yml` freezes the real sidecar and asserts the
/// binary exists before ever invoking `tauri build`, so this is
/// defense-in-depth for anyone bundling outside that workflow, not a
/// replacement for it.
fn ensure_sidecar_resource_present() {
    let dist_dir = Path::new("../../sidecar/dist/openvoice-asr");
    let frozen_exe = dist_dir.join("openvoice-asr.exe");

    if frozen_exe.exists() {
        return;
    }

    let is_release = std::env::var("PROFILE").as_deref() == Ok("release");
    if is_release {
        panic!(
            "no frozen sidecar at {} — run `pwsh scripts/build-sidecar.ps1` before a release \
             build, or the installer ships with no speech engine",
            frozen_exe.display()
        );
    }

    if !dist_dir.exists() {
        std::fs::create_dir_all(dist_dir).unwrap_or_else(|e| {
            panic!(
                "failed to create placeholder sidecar resource dir {}: {e}",
                dist_dir.display()
            )
        });
        println!(
            "cargo:warning=sidecar/dist/openvoice-asr/ is empty (dev build placeholder); \
             run scripts/build-sidecar.ps1 before packaging a real installer"
        );
    }
}
