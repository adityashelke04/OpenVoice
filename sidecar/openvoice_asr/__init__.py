"""OpenVoice ASR sidecar.

A long-lived child process that turns audio into text using faster-whisper, spoken
to over newline-delimited JSON on stdin/stdout. See ADR 0003 for why the day-one
backend lives in a separate process rather than in the Rust binary.
"""

__version__ = "0.1.0"
