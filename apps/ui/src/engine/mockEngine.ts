/** A stand-in for the Rust engine, so the panel can be built and reviewed before
 *  the Tauri shell exists.
 *
 *  It emits the same event stream `ov-core` will emit, with realistic timings taken
 *  from measurements on the reference machine (RTX 3050 Laptop): ~190-600 ms decode,
 *  ~1.4 s warm model load, ~27x realtime. When the real engine lands, this file is
 *  deleted and `subscribe` is repointed at Tauri's event channel — nothing else in
 *  the UI changes, which is the point of keeping the UI a pure projection. */

import type { EngineEvent, Utterance } from "./types";

type Listener = (e: EngineEvent) => void;

const SAMPLE_TRANSCRIPTS = [
  {
    raw: "um so we need to call use effect here comma then return null",
    final: "So we need to call useEffect here, then return null",
    profile: "editor",
    app: "Code.exe",
  },
  {
    raw: "cube control get pods dash dash all namespaces",
    final: "kubectl get pods --all-namespaces",
    profile: "terminal",
    app: "WindowsTerminal.exe",
  },
  {
    raw: "camel case user profile equals new user profile open paren close paren",
    final: "userProfile = new UserProfile()",
    profile: "editor",
    app: "Cursor.exe",
  },
];

export class MockEngine {
  private listeners = new Set<Listener>();
  private timers: number[] = [];
  private session = 4;
  private running = false;

  subscribe(fn: Listener): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private emit(e: EngineEvent) {
    this.listeners.forEach((l) => l(e));
  }

  private at(ms: number, fn: () => void) {
    this.timers.push(window.setTimeout(fn, ms));
  }

  /** Run one full dictation session, end to end, with plausible timings. */
  simulate(durationMs = 3200) {
    if (this.running) return;
    this.running = true;
    this.clear();

    const s = ++this.session;
    const sample = SAMPLE_TRANSCRIPTS[s % SAMPLE_TRANSCRIPTS.length];
    this.emit({ type: "Listening", session: s, profile: sample.profile });

    // Level frames at ~30 Hz, shaped like speech: bursts with pauses between,
    // not a smooth sine. A meter that never rests reads as a progress bar.
    const frameMs = 33;
    for (let t = 0; t < durationMs; t += frameMs) {
      this.at(t, () => {
        const syllable = Math.sin(t / 110) * 0.5 + 0.5;
        const phrase = Math.sin(t / 900) * 0.35 + 0.65;
        const noise = Math.random() * 0.18;
        const rms = Math.max(0, Math.min(1, syllable * phrase * 0.7 + noise));
        this.emit({
          type: "Level",
          rms,
          peak: Math.min(1, rms + 0.12 + Math.random() * 0.1),
          elapsedMs: t,
        });
      });
    }

    const decodeMs = 380 + Math.round(Math.random() * 220);
    this.at(durationMs, () =>
      this.emit({ type: "Transcribing", session: s, audioMs: durationMs }),
    );
    this.at(durationMs + 8, () =>
      this.emit({ type: "Timing", session: s, stage: "finalize", tookMs: 8 }),
    );
    this.at(durationMs + 26, () =>
      this.emit({ type: "Timing", session: s, stage: "vad", tookMs: 18 }),
    );
    this.at(durationMs + decodeMs, () => {
      this.emit({ type: "Timing", session: s, stage: "decode", tookMs: decodeMs });
      this.emit({ type: "Timing", session: s, stage: "format", tookMs: 3 });
      this.emit({
        type: "Injecting",
        session: s,
        chars: sample.final.length,
      });
    });

    const injectMs = 70 + Math.round(Math.random() * 50);
    const total = 8 + 18 + decodeMs + 3 + injectMs;
    this.at(durationMs + decodeMs + injectMs, () => {
      this.emit({ type: "Timing", session: s, stage: "inject", tookMs: injectMs });
      this.emit({ type: "Timing", session: s, stage: "total", tookMs: total });
      this.emit({
        type: "Finished",
        session: s,
        outcome: { kind: "delivered" },
        text: sample.final,
        latencyMs: total,
      });
      this.running = false;
    });
    this.at(durationMs + decodeMs + injectMs + 1400, () =>
      this.emit({ type: "Idle" }),
    );
  }

