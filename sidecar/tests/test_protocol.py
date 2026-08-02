"""Protocol and hint-packing tests.

Deliberately model-free so they run in milliseconds on every push. Inference is
covered by a nightly job -- downloading weights per-PR would make CI slower than
the review it exists to support.
"""

from __future__ import annotations

import io
import json
import os
import struct
import wave
from pathlib import Path

import numpy as np
import pytest

from openvoice_asr.engine import MAX_HINT_CHARS, Engine, build_hint, model_repo_id, online
from openvoice_asr.protocol import (
    BadRequest,
    Request,
    err,
    ok,
    progress,
    read_requests,
)


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


class TestProgress:
    """Interim messages sent while a long operation is still running."""

    def test_carries_no_ok_key(self):
        # The host tells a progress tick from a final response by the presence of
        # `event` and reads on. Adding `ok` would make a typed reader treat a
        # download that is 3% complete as the answer to the request.
        msg = progress(4, downloaded=100, total=1000)
        assert "ok" not in msg
        assert msg["event"] == "progress"
        assert msg["id"] == 4

    def test_is_addressed_to_its_request(self):
        assert progress(9)["id"] == 9

    def test_survives_a_round_trip_through_the_wire(self):
        decoded = json.loads(json.dumps(progress(1, downloaded=5, total=7)))
        assert decoded == {"id": 1, "event": "progress", "downloaded": 5, "total": 7}


class TestModelRepoId:
    def test_expands_a_short_preset(self):
        assert model_repo_id("base.en") == "Systran/faster-whisper-base.en"

    def test_leaves_a_fully_qualified_repo_alone(self):
        # large-v3-turbo is a third-party conversion, not one of Systran's.
        assert model_repo_id("large-v3-turbo") == "deepdml/faster-whisper-large-v3-turbo-ct2"

    def test_rejects_an_unknown_name(self):
        # An unknown preset must not reach huggingface_hub as a repository id: the
        # failure would arrive as a 404 from the network rather than as a fact we
        # already knew locally.
        with pytest.raises(ValueError, match="unknown model"):
            model_repo_id("enormous-v9")


class TestOnline:
    def test_restores_offline_mode_afterwards(self):
        os.environ["HF_HUB_OFFLINE"] = "1"
        try:
            with online():
                assert "HF_HUB_OFFLINE" not in os.environ
            assert os.environ["HF_HUB_OFFLINE"] == "1"
        finally:
            os.environ.pop("HF_HUB_OFFLINE", None)

    def test_restores_offline_mode_after_a_failure(self):
        # A download that raises must not leave the process online. It would
        # silently reintroduce the ~171s cached-model load on every later start,
        # with nothing to connect the slowness back to this failure.
        os.environ["HF_HUB_OFFLINE"] = "1"
        try:
            with pytest.raises(RuntimeError), online():
                raise RuntimeError("connection reset")
            assert os.environ["HF_HUB_OFFLINE"] == "1"
        finally:
            os.environ.pop("HF_HUB_OFFLINE", None)

    def test_leaves_the_variable_unset_if_it_was_unset(self):
        os.environ.pop("HF_HUB_OFFLINE", None)
        with online():
            pass
        assert "HF_HUB_OFFLINE" not in os.environ


class TestLoadPcm16Wav:
    """The reader that replaced PyAV for this sidecar's own WAV files.

    ov-asr is the only producer of the files this reads, and it always writes 16
    kHz mono 16-bit PCM (crates/ov-asr/src/wav.rs). These tests exist to prove the
    hand-rolled int16->float32 conversion is the exact one faster-whisper's own
    decode_audio performs, not an approximation of it.
    """

    def _write_wav(self, path, samples, *, channels=1, rate=16_000, width=2):
        with wave.open(str(path), "wb") as f:
            f.setnchannels(channels)
            f.setsampwidth(width)
            f.setframerate(rate)
            f.writeframes(struct.pack(f"<{len(samples)}h", *samples))

    def test_matches_decode_audios_int16_to_float32_conversion(self, tmp_path):
        path = tmp_path / "in.wav"
        # 0, max, min, and a mid-scale value -- the corners decode_audio's own
        # `astype(float32) / 32768.0` has to get right.
        samples = [0, 32767, -32768, 16384]
        self._write_wav(path, samples)

        engine = Engine(model="base.en", device="cpu")
        audio = engine._load_pcm16_wav(str(path))

        assert audio.dtype == np.float32
        expected = np.array(samples, dtype=np.float32) / 32768.0
        np.testing.assert_array_almost_equal(audio, expected, decimal=6)

    def test_rejects_stereo(self, tmp_path):
        path = tmp_path / "stereo.wav"
        self._write_wav(path, [0, 0, 0, 0], channels=2)
        engine = Engine(model="base.en", device="cpu")
        with pytest.raises(ValueError, match="16 kHz mono"):
            engine._load_pcm16_wav(str(path))

    def test_rejects_the_wrong_sample_rate(self, tmp_path):
        path = tmp_path / "wrong-rate.wav"
        self._write_wav(path, [0, 0], rate=44_100)
        engine = Engine(model="base.en", device="cpu")
        with pytest.raises(ValueError, match="16 kHz mono"):
            engine._load_pcm16_wav(str(path))

    def test_reads_the_real_fixture_files(self):
        # The actual audio this protocol carries in production, not a synthetic
        # WAV -- proof the reader works on what ov-asr really writes, not just on
        # inputs shaped to fit it.
        fixtures = Path(__file__).parents[2] / "fixtures" / "audio"
        if not fixtures.is_dir():
            pytest.skip("fixtures/audio not present in this checkout")
        engine = Engine(model="base.en", device="cpu")
        found = False
        for wav in fixtures.glob("*.wav"):
            found = True
            audio = engine._load_pcm16_wav(str(wav))
            assert audio.dtype == np.float32
            assert audio.ndim == 1
            assert np.all(np.abs(audio) <= 1.0)
        assert found, "expected at least one fixture WAV"
