"""faster-whisper wrapper.

Isolated from the protocol layer so the transport can be swapped (or the whole
sidecar replaced by an in-process whisper.cpp backend, per ADR 0003) without
touching inference code.
"""

from __future__ import annotations

import contextlib
import io
import os
import sys
import sysconfig
import time
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .protocol import log

# Names ctranslate2 needs at decode time. If these cannot be loaded, CUDA is not
# actually usable no matter what the device count says.
_REQUIRED_CUDA_DLLS = ("cublas64_12.dll", "cudnn64_9.dll")


def register_cuda_dll_dirs() -> list[str]:
    """Make pip-installed CUDA libraries findable on Windows.

    ``nvidia-cublas-cu12`` and ``nvidia-cudnn-cu12`` drop their DLLs into
    ``site-packages/nvidia/<pkg>/bin``, which is not on the DLL search path. Without
    this, ctranslate2 reports a CUDA device, loads the model onto the GPU, and then
    fails at the *first decode* with ``Library cublas64_12.dll is not found``.

    That timing is the problem: everything looks healthy right up until the user's
    first sentence. Registering the directories up front turns a confusing runtime
    failure into a condition we can detect and route around.

    Returns the directories that were added.
    """
    if sys.platform != "win32":
        return []

    added: list[str] = []
    for scheme in ("purelib", "platlib"):
        root = Path(sysconfig.get_paths()[scheme]) / "nvidia"
        if not root.is_dir():
            continue
        for sub in sorted(root.glob("*/bin")) + sorted(root.glob("*/lib")):
            if not sub.is_dir() or str(sub) in added:
                continue
            with contextlib.suppress(OSError):
                os.add_dll_directory(str(sub))
                added.append(str(sub))
    if added:
        # Also extend PATH: some loaders consult it rather than the directory list.
        os.environ["PATH"] = os.pathsep.join(added) + os.pathsep + os.environ.get("PATH", "")
    return added


def find_cuda_dlls() -> dict[str, str | None]:
    """Locate each required CUDA DLL, or report it missing."""
    found: dict[str, str | None] = {}
    search = [Path(p) for p in os.environ.get("PATH", "").split(os.pathsep) if p]
    for name in _REQUIRED_CUDA_DLLS:
        found[name] = next((str(d / name) for d in search if (d / name).is_file()), None)
    return found


# Registered at import so it is in place before ctranslate2 is ever loaded.
_CUDA_DLL_DIRS = register_cuda_dll_dirs()


def enforce_offline_by_default() -> bool:
    """Stop huggingface_hub from touching the network on every model load.

    Measured on the reference machine: loading an already-cached ``base.en`` took
    **171 seconds**, reproducibly, to the millisecond. That precision was the tell --
    compute does not repeat like that, but a network timeout does. ``huggingface_hub``
    was revalidating a model it already had on disk and blocking on a ~171 s timeout
    before falling back to the cache. With ``HF_HUB_OFFLINE=1`` the same load takes
    **1.3 seconds**.

    So this is not merely an optimisation. A dictation tool that takes three minutes
    to become ready, and does so for a reason invisible to the user, is broken.

    It also happens to be exactly what the architecture already required: the sidecar
    is inference only, and downloading models is the model manager's job, guarded by
    a checksum manifest. Set ``OPENVOICE_ALLOW_DOWNLOAD=1`` to permit fetching.

    Returns whether offline mode is in force.
    """
    if os.environ.get("OPENVOICE_ALLOW_DOWNLOAD") == "1":
        os.environ.pop("HF_HUB_OFFLINE", None)
        return False
    os.environ.setdefault("HF_HUB_OFFLINE", "1")
    os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
    os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
    return True


_OFFLINE = enforce_offline_by_default()

# A load slower than this means something is wrong -- almost always the network.
# Warn rather than fail: a slow load is still a working app.
SLOW_LOAD_WARN_MS = 15_000

# Decoding beam width. Overridable with OPENVOICE_BEAM_SIZE for experiments.
BEAM_SIZE = int(os.environ.get("OPENVOICE_BEAM_SIZE", "5"))

