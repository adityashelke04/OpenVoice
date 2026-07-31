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
import traceback

from .engine import Engine, probe
from .protocol import (
    PROTOCOL_VERSION,
    BadRequest,
    Request,
    err,
    log,
    ok,
    read_requests,
    write,
)


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
    parser = argparse.ArgumentParser(prog="openvoice_asr")
    parser.add_argument("--model", default="base.en")
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
        engine = Engine(model=args.model, device=args.device)
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
