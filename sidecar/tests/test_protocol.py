"""Protocol and hint-packing tests.

Deliberately model-free so they run in milliseconds on every push. Inference is
covered by a nightly job -- downloading weights per-PR would make CI slower than
the review it exists to support.
"""

from __future__ import annotations

import io
import json

import pytest

from openvoice_asr.engine import MAX_HINT_CHARS, build_hint
from openvoice_asr.protocol import BadRequest, Request, err, ok, read_requests


class TestRequest:
    def test_parses_op_and_params(self):
        req = Request.parse('{"id": 7, "op": "transcribe", "wav": "a.wav"}')
        assert req.id == 7
        assert req.op == "transcribe"
        assert req.params == {"wav": "a.wav"}

    def test_id_defaults_to_zero(self):
        assert Request.parse('{"op": "ping"}').id == 0

    def test_rejects_missing_op(self):
        with pytest.raises(ValueError, match="missing 'op'"):
            Request.parse('{"id": 1}')

    def test_rejects_non_object(self):
        with pytest.raises(ValueError, match="JSON object"):
            Request.parse("[1, 2, 3]")

    def test_tolerates_a_utf8_bom(self):
        # PowerShell's pipe emits a BOM by default, and Windows is the primary
        # platform. Without this the sidecar appears broken on first contact.
        assert Request.parse('﻿{"op": "ping"}').op == "ping"


class TestReadRequests:
    def test_skips_blank_lines(self):
        stream = io.StringIO('{"op":"ping"}\n\n  \n{"op":"probe"}\n')
        assert [r.op for r in read_requests(stream)] == ["ping", "probe"]

    def test_ends_cleanly_when_stdin_closes(self):
        # A closed stdin means the parent died. The loop must terminate so the
        # process exits instead of orphaning and holding VRAM.
        assert list(read_requests(io.StringIO(""))) == []

    def test_a_malformed_line_does_not_kill_the_loop(self):
        # The failure this guards against: an exception escaping the generator
        # ends iteration and exits the process, so one stray byte on stdin unloads
        # 1.6 GB of weights and forces a multi-second reload.
        out = list(read_requests(io.StringIO('{"op":"ping"}\nnot json\n{"op":"probe"}\n')))
        assert [type(o) for o in out] == [Request, BadRequest, Request]
        assert out[2].op == "probe", "service must continue after a bad line"

    def test_bad_request_truncates_the_offending_line(self):
        # The raw line is echoed into logs; an unbounded one could dump a whole
        # transcript into a file the user did not consent to.
        out = list(read_requests(io.StringIO("x" * 5000 + "\n")))
        assert isinstance(out[0], BadRequest)
        assert len(out[0].raw) <= 200


class TestResponses:
    def test_ok_carries_the_request_id(self):
        assert ok(3, text="hi") == {"id": 3, "ok": True, "text": "hi"}

    def test_err_defaults_to_retriable(self):
        assert err(1, "boom")["retriable"] is True

    def test_err_can_mark_permanent_failures(self):
        # A malformed request will fail identically forever; restarting the
        # sidecar for it would be a pointless loop.
        assert err(1, "bad wav", retriable=False)["retriable"] is False

    def test_responses_are_json_serialisable(self):
        json.dumps(ok(1, text="ünïcødé", confidence=-0.3))


class TestBuildHint:
    def test_no_vocabulary_means_no_prompt(self):
        assert build_hint(None) is None
        assert build_hint([]) is None

    def test_includes_terms(self):
        hint = build_hint(["useEffect", "kubectl"])
        assert hint is not None
        assert "useEffect" in hint and "kubectl" in hint

    def test_truncates_to_the_prompt_budget(self):
        # Whisper silently truncates an over-long initial prompt, dropping whatever
        # landed last. Budgeting here keeps that from being a surprise.
        hint = build_hint([f"term{i:04d}" for i in range(500)])
        assert hint is not None
        assert len(hint) <= MAX_HINT_CHARS + len("Technical vocabulary: .")

    def test_keeps_highest_priority_terms_when_truncating(self):
        terms = ["first"] + [f"filler{i:04d}" for i in range(500)]
        hint = build_hint(terms)
        assert hint is not None
        assert "first" in hint, "callers rank by relevance; ranking must be honoured"