# Rejection thresholds for hallucinated output. Whisper does not fail silently on
# noise -- it produces confident, well-formed words -- so these are the difference
# between a dictation tool and a random word generator.
#
# A segment is discarded if the model's own "this is not speech" probability is
# above NO_SPEECH_MAX, or if its mean token log-probability is below
# MIN_AVG_LOGPROB. An entire utterance is discarded if the voice-activity detector
# found less than MIN_SPEECH_SECONDS of actual speech in it.
NO_SPEECH_MAX = float(os.environ.get("OPENVOICE_NO_SPEECH_MAX", "0.6"))
MIN_AVG_LOGPROB = float(os.environ.get("OPENVOICE_MIN_AVG_LOGPROB", "-1.0"))
MIN_SPEECH_SECONDS = float(os.environ.get("OPENVOICE_MIN_SPEECH_S", "0.35"))

# Reference machine: RTX 3050 Laptop, 4 GB VRAM. These presets are chosen so the
# default fits comfortably alongside a desktop compositor and a browser, which is
# the realistic condition this app runs under -- not a benchmark machine with an
# idle GPU.
# compute_type is float16 rather than int8_float16 on CUDA, from measurement on the
# reference GPU: float16 decoded a median 623 ms against int8_float16's 661 ms and
# loaded in half the time (3.7 s vs 7.2 s), with byte-identical transcripts. Int8
# weights have to be dequantized every forward pass, and on a GPU that costs more
# than the memory bandwidth it saves. `fallback_compute` is used if the larger
# weights will not fit -- a 4 GB laptop GPU also has a desktop compositor and a
# browser on it.
MODEL_PRESETS: dict[str, dict[str, Any]] = {
    "base.en": {"repo": "base.en", "compute_type": "int8", "vram_mb": 0},
    "small.en": {
        "repo": "small.en",
        "compute_type": "float16",
        "fallback_compute": "int8_float16",
        "vram_mb": 600,
    },
    "large-v3-turbo": {
        "repo": "deepdml/faster-whisper-large-v3-turbo-ct2",
        "compute_type": "float16",
        "fallback_compute": "int8_float16",
        "vram_mb": 1600,
    },
}

# Whisper's initial prompt is capped at 224 tokens. Overrunning it does not error --
# the decoder silently truncates, dropping whichever terms happened to land last.
# Budget conservatively in characters so the truncation never surprises us.
MAX_HINT_CHARS = 600


@contextlib.contextmanager
def guard_stdout() -> Iterator[None]:
    """Redirect stdout to stderr for the duration of the block.

    Model loading pulls in libraries that occasionally print progress to stdout.
    On this protocol that is not a cosmetic problem: it corrupts the stream and
    produces a JSON parse error in the host that points nowhere near the cause.
    """
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        yield
    captured = buf.getvalue().strip()
    if captured:
        log(f"suppressed stdout during load: {captured[:500]}")


@dataclass
class Transcription:
    """Result of one decode."""

    text: str
    language: str | None
    confidence: float | None
    decode_ms: int