  /** The degraded path: transcription succeeded, the target refused the paste. */
  simulateClipboardFallback() {
    if (this.running) return;
    this.running = true;
    this.clear();
    const s = ++this.session;
    const text = "SELECT * FROM users WHERE created_at > now() - interval '7 days'";

    this.emit({ type: "Listening", session: s, profile: "editor" });
    for (let t = 0; t < 2000; t += 33) {
      this.at(t, () =>
        this.emit({
          type: "Level",
          rms: Math.abs(Math.sin(t / 130)) * 0.6 + Math.random() * 0.15,
          peak: 0.8,
          elapsedMs: t,
        }),
      );
    }
    this.at(2000, () =>
      this.emit({ type: "Transcribing", session: s, audioMs: 2000 }),
    );
    this.at(2450, () =>
      this.emit({ type: "Injecting", session: s, chars: text.length }),
    );
    this.at(2600, () => {
      this.emit({
        type: "Finished",
        session: s,
        outcome: { kind: "clipboard_fallback", detail: text },
        text,
        latencyMs: 610,
      });
      // Phrased as what to do, not what broke. Nothing was lost.
      this.emit({
        type: "Notice",
        level: "warn",
        message: "Copied to clipboard — press Ctrl+V",
      });
      this.running = false;
    });
    this.at(4200, () => this.emit({ type: "Idle" }));
  }

  /** Microphone muted: caught before wasting a decode on silence. */
  simulateSilent() {
    if (this.running) return;
    this.running = true;
    this.clear();
    const s = ++this.session;
    this.emit({ type: "Listening", session: s, profile: "editor" });
    for (let t = 0; t < 1500; t += 33) {
      this.at(t, () =>
        this.emit({ type: "Level", rms: 0.001, peak: 0.002, elapsedMs: t }),
      );
    }
    this.at(1500, () => {
      this.emit({
        type: "Finished",
        session: s,
        outcome: { kind: "silent" },
        text: "",
        latencyMs: 12,
      });
      this.emit({
        type: "Notice",
        level: "warn",
        message: "No speech detected — is your microphone muted?",
      });
      this.running = false;
    });
    this.at(3000, () => this.emit({ type: "Idle" }));
  }

  clear() {
    this.timers.forEach(clearTimeout);
    this.timers = [];
  }

  stop() {
    this.clear();
    this.running = false;
    this.emit({ type: "Idle" });
  }
}

export const HISTORY_SEED: Utterance[] = [
  {
    id: 4,
    createdAt: Date.now() - 1000 * 60 * 3,
    durationMs: 3400,
    rawText: "um so we need to call use effect here comma then return null",
    finalText: "So we need to call useEffect here, then return null",
    profile: "editor",
    targetApp: "Code.exe",
    model: "faster-whisper/large-v3-turbo@cuda",
    status: "delivered",
    latencyMs: 612,
  },
  {
    id: 3,
    createdAt: Date.now() - 1000 * 60 * 14,
    durationMs: 2100,
    rawText: "cube control get pods dash dash all namespaces",
    finalText: "kubectl get pods --all-namespaces",
    profile: "terminal",
    targetApp: "WindowsTerminal.exe",
    model: "faster-whisper/large-v3-turbo@cuda",
    status: "delivered",
    latencyMs: 448,
  },
  {
    id: 2,
    createdAt: Date.now() - 1000 * 60 * 41,
    durationMs: 8800,
    rawText:
      "refactor the transcriber trait so the sidecar can be swapped for whisper cpp without touching core",
    finalText:
      "Refactor the Transcriber trait so the sidecar can be swapped for whisper.cpp without touching core",
    profile: "prose",
    targetApp: "chrome.exe",
    model: "faster-whisper/large-v3-turbo@cuda",
    status: "clipboard_fallback",
    latencyMs: 1104,
  },
  {
    id: 1,
    createdAt: Date.now() - 1000 * 60 * 63,
    durationMs: 1400,
    rawText: "git commit dash m fix the ring buffer overrun",
    finalText: "git commit -m fix the ring buffer overrun",
    profile: "terminal",
    targetApp: "WindowsTerminal.exe",
    model: "faster-whisper/base.en@cuda",
    status: "delivered",
    latencyMs: 296,
  },
];
