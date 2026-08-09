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
import wave
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np

from .protocol import log

# Names ctranslate2 needs at decode time. If these cannot be loaded, CUDA is not
# actually usable no matter what the device count says.
_REQUIRED_CUDA_DLLS = ("cublas64_12.dll", "cudnn64_9.dll")


def _cuda_roots() -> list[Path]:
    """Directories that may contain an ``nvidia/<pkg>/bin`` layout.

    Two ways the CUDA libraries reach a machine, and the sidecar has to work with
    either:

    * **From pip**, in a developer's virtualenv, under ``site-packages/nvidia``.
    * **Downloaded after install**, into a directory the app owns and names in
      ``OPENVOICE_CUDA_DIR``. The frozen build excludes these libraries because
      they are 1.9 GB and useless without an NVIDIA GPU, so this is the only
      route for an installed copy.

    A frozen binary has no meaningful ``sysconfig`` paths, and a developer has no
    ``OPENVOICE_CUDA_DIR``; checking both costs two stat calls and removes an
    entire class of "works on my machine".
    """
    roots: list[Path] = []
    if override := os.environ.get("OPENVOICE_CUDA_DIR"):
        roots.append(Path(override))
    if not getattr(sys, "frozen", False):
        roots += [Path(sysconfig.get_paths()[s]) / "nvidia" for s in ("purelib", "platlib")]
    return roots


def register_cuda_dll_dirs() -> list[str]:
    """Make the CUDA libraries findable on Windows.

    ``nvidia-cublas-cu12`` and ``nvidia-cudnn-cu12`` keep their DLLs in
    ``<root>/<pkg>/bin``, which is not on the DLL search path. Without this,
    ctranslate2 reports a CUDA device, loads the model onto the GPU, and then
    fails at the *first decode* with ``Library cublas64_12.dll is not found``.

    That timing is the problem: everything looks healthy right up until the user's
    first sentence. Registering the directories up front turns a confusing runtime
    failure into a condition we can detect and route around.

    Returns the directories that were added.
    """
    if sys.platform != "win32":
        return []

    added: list[str] = []
    for root in _cuda_roots():
        if not root.is_dir():
            continue
        # The download may be unpacked either as `nvidia/<pkg>/bin` or flattened
        # to `<pkg>/bin`, depending on how the archive was made. Glob both rather
        # than depend on the packaging staying still.
        subs = sorted(root.glob("*/bin")) + sorted(root.glob("*/lib"))
        subs += sorted(root.glob("*/*/bin")) + sorted(root.glob("*/*/lib"))
        for sub in subs:
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
# The catalogue of models lives in `crates/ov-asr/src/catalog.rs`, not here.
#
# It used to live in both places, and the two could disagree without either being
# wrong on its own terms: a model added on one side was invisible to the other. The
# sidecar is now *told* what to load -- repository, compute type, and a fallback --
# so there is exactly one file to edit when a model is added, and it is the one that
# also drives the picker in the UI.
#
# These defaults exist only so `python -m openvoice_asr` is runnable by hand for
# debugging. The app always passes all three explicitly.
DEFAULT_MODEL = "base.en"
DEFAULT_REPO = "Systran/faster-whisper-base.en"
DEFAULT_COMPUTE = "int8"

# Whisper's initial prompt is capped at 224 tokens. Overrunning it does not error --
# the decoder silently truncates, dropping whichever terms happened to land last.
# Budget conservatively in characters so the truncation never surprises us.
MAX_HINT_CHARS = 600

# The files a CTranslate2 Whisper repository actually needs. Downloading the whole
# repository would also pull the PyTorch weights, which this engine never reads and
# which roughly double the transfer.
MODEL_FILES = (
    "config.json",
    "preprocessor_config.json",
    "model.bin",
    "tokenizer.json",
    "vocabulary.*",
)


@contextlib.contextmanager
def online() -> Iterator[None]:
    """Lift offline mode for the duration of a deliberate download.

    The sidecar runs with ``HF_HUB_OFFLINE=1`` because revalidating a cached model
    costs ~171 s per load (see :func:`enforce_offline_by_default`). Fetching weights
    is the one operation that genuinely needs the network, and it is only ever
    reached because the user asked for a model they do not have.

    Restored in a ``finally`` so a failed or cancelled download cannot leave the
    process online — that would silently reintroduce the 171 s load on every
    subsequent start, with nothing to connect it back to the failure.
    """
    previous = os.environ.get("HF_HUB_OFFLINE")
    os.environ.pop("HF_HUB_OFFLINE", None)
    try:
        yield
    finally:
        if previous is not None:
            os.environ["HF_HUB_OFFLINE"] = previous


def model_is_cached(repo: str) -> bool:
    """Whether `repo` can be loaded without touching the network.

    Asked before every start. A wrong ``True`` here strands the user on a spinner
    while huggingface_hub blocks on a download nobody agreed to, so this resolves
    the snapshot offline rather than guessing from directory names.
    """
    from huggingface_hub import snapshot_download

    try:
        snapshot_download(
            repo,
            allow_patterns=list(MODEL_FILES),
            local_files_only=True,
        )
    except Exception:  # noqa: BLE001
        return False
    return True