class Engine:
    """Holds a loaded model and transcribes audio."""

    def __init__(self, model: str = "base.en", device: str = "auto") -> None:
        if model not in MODEL_PRESETS:
            raise ValueError(f"unknown model {model!r}; known: {', '.join(sorted(MODEL_PRESETS))}")
        self.model_name = model
        self.preset = MODEL_PRESETS[model]
        self.device = device
        self._model: Any = None
        self._resolved_device: str | None = None

    # -- lifecycle ---------------------------------------------------------

    def load(self) -> None:
        """Load weights. Idempotent, so ``warm`` can be sent freely."""
        if self._model is not None:
            return

        from faster_whisper import WhisperModel  # imported late: ~2 s of import cost

        device, compute_type = self._choose_device()
        started = time.perf_counter()
        root = os.environ.get("OPENVOICE_MODEL_DIR") or None

        def build(ct: str) -> Any:
            with guard_stdout():
                return WhisperModel(
                    self.preset["repo"], device=device, compute_type=ct, download_root=root
                )

        try:
            self._model = build(compute_type)
        except Exception as exc:  # noqa: BLE001
            fallback = self.preset.get("fallback_compute")
            if not fallback or fallback == compute_type:
                raise
            # Almost always out of VRAM. Smaller weights beat no dictation.
            log(f"{compute_type} failed ({str(exc)[:120]}); retrying with {fallback}")
            compute_type = fallback
            self._model = build(compute_type)
        self._resolved_device = device
        took = int((time.perf_counter() - started) * 1000)
        log(f"loaded {self.model_name} on {device}/{compute_type} in {took}ms")
        if took > SLOW_LOAD_WARN_MS:
            # Read the environment rather than the import-time flag: --allow-download
            # flips this after import, and reporting the stale value tells the reader
            # the exact opposite of what happened.
            offline = os.environ.get("HF_HUB_OFFLINE") == "1"
            log(
                f"load took {took}ms, which is far above the ~1300ms expected from a "
                "warm cache. Expected while downloading weights; otherwise the usual "
                f"cause is huggingface_hub reaching the network (offline mode is "
                f"{'on' if offline else 'OFF'})."
            )

    def _choose_device(self) -> tuple[str, str]:
        """Pick CUDA when it is genuinely usable, else fall back to CPU.

        A missing cuDNN is the common failure here, and it surfaces as an opaque
        DLL load error at the first decode rather than at load time. Failing over
        to CPU with a clear log line beats a working-looking app that errors on the
        user's first sentence.
        """
        want = self.device
        if want == "cpu":
            return "cpu", "int8"

        try:
            import ctranslate2

            if ctranslate2.get_cuda_device_count() > 0:
                # A visible device is not sufficient. ctranslate2 reports the GPU
                # from the driver, but decoding needs cuBLAS and cuDNN to actually
                # load -- and when they are pip-installed on Windows they often
                # cannot be found. Check before committing to CUDA, so the failure
                # lands here with a clear message instead of mid-sentence.
                missing = [n for n, p in find_cuda_dlls().items() if p is None]
                if missing:
                    log(
                        f"CUDA device present but {', '.join(missing)} not loadable; "
                        "using CPU. Install nvidia-cublas-cu12 and nvidia-cudnn-cu12."
                    )
                else:
                    return "cuda", self.preset["compute_type"]
            else:
                log("no CUDA device visible to ctranslate2; using CPU")
        except Exception as exc:  # noqa: BLE001 - any failure means "no CUDA"
            log(f"CUDA probe failed ({exc}); using CPU")

        if want == "cuda":
            log("CUDA was requested but is unavailable; falling back to CPU")
        return "cpu", "int8"

    @property
    def device_in_use(self) -> str:
        """Device actually selected, known only after :meth:`load`."""
        return self._resolved_device or "unloaded"

    @property
    def model_id(self) -> str:
        """Identifier recorded in history alongside every transcript."""
        return f"faster-whisper/{self.model_name}@{self.device_in_use}"

    # -- inference ---------------------------------------------------------

    def transcribe(
        self,
        wav_path: str,
        vocabulary: list[str] | None = None,
        language: str | None = "en",
    ) -> Transcription:
        """Transcribe a 16 kHz mono WAV file."""
        self.load()
        assert self._model is not None

        try:
            return self._decode(wav_path, vocabulary, language)
        except RuntimeError as exc:
            # A CUDA library that fails to load surfaces here, at the first decode.
            # "Never lose a word" means degrading to CPU and answering rather than
            # handing the user an error for audio they already spoke.
            if not _is_cuda_library_error(exc) or self._resolved_device == "cpu":
                raise
            log(f"CUDA decode failed ({exc}); reloading on CPU and retrying once")
            self._model = None
            self.device = "cpu"
            self.load()
            return self._decode(wav_path, vocabulary, language)

    def _decode(
        self,
        wav_path: str,
        vocabulary: list[str] | None,
        language: str | None,
    ) -> Transcription:
        assert self._model is not None
        started = time.perf_counter()
        segments, info = self._model.transcribe(
            wav_path,
            language=language,
            # Beam search, not greedy. beam_size=1 was chosen for latency and it
            # was a bad trade: greedy decoding falls into repetition loops, which
            # is where `?????????????`, `kkkkkkkkkkkk` and `...............` come
            # from. A beam keeps alternative hypotheses alive so a degenerate one
            # loses, and it costs a few hundred milliseconds. Correct output is
            # worth more than the latency.
            beam_size=BEAM_SIZE,
            # Direct suppression of loops, belt to the beam's braces.
            repetition_penalty=1.15,
            no_repeat_ngram_size=3,
            # Whisper hallucinates confidently on silence and noise. These three
            # are the model's own escape hatches and are worth setting explicitly
            # rather than inheriting: a segment that is too repetitive (high
            # compression ratio), too improbable, or too likely to be silence gets
            # retried at a higher temperature and then dropped.
            temperature=[0.0, 0.2, 0.4, 0.6, 0.8, 1.0],
            compression_ratio_threshold=2.4,
            log_prob_threshold=-1.0,
            no_speech_threshold=0.6,
            # Trim silence before decoding. Push-to-talk always captures a moment
            # of room tone at each end, and room tone is exactly what Whisper
            # invents words for.
            vad_filter=True,
            vad_parameters={
                "min_silence_duration_ms": 300,
                "speech_pad_ms": 200,
            },
            initial_prompt=build_hint(vocabulary),
            condition_on_previous_text=False,
            # Each utterance is independent. Conditioning on previous text makes
            # Whisper echo earlier dictation into later transcripts, which is
            # baffling to the user and actively harmful for one-shot commands.
        )

        parts: list[str] = []
        logprobs: list[float] = []
        dropped = 0
        for seg in segments:  # generator: this is where decoding actually happens
            # Per-segment rejection. Whisper will confidently transcribe room tone
            # -- a 2 s capture that was 74% silence produced the word "Jordan" --
            # and a hallucinated segment carries the evidence of its own
            # unreliability in these two numbers. Dropping them here is far better
            # than letting them reach the user's editor.
            no_speech = getattr(seg, "no_speech_prob", 0.0) or 0.0
            avg_lp = seg.avg_logprob if seg.avg_logprob is not None else 0.0
            if no_speech > NO_SPEECH_MAX or avg_lp < MIN_AVG_LOGPROB:
                dropped += 1
                log(
                    f"dropped segment (no_speech={no_speech:.2f}, "
                    f"avg_logprob={avg_lp:.2f}): {seg.text.strip()[:60]!r}"
                )
                continue
            parts.append(seg.text)
            logprobs.append(avg_lp)

        text = "".join(parts).strip()

        # Whole-utterance rejection. `duration_after_vad` is how much audio the
        # voice-activity detector actually considered speech; when that is a
        # fraction of a second, anything the model produced came from noise.
        speech_s = getattr(info, "duration_after_vad", None)
        if speech_s is not None and speech_s < MIN_SPEECH_SECONDS:
            if text:
                log(f"discarding {text[:60]!r}: only {speech_s:.2f}s of speech detected")
            text = ""

        if dropped:
            log(f"dropped {dropped} low-confidence segment(s)")

        decode_ms = int((time.perf_counter() - started) * 1000)
        return Transcription(
            text=text,
            language=getattr(info, "language", language),
            confidence=(sum(logprobs) / len(logprobs)) if logprobs else None,
            decode_ms=decode_ms,
        )


