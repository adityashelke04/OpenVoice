; Installer hooks — where the speech model goes, and why it is not a resource.
;
; The 631 MB Parakeet model ships inside the installer so a fresh install can
; dictate immediately, offline, with no first-run download. The obvious way to do
; that is `bundle.resources` in tauri.conf.json. That way is wrong here.
;
; Tauri's NSIS updater downloads the whole installer on every update. With the
; model as a bundled resource, every patch release would be a ~550 MB download
; for every user — on a project shipping frequent patches, that is the difference
; between an update people accept and one they cancel.
;
; So the model is installed from here instead, into $INSTDIR\models\, and CI
; publishes a slim app-only artifact for the update channel. The model is a
; fixed, versioned asset that never changes between app releases, so excluding it
; from the app's update channel is correct in principle, not merely convenient.
;
; MODEL_SOURCE_DIR is passed by the build (see .github/workflows/release.yml).
; When it is not defined — a developer running `tauri build` without having run
; scripts/fetch-model.ps1 — the macros below do nothing and the app falls back to
; locating the model in the checkout. That is deliberate: a local packaging run
; should not fail on a 482 MB download nobody asked for.

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !ifdef MODEL_SOURCE_DIR
    ; Skip when this version's weights are already here. Re-installing or
    ; repairing should not rewrite 631 MB to produce byte-identical files.
    IfFileExists "$INSTDIR\models\parakeet-tdt-0.6b-v2\tokens.txt" model_present 0
      DetailPrint "Installing the speech model (631 MB)..."
      SetOutPath "$INSTDIR\models\parakeet-tdt-0.6b-v2"
      File "${MODEL_SOURCE_DIR}\encoder.int8.onnx"
      File "${MODEL_SOURCE_DIR}\decoder.int8.onnx"
      File "${MODEL_SOURCE_DIR}\joiner.int8.onnx"
      File "${MODEL_SOURCE_DIR}\tokens.txt"
    model_present:
  !endif
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Uninstalling must not strand 631 MB. The weights are ours, they live inside
  ; $INSTDIR, and nothing else on the machine refers to them.
  RMDir /r "$INSTDIR\models"
!macroend
