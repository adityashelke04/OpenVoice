# Native build dependency patch

`sherpa-onnx-sys-1.13.7` is the crates.io package with two small patches:

- Its build dependency `zip` is upgraded from 2 to 9 with only
Stored/Deflate archive support. ZIP 9 fixes
RUSTSEC-2026-0367, RUSTSEC-2026-0368 and RUSTSEC-2026-0369. The upstream native
bindings, license and 1.13.7 native ABI are unchanged. ZIP is used
only to unpack the upstream iOS xcframework; Windows/Linux use tar.bz2 archives.
Unneeded ZIP encryption and other compression backends are not built.
- The build script's symlink target variable is named `_target`, since its value
  is used only on Unix. This preserves all IO/error handling and prevents an
  unused-variable error under the repository's strict Windows compiler flags.

The original package archive was verified against Cargo.lock before extraction:
SHA-256 `566aad07d0924a7ed897cc035cc67ad71cdfddb959717dbf445031d5d4b58d4d`.
`Cargo.toml.orig` and the dependency's own unused Cargo.lock are retained as
upstream provenance; only the workspace lockfile determines our build graph.

Remove the patch when a published sherpa-onnx-sys release uses ZIP 9 or later,
after verifying its native-library ABI and model behavior. Do not edit the FFI
bindings here or replace this patch with an advisory suppression.