def _is_cuda_library_error(exc: BaseException) -> bool:
    """Whether an exception is a CUDA library that failed to load."""
    text = str(exc).lower()
    return "library" in text and any(k in text for k in ("cublas", "cudnn", "cuda"))


def build_hint(vocabulary: list[str] | None) -> str | None:
    """Pack vocabulary terms into an initial prompt, within budget.

    Biasing the decoder is strictly better than repairing its output afterwards:
    at decode time the model still has the acoustic evidence that post-processing
    has already discarded. This is why the dictionary is fed in here as well as
    being applied by ``ov-format``.
    """
    if not vocabulary:
        return None
    kept: list[str] = []
    used = 0
    for term in vocabulary:
        cost = len(term) + 2
        if used + cost > MAX_HINT_CHARS:
            break
        kept.append(term)
        used += cost
    if not kept:
        return None
    return "Technical vocabulary: " + ", ".join(kept) + "."


def probe() -> dict[str, Any]:
    """Report the environment, for the settings UI and for bug reports."""
    dlls = find_cuda_dlls()
    info: dict[str, Any] = {
        "python": sys.version.split()[0],
        "models": sorted(MODEL_PRESETS),
        "cuda_dll_dirs": _CUDA_DLL_DIRS,
        "cuda_dlls_missing": [n for n, p in dlls.items() if p is None],
    }
    try:
        import ctranslate2

        info["ctranslate2"] = ctranslate2.__version__
        info["cuda_devices"] = ctranslate2.get_cuda_device_count()
    except Exception as exc:  # noqa: BLE001
        info["ctranslate2"] = f"unavailable: {exc}"
        info["cuda_devices"] = 0
    try:
        import faster_whisper

        info["faster_whisper"] = faster_whisper.__version__
    except Exception as exc:  # noqa: BLE001
        info["faster_whisper"] = f"unavailable: {exc}"
    return info
