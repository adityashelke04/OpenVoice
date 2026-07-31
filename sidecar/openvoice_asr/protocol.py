"""Newline-delimited JSON protocol between the Rust host and this sidecar.

One JSON object per line, in both directions. Chosen over gRPC or a socket because
it is trivially debuggable: you can drive the sidecar by hand from a terminal and
read the wire format with your eyes, which matters a great deal when the thing you
are debugging is a child process that only misbehaves under load.

**Invariant: stdout carries protocol traffic and nothing else.** Every log line,
warning, progress bar, and traceback goes to stderr. A stray ``print`` in a
dependency corrupts the stream and produces a failure that looks like a parser bug
three layers away, so :func:`openvoice_asr.engine.guard_stdout` redirects stdout
during model loading as a belt-and-braces measure.
"""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass, field
from typing import Any, Iterator

PROTOCOL_VERSION = 1


@dataclass(frozen=True)
class Request:
    """A message from the Rust host."""

    id: int
    op: str
    """One of: ``hello``, ``warm``, ``transcribe``, ``ping``, ``shutdown``."""

    params: dict[str, Any] = field(default_factory=dict)

    @staticmethod
    def parse(line: str) -> "Request":
        # Strip a UTF-8 BOM. Windows tooling emits one readily -- PowerShell's pipe
        # does it by default -- and a BOM on the first line would otherwise make the
        # sidecar look broken on exactly the platform it primarily targets.
        obj = json.loads(line.lstrip("﻿"))
        if not isinstance(obj, dict):
            raise ValueError("request must be a JSON object")
        if "op" not in obj:
            raise ValueError("request missing 'op'")
        return Request(
            id=int(obj.get("id", 0)),
            op=str(obj["op"]),
            params={k: v for k, v in obj.items() if k not in ("id", "op")},
        )


@dataclass(frozen=True)
class BadRequest:
    """A line that could not be parsed.

    Yielded instead of raised. An exception escaping the read loop would terminate
    the generator and kill the process, meaning one malformed byte on stdin takes
    down a daemon holding 1.6 GB of loaded weights. A dictation tool has to be more
    durable than that: report the bad line, keep serving.
    """

    raw: str
    error: str


def ok(request_id: int, **fields: Any) -> dict[str, Any]:
    """Build a success response."""
    return {"id": request_id, "ok": True, **fields}


def err(request_id: int, message: str, *, retriable: bool = True) -> dict[str, Any]:
    """Build an error response.

    ``retriable`` tells the host's supervisor whether restarting the sidecar could
    plausibly help. A corrupt WAV is not retriable; a CUDA OOM is.
    """
    return {"id": request_id, "ok": False, "error": message, "retriable": retriable}


def write(obj: dict[str, Any]) -> None:
    """Emit one protocol message and flush.

    The flush is not optional. Without it the host blocks waiting for a response
    that is sitting in a buffer, and the resulting hang looks exactly like a model
    that is merely slow.
    """
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def read_requests(stream: Any = None) -> Iterator[Request | BadRequest]:
    """Yield requests until the host closes stdin.

    Never raises on malformed input; see :class:`BadRequest`.

    A closed stdin means the parent died. Ending the iteration lets ``main`` exit
    cleanly rather than leaving an orphaned process holding 1.6 GB of VRAM.
    """
    stream = stream if stream is not None else sys.stdin
    for line in stream:
        line = line.strip()
        if not line:
            continue
        try:
            yield Request.parse(line)
        except (ValueError, TypeError) as exc:
            yield BadRequest(raw=line[:200], error=str(exc))


def log(message: str) -> None:
    """Write a diagnostic line to stderr, where it cannot corrupt the protocol."""
    sys.stderr.write(f"[ov-asr] {message}\n")
    sys.stderr.flush()
