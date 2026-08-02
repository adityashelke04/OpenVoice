"""Frozen-binary entry point.

PyInstaller runs a *script*, not a module, so `python -m openvoice_asr` has no
equivalent here — the relative imports in ``__main__.py`` would fail if that file
were used as the entry point directly. This shim exists only to turn the module
into a script, and must stay free of logic: everything real belongs in the
package, where the tests can reach it.
"""

from __future__ import annotations

import multiprocessing
import sys
import types


def _stub_out_pyav() -> None:
    """Satisfy `import av` without the real package.

    `faster_whisper/__init__.py` does `from faster_whisper.audio import
    decode_audio` unconditionally, and that module does `import av` at module
    level -- so importing `faster_whisper` at all requires `av` to exist, even
    though `engine.py` never calls `decode_audio` (it reads WAV samples itself
    and hands `transcribe()` an array; see `Engine._load_pcm16_wav`). The real
    PyAV bundles a full FFmpeg and costs ~65 MB in a frozen build for a code path
    this sidecar does not use.

    `av.audio.resampler.AudioResampler` and `av.open` are referenced only inside
    `decode_audio`'s body, not at module scope, so Python never evaluates those
    attributes unless that function actually runs. An empty module satisfies
    every `import av` in the process from this point on, because later imports
    are served from `sys.modules` rather than re-executed.

    Installed only when the real package is absent, so running this entry point
    from a full development environment (where PyAV is genuinely installed)
    is unaffected.
    """
    if "av" in sys.modules:
        return
    try:
        import av  # noqa: F401

        return
    except ImportError:
        pass
    sys.modules["av"] = types.ModuleType("av")


_stub_out_pyav()

from openvoice_asr.__main__ import main  # noqa: E402

if __name__ == "__main__":
    # Required in a frozen build. Without it, any library that starts a worker
    # process re-executes this binary from the top and forks a second sidecar,
    # which then also loads the model. Two copies of large-v3-turbo is roughly
    # 3 GB of VRAM and a machine that stops responding.
    multiprocessing.freeze_support()
    sys.exit(main())
