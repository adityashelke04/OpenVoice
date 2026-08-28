/** Two tiny, synthesized UI sounds: one for a dictation starting, one for it
 *  finishing successfully.
 *
 * No audio assets ship in this app — both tones are generated with the Web
 * Audio API instead, so there is nothing to license, localize, or bundle at a
 * particular sample rate. Both are short and quiet on purpose: this is a
 * peripheral acknowledgement for a utility window seen for a second at a
 * time, not a jingle.
 */

let ctx: AudioContext | null = null;

function getCtx(): AudioContext | null {
  if (typeof window === "undefined" || !("AudioContext" in window)) return null;
  ctx ??= new AudioContext();
  if (ctx.state === "suspended") {
    void ctx.resume();
  }
  return ctx;
}

/**
 * One voice: a sine at `frequency` plus a quiet second harmonic, both with a
 * fast attack and a smooth exponential decay to (near) silence.
 *
 * The overtone is what keeps this from reading as a raw test-tone beep — a
 * single bare sine is the textbook "AI generated sound" tell; a fundamental
 * plus a soft octave above it is closer to how an actual chime or bell is
 * built, at the cost of two oscillators instead of one.
 */
function voice(c: AudioContext, frequency: number, at: number, duration: number, peak: number) {
  const t0 = c.currentTime + at;
  const stop = t0 + duration;

  for (const [freq, gain] of [
    [frequency, peak],
    [frequency * 2, peak * 0.18],
  ] as const) {
    const osc = c.createOscillator();
    const g = c.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    // exponentialRampToValueAtTime can never target exactly 0, hence 0.0001 as
    // the effective floor on both ends of the envelope.
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(gain, t0 + 0.006);
    g.gain.exponentialRampToValueAtTime(0.0001, stop);
    osc.connect(g).connect(c.destination);
    osc.start(t0);
    osc.stop(stop + 0.02);
  }
}

/**
 * Played the instant the hotkey engages and recording starts.
 *
 * One short, low-key tap — deliberately less "eventful" than the completion
 * chime below, since starting is the routine half of this interaction and
 * happens far more often than any single completion is dwelt on.
 */
export function playStartTone() {
  const c = getCtx();
  if (!c) return;
  voice(c, 440, 0, 0.08, 0.05); // A4, ~80ms
}

/**
 * Played when a dictation finishes and lands successfully. Must never be
 * called for a failed or clipboard-fallback completion — the caller decides
 * that, this function just makes the sound.
 *
 * A rising major third (C5 -> E5) rather than an arbitrary pair of
 * frequencies: that specific interval is what reads as "resolved" and
 * "positive" to a Western-tuned ear, the same reason it shows up in most
 * hand-tuned success chimes rather than a random pair of tones would. Kept to
 * ~170ms end to end so it stays a glance-length acknowledgement.
 */
export function playCompletionChime() {
  const c = getCtx();
  if (!c) return;
  voice(c, 523.25, 0, 0.13, 0.055); // C5
  voice(c, 659.25, 0.055, 0.15, 0.05); // E5, slightly overlapping the first
}
