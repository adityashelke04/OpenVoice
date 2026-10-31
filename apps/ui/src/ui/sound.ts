/** Optional local reference recordings take precedence over original synthesized
 * fallback cues. The user's reference pack is Git-excluded, never downloaded. */

export type FlowSound = "start" | "stop" | "cancel" | "success" | "lock" | "error";

let ctx: AudioContext | null = null;
let pending = 0;
const active = new Map<AudioScheduledSourceNode, () => void>();
const recordings = new Map<FlowSound, AudioBuffer>();
let loading: Promise<void> | undefined;

function context(): AudioContext | null {
  if (typeof window === "undefined" || !("AudioContext" in window)) return null;
  if (!ctx || ctx.state === "closed") ctx = new AudioContext();
  return ctx;
}

/** Resolve only bundled, same-origin files, before any cue needs to play. */
export function preloadFlowSounds(): Promise<void> {
  return loading ??= (async () => {
    try {
      const c = context();
      if (!c) return;
      await Promise.all((["start", "stop", "success", "lock", "error"] as const).map(async cue => {
        try {
          const response = await fetch(`/flow-sounds/${cue}.wav`);
          if (response.ok) recordings.set(cue, await c.decodeAudioData(await response.arrayBuffer()));
        } catch { /* This optional local pack is absent from repository builds. */ }
      }));
    } catch { /* A device failure must not affect the rest of the application. */ }
  })();
}

export function silenceFlowSounds() {
  pending++;
  for (const [oscillator, disconnect] of active) {
    try { oscillator.stop(); } catch { /* It may already have ended. */ }
    disconnect();
  }
}

/** A soft second harmonic gives the short acknowledgement a little body. */
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
    const disconnect = () => {
      if (!active.delete(osc)) return;
      osc.disconnect();
      g.disconnect();
    };
    active.set(osc, disconnect);
    osc.onended = disconnect;
    try {
      osc.connect(g).connect(c.destination);
      osc.start(t0);
      osc.stop(stop);
    } catch {
      disconnect();
      throw new Error("Sound output unavailable");
    }
  }
}

/** Device/autoplay failures are silent, and a delayed resume cannot replay old cues. */
export async function playFlowSound(cue: FlowSound): Promise<void> {
  const generation = ++pending;
  const requestedAt = performance.now();
  try {
    const c = context();
    if (!c) return;
    if (c.state === "suspended") await c.resume();
    if (generation !== pending || c.state !== "running" || performance.now() - requestedAt > 250) return;
    const recording = recordings.get(cue === "cancel" ? "stop" : cue);
    if (recording) {
      let source: AudioBufferSourceNode | undefined;
      let gain: GainNode | undefined;
      try {
        source = c.createBufferSource();
        const node = source;
        node.buffer = recording;
        if (cue === "error") {
          gain = c.createGain();
          gain.gain.value = 0.5;
          gain.connect(c.destination);
        }
        const disconnect = () => {
          if (!active.delete(node)) return;
          node.disconnect();
          gain?.disconnect();
        };
        active.set(node, disconnect);
        node.onended = disconnect;
        node.connect(gain ?? c.destination);
        node.start();
        return;
      } catch {
        if (source) { active.delete(source); source.disconnect(); }
        gain?.disconnect();
        // A failed recording still gets immediate synthesized feedback.
      }
    }
    switch (cue) {
      case "start": voice(c, 443.7, 0.018, 0.162, 0.06); break;
      case "stop":
      case "cancel": voice(c, 287.5, 0.038, 0.181, 0.055); break;
      case "lock": voice(c, 223.9, 0, 0.165, 0.045); break;
      case "success":
        voice(c, 265.1, 0, 0.38, 0.025);
        voice(c, 387.9, 0.025, 0.46, 0.025);
        voice(c, 522.1, 0.05, 0.569, 0.04);
        break;
      case "error":
        voice(c, 440.2, 0, 0.22, 0.04);
        voice(c, 660.3, 0, 0.22, 0.02);
        voice(c, 329.1, 0.16, 0.308, 0.04);
        break;
    }
  } catch { /* Feedback must never interrupt capture, delivery, or the UI. */ }
}
