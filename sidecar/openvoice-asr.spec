# PyInstaller spec for the OpenVoice ASR sidecar.
#
# Produces `dist/openvoice-asr/` — a folder containing `openvoice-asr.exe` and
# every library it needs. A machine running the installed app has no Python, no
# virtualenv and no pip; this folder is the entire speech engine.
#
# Build with `scripts/build-sidecar.ps1`, not by invoking PyInstaller directly:
# the script pins the interpreter and checks the result actually answers on its
# protocol, which is the only proof that matters.
#
# ## onedir, not onefile
#
# A onefile build unpacks ~250 MB to a temp directory on *every* launch. The
# sidecar starts whenever the model changes or the supervisor restarts it, so
# that cost is paid repeatedly, and it leaves the extracted copy behind when the
# process is killed. onedir starts immediately and the installer compresses it
# just as well.
#
# ## CUDA is deliberately absent
#
# `nvidia-cublas-cu12` and `nvidia-cudnn-cu12` are 1.9 GB — 88% of the
# dependency tree — and are useless to anyone without an NVIDIA GPU. They are
# excluded here and fetched after install, only for machines that can use them.
# `engine.py` reads OPENVOICE_CUDA_DIR to find them; with no CUDA present it
# runs on CPU, which works everywhere.

from PyInstaller.utils.hooks import collect_all, collect_dynamic_libs

datas = []
binaries = []
hiddenimports = []

# These packages all ship native extensions, data files, or both, and all of them
# resolve imports at runtime in ways static analysis misses. `collect_all` is the
# blunt instrument, and the right one: a missing tokenizer file surfaces as a
# crash on the user's first sentence, long after the build looked successful.
for pkg in (
    "faster_whisper",  # + assets/*.onnx, the Silero VAD models
    "ctranslate2",     # the inference engine itself
    "onnxruntime",     # runs the VAD
    "tokenizers",
    "huggingface_hub",
    # The Xet download backend. huggingface_hub imports it lazily, from inside the
    # function that decides whether to use it, so static analysis never sees it and
    # a frozen build silently lacked it -- the sidecar logged "Xet Storage is
    # enabled for this repo, but the 'hf_xet' package is not installed. Falling
    # back to regular HTTP download" on every first run. The fallback works, but it
    # is slower and it resumes far less well, which matters most on exactly the
    # transfer this app cannot afford to lose: the first one.
    "hf_xet",
):
    pkg_datas, pkg_binaries, pkg_hidden = collect_all(pkg)
    datas += pkg_datas
    binaries += pkg_binaries
    hiddenimports += pkg_hidden

# numpy is large and PyInstaller's own hook handles its imports correctly; only
# its compiled libraries need help.
binaries += collect_dynamic_libs("numpy")

a = Analysis(
    ["entry.py"],
    pathex=["."],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    runtime_hooks=[],
    excludes=[
        # The CUDA runtime. See the note above.
        "nvidia",
        # Development-only, and each drags in a tree of its own.
        "pytest",
        "_pytest",
        "setuptools",
        "pip",
        # Never imported by this sidecar. PIL arrives through a transitive
        # dependency and adds ~30 MB of image codecs to a speech engine.
        "PIL",
        "tkinter",
        "matplotlib",
        "IPython",
        # PyAV, and the FFmpeg it bundles (~65 MB). faster-whisper only reaches
        # for it inside `decode_audio`, to turn an arbitrary input file into a
        # float32 array; `engine.py` reads the WAV itself (it is the only producer
        # of these files, and always writes the same 16 kHz mono 16-bit format) and
        # hands `transcribe()` the array directly, so `decode_audio` is never
        # called and PyAV is dead weight in this build specifically.
        "av",
    ],
    noarchive=False,
    optimize=0,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="openvoice-asr",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    # UPX corrupts some CUDA and ONNX Runtime DLLs, and the failure appears at
    # load time with no useful message. The compression is not worth it.
    upx=False,
    # The protocol *is* stdin and stdout, so this must be a console subsystem
    # binary. The parent suppresses the console window with CREATE_NO_WINDOW;
    # building it windowed instead would detach the standard handles and the
    # sidecar would never receive a single request.
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    name="openvoice-asr",
)