def model_download_size(repo: str) -> int:
    """Total bytes to fetch for `repo`, or 0 if the total cannot be determined.

    Returning 0 is a supported answer, not a failure: the caller shows an
    indeterminate progress bar instead of a percentage. Refusing to download
    because the size lookup failed would be a worse outcome than an imprecise bar.
    """
    from fnmatch import fnmatch

    from huggingface_hub import HfApi

    try:
        with online():
            info = HfApi().model_info(repo, files_metadata=True)
    except Exception as exc:  # noqa: BLE001
        log(f"could not determine download size for {repo}: {exc}")
        return 0
    return sum(
        f.size or 0
        for f in (info.siblings or [])
        if any(fnmatch(f.rfilename, pat) for pat in MODEL_FILES)
    )


def download_model(repo: str, on_progress: Any = None) -> str:
    """Fetch `repo`'s weights, reporting progress in bytes.

    ``on_progress(downloaded, total)`` is called as the transfer advances; `total`
    is 0 when unknown. Returns the local snapshot path.

    huggingface_hub reports progress by driving a tqdm instance per file, so the
    only way to observe it is to supply the tqdm class. The subclass below adds
    each file's increments into one running total, because a per-file bar that
    restarts five times tells the user nothing about how long they are waiting.
    """
    from huggingface_hub import snapshot_download
    from tqdm.auto import tqdm as _tqdm

    total = model_download_size(repo)
    state = {"done": 0}

    class ProgressTqdm(_tqdm):  # type: ignore[misc]
        def update(self, n: int | None = 1) -> bool | None:
            state["done"] += n or 0
            if on_progress is not None:
                on_progress(state["done"], total)
            return super().update(n)

    log(f"downloading {repo} ({total / 1e6:.0f} MB)" if total else f"downloading {repo}")
    # Downloads write to stdout under some terminal configurations, which would
    # corrupt the protocol stream mid-transfer.
    with online(), guard_stdout():
        path = snapshot_download(
            repo,
            allow_patterns=list(MODEL_FILES),
            tqdm_class=ProgressTqdm,
        )
    # A resumed transfer never reports the bytes it skipped, so the final tick has
    # to be sent explicitly or the bar stops short of full and looks stuck.
    if on_progress is not None and total:
        on_progress(total, total)
    log(f"downloaded {repo} to {path}")
    return path


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

    def __init__(
        self,
        model: str = DEFAULT_MODEL,
        device: str = "auto",
        repo: str = DEFAULT_REPO,
        compute_type: str = DEFAULT_COMPUTE,
        fallback_compute: str | None = None,
    ) -> None:
        # No catalogue lookup and no validation of `model` against a known set:
        # the host resolved all of this before spawning us (see
        # `crates/ov-asr/src/catalog.rs`). `model` is carried only so logs and the
        # `model_id` reported back to the app say `base.en` rather than
        # `Systran/faster-whisper-base.en`.
        if "/" not in repo:
            raise ValueError(f"repo must be a full 'org/name' id, got {repo!r}")
        self.model_name = model
        self.repo = repo
        self.compute_type = compute_type
        self.fallback_compute = fallback_compute
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
                return WhisperModel(self.repo, device=device, compute_type=ct, download_root=root)

        try:
            self._model = build(compute_type)
        except Exception as exc:  # noqa: BLE001
            fallback = self.fallback_compute
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
                    return "cuda", self.compute_type
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

    def _load_pcm16_wav(self, path: str) -> np.ndarray:
        """Read a 16 kHz mono 16-bit WAV straight into the array faster-whisper wants.

        faster-whisper's own path (`decode_audio`) goes through PyAV, which bundles
        an entire FFmpeg — general-purpose machinery for a format this sidecar never
        actually needs, because `ov-asr` is the only producer of these files and it
        always writes exactly this format (`crates/ov-asr/src/wav.rs`). Reading it
        with the standard library and doing `decode_audio`'s own int16→float32
        conversion by hand (`/ 32768.0`) gets byte-identical samples without the
        ~65 MB PyAV costs in the frozen build.

        Raises `ValueError` if the file is not exactly what `ov-asr` promises to
        write, rather than silently resampling or reinterpreting it — a format
        mismatch here means the two sides of the protocol have drifted apart, which
        is a bug to surface, not paper over.
        """
        with wave.open(path, "rb") as f:
            if f.getnchannels() != 1 or f.getsampwidth() != 2 or f.getframerate() != 16_000:
                raise ValueError(
                    f"expected 16 kHz mono 16-bit PCM, got {f.getnchannels()}ch "
                    f"{f.getsampwidth() * 8}bit {f.getframerate()}Hz from {path}"
                )
            raw = f.readframes(f.getnframes())
        return np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0

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
        audio = self._load_pcm16_wav(wav_path)
        segments, info = self._model.transcribe(
            audio,
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
        # No model list here any more: the sidecar is told which model to load and
        # has no view of the catalogue. `crates/ov-asr/src/catalog.rs` is the list.
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
