"""Sidecar entry point.

Run by ``ov-asr`` as a long-lived child process:

    python -m openvoice_asr --model large-v3-turbo

Reads newline-delimited JSON requests on stdin, writes responses on stdout.
Diagnostics go to stderr. See :mod:`openvoice_asr.protocol`.

Manual driving, which is the fastest way to check the model actually works::

    echo {"id":1,"op":"probe"} | python -m openvoice_asr
"""

from __future__ import annotations

import argparse
import sys
import time
import traceback

from .engine import (
    DEFAULT_COMPUTE,
    DEFAULT_MODEL,
    DEFAULT_REPO,
    Engine,
    download_model,
    model_is_cached,
    probe,
)
from .protocol import (
    PROTOCOL_VERSION,
    BadRequest,
    Request,
    err,
    log,
    ok,
    progress,
    read_requests,
    write,
)

# Progress ticks are throttled to this interval. huggingface_hub advances its
# counter once per network chunk, which is thousands of times a second on a fast
# link; forwarding all of them would swamp the host's pipe with messages nobody
# can read and turn a progress bar into a bottleneck.
PROGRESS_INTERVAL_S = 0.2


def _force_utf8_io() -> None:
    """Make stdin/stdout/stderr UTF-8, unconditionally.

    Both sides of this protocol write and read raw UTF-8: this module's own
    ``write()`` uses ``ensure_ascii=False``, and the Rust host's ``serde_json``
    does the same. But Python's stdio streams are only UTF-8 by default inside
    an interactive console (PEP 528) -- redirected to a pipe, which is exactly
    how the Rust host launches this process, they fall back to
    ``locale.getpreferredencoding()``, the ANSI code page (``cp1252`` on a
    typical Windows install). That breaks the protocol two different ways,
    confirmed by actually spawning a piped child on this platform:

    - A character cp1252 cannot represent at all (CJK, emoji) raises
      ``UnicodeEncodeError`` on ``write()`` and crashes the process outright.
    - A character cp1252 *can* represent, but as a single byte that is not
      valid UTF-8 on its own (ordinary accented Latin text -- "café",
      "naïve" -- or the smart quotes and en/em dashes Whisper's own
      punctuation restoration commonly adds), writes bytes that Rust's
      ``read_line`` then rejects as invalid UTF-8. The Rust side retries the
      request once against a freshly-spawned sidecar, which decodes the exact
      same non-ASCII characters and hits the exact same failure again -- so
      the retry cannot help, and the transcript is lost outright. That is
      precisely the "never lose a word" guarantee this app promises, broken
      by something as ordinary as a dictated loanword.

    Called first thing, before any request is read or any response written.
    """
    for stream in (sys.stdin, sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")


def ensure_model(req: Request, engine: Engine) -> dict[str, object]:
    """Make a model's weights available locally, downloading them if needed.

    Reports progress as interim ``event: progress`` messages before the single
    final response. The first run of a fresh install fetches ~1.6 GB for the
    default model; without progress the app looks hung for several minutes, and
    the most common reaction to that is to kill it halfway through.

    Idempotent, and cheap when the weights are already present — so the host can
    call it unconditionally on every start rather than tracking installation
    state of its own, which would drift the moment a user cleared the cache.
    """
    name = req.params.get("model") or engine.model_name
    if not isinstance(name, str):
        return err(req.id, "'model' must be a string", retriable=False)

    # The repository to fetch, which the host resolves from its catalogue. It may
    # be a model this sidecar was not started for and cannot load: the Models
    # screen offers a Download button per model, so that a user can fetch weights
    # before committing to them and without restarting the app.
    #
    # Downloading is independent of loading -- it writes to the shared cache and
    # touches nothing already resident -- so serving it from the running process
    # is safe. It just cannot be followed by a decode on those weights until the
    # host restarts the sidecar against them.
    repo = req.params.get("repo") or engine.repo
    if not isinstance(repo, str) or "/" not in repo:
        return err(req.id, "'repo' must be a full 'org/name' id", retriable=False)

    if model_is_cached(repo):
        return ok(req.id, model=name, fetched=False)

    last_tick = 0.0

    def report(done: int, total: int) -> None:
        nonlocal last_tick
        now = time.monotonic()
        # Always let the final tick through, so the bar reaches full rather than
        # stopping at whatever fraction the throttle last allowed.
        if now - last_tick < PROGRESS_INTERVAL_S and not (total and done >= total):
            return
        last_tick = now
        write(progress(req.id, downloaded=done, total=total))

    try:
        download_model(repo, report)
    except Exception as exc:  # noqa: BLE001
        # Retriable: this is nearly always a dropped connection, and the transfer
        # resumes from where it stopped.
        return err(req.id, f"could not download {name}: {type(exc).__name__}: {exc}")

    # `fetched`, not `downloaded`: the progress messages already use `downloaded`
    # for a byte count, and a field that is an integer on one message and a boolean
    # on the next cannot be decoded by a typed reader.
    return ok(req.id, model=name, fetched=True)


def handle(req: Request, engine: Engine) -> dict[str, object] | None:
    """Dispatch one request. Returns ``None`` to signal shutdown."""
    if req.op == "hello":
        return ok(
            req.id,
            protocol=PROTOCOL_VERSION,
            model=engine.model_name,
            pid=__import__("os").getpid(),
        )

    if req.op == "ping":
        # Cheap liveness check. The host's supervisor uses this to distinguish
        # "busy decoding" from "wedged", which are indistinguishable from the
        # outside without it.
        return ok(req.id, pong=True)

    if req.op == "probe":
        return ok(req.id, **probe())

    if req.op == "warm":
        engine.load()
        return ok(req.id, device=engine.device_in_use, model_id=engine.model_id)

    if req.op == "ensure_model":
        return ensure_model(req, engine)

    if req.op == "transcribe":
        wav = req.params.get("wav")
        if not isinstance(wav, str) or not wav:
            return err(req.id, "transcribe requires a 'wav' path", retriable=False)
        result = engine.transcribe(
            wav,
            vocabulary=req.params.get("vocabulary") or [],
            language=req.params.get("language") or None,
        )
        return ok(
            req.id,
            text=result.text,
            language=result.language,
            confidence=result.confidence,
            decode_ms=result.decode_ms,
            model_id=engine.model_id,
        )

    if req.op == "shutdown":
        return None

    return err(req.id, f"unknown op {req.op!r}", retriable=False)


def main(argv: list[str] | None = None) -> int:
    _force_utf8_io()

    parser = argparse.ArgumentParser(prog="openvoice_asr")
    # The host resolves the model from its own catalogue and passes the result.
    # The defaults here only make a hand-run `python -m openvoice_asr` work for
    # debugging; see `crates/ov-asr/src/catalog.rs`.
    parser.add_argument("--model", default=DEFAULT_MODEL, help="name, for logs and history")
    parser.add_argument("--repo", default=DEFAULT_REPO, help="Hugging Face repository to load")
    parser.add_argument("--compute-type", default=DEFAULT_COMPUTE)
    parser.add_argument(
        "--fallback-compute",
        default=None,
        help="compute type to retry with when the preferred one will not fit, "
        "which is almost always an out-of-VRAM failure",
    )
    parser.add_argument(
        "--device",
        default="auto",
        choices=["auto", "cuda", "cpu"],
        help="'auto' uses CUDA when usable and silently falls back to CPU",
    )
    parser.add_argument(
        "--preload",
        action="store_true",
        help="load weights before accepting requests, so the first utterance is not "
        "penalised by a cold start",
    )
    parser.add_argument(
        "--allow-download",
        action="store_true",
        help="permit fetching weights from Hugging Face. Off by default: the sidecar "
        "is inference-only, and an unguarded network call costs ~171s per load on a "
        "cached model. Downloads belong to the model manager.",
    )
    args = parser.parse_args(argv)

    if args.allow_download:
        import os

        os.environ["OPENVOICE_ALLOW_DOWNLOAD"] = "1"
        from .engine import enforce_offline_by_default

        enforce_offline_by_default()
        log("network access enabled for this run (model download)")

    try:
        engine = Engine(
            model=args.model,
            device=args.device,
            repo=args.repo,
            compute_type=args.compute_type,
            fallback_compute=args.fallback_compute,
        )
    except ValueError as exc:
        log(str(exc))
        return 2

    if args.preload:
        try:
            engine.load()
        except Exception as exc:  # noqa: BLE001
            # Do not die here. A failed preload must still leave a live sidecar that
            # can report the problem over the protocol; exiting instead would send
            # the host's supervisor into a restart loop with no diagnosis.
            log(f"preload failed: {exc}")

    log(f"ready (model={args.model}, device={args.device})")

    for req in read_requests():
        if isinstance(req, BadRequest):
            # Report and keep serving. Dying here would mean one stray byte on
            # stdin unloads the model and restarts a multi-second warm-up.
            log(f"malformed request: {req.error}")
            write(err(0, f"malformed request: {req.error}", retriable=False))
            continue
        try:
            response = handle(req, engine)
        except Exception as exc:  # noqa: BLE001
            traceback.print_exc(file=sys.stderr)
            write(err(req.id, f"{type(exc).__name__}: {exc}"))
            continue
        if response is None:
            log("shutdown requested")
            break
        write(response)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
