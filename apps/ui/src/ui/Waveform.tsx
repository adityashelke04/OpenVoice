import { useEffect, useRef } from "react";
import type { CSSProperties } from "react";

/**
 * How tall bar `i` of `n` may reach, as a fraction of the full height.
 *
 * A row of bars that can all hit the ceiling reads as an equaliser. A voice seen
 * on a waveform is fullest in the middle of a phrase and tapers at both ends, and
 * shaping the row that way is most of what makes the Flow Bar's wave read as
 * someone speaking rather than as a level meter. The floor is 0.35 so the ends
 * still move visibly: an envelope that reached zero would leave dead bars.
 */
export function waveEnvelope(i: number, n: number): number {
  return 0.35 + 0.65 * Math.sin((Math.PI * (i + 0.5)) / n);
}

/**
 * Scrolling live waveform.
 *
 * Holds a rolling buffer of the last ~1.5 s of loudness. Each frame the newest
 * sample enters at the right and every older one shifts left, so a spoken syllable
 * becomes **a shape that travels** — you watch the sentence you just said move
 * across the bar. An equaliser, where bars jitter in place, communicates nothing to
 * a person and is the thing this deliberately is not.
 *
 * Heights are written straight to the DOM rather than through React state. At 30 Hz
 * a re-render per frame would make this the most expensive thing in an app that is
 * supposed to be invisible when idle.
 */
export function Waveform({
  level,
  levelRef,
  bars = 32,
  idle,
  publish,
  shaped = false,
}: {
  /** Static level. Used by the component sheet and previews. */
  level?: number;
  /**
   * Live level, read every animation frame.
   *
   * Passing a ref rather than a value is the point: the microphone level changes
   * ~30 times a second, and routing that through React state re-rendered the whole
   * overlay at frame rate, restarting CSS animations mid-flight and making the bar
   * flicker. A ref costs zero renders.
   */
  levelRef?: { current: number };
  bars?: number;
  idle?: boolean;
  /**
   * Element to publish the metered level onto, as a `--level` custom property.
   *
   * The Flow Bar's glow tracks the voice, and the metered value it needs is
   * computed here, once, in a loop that already runs every frame. Writing it
   * onto an ancestor element rather than returning it keeps that value out of
   * React entirely — the alternative is a state update per frame, which is the
   * exact thing the `levelRef` note above exists to prevent. Custom properties
   * inherit downward, so this has to be the element that *owns* the glow.
   */
  publish?: { current: HTMLElement | null };
  /** Scale each bar by `waveEnvelope`, so the row tapers like a voice. */
  shaped?: boolean;
}) {
  const host = useRef<HTMLDivElement>(null);
  const latest = useRef(level ?? 0);
  latest.current = level ?? 0;
  const isIdle = useRef(idle);
  isIdle.current = idle;
  const external = useRef(levelRef);
  external.current = levelRef;
  const sink = useRef(publish);
  sink.current = publish;
  const isShaped = useRef(shaped);
  isShaped.current = shaped;

  useEffect(() => {
    const el = host.current;
    if (!el) return;

    const n = bars;
    /** Where each bar is heading — the scrolled history of loudness. */
    const target = new Float32Array(n);
    /** Where each bar actually is. Chases `target` every frame. */
    const current = new Float32Array(n);
    const nodes = Array.from(el.children) as HTMLElement[];

    let raf = 0;
    let lastShift = 0;
    let lastFrame = 0;

    /** How often the wave advances one bar. ~18 steps/sec reads as a flowing
     *  ribbon; faster becomes a blur, slower becomes a stutter. */
    const SHIFT_MS = 55;
    /** Per-bar chase, as a time constant rather than a per-frame fraction, so the
     *  wave travels at the same speed on a 60Hz and a 144Hz display. 50ms is the
     *  constant the old fixed 0.28-per-frame worked out to at 60fps — each bar
     *  stays slightly behind its neighbour, which is what makes this read as a
     *  wave travelling rather than a row of independent bars. */
    const CHASE_MS = 50;
    const MIN = 0.05;

    /* VU ballistics.
     *
     * A meter that rises and falls at the same rate reads as a progress bar. A
     * real one snaps up and eases down, and holds its peak long enough for the
     * eye to catch it — that asymmetry is the whole reason this looks like an
     * instrument instead of a graph.
     *
     * The wave samples `meter`, not the raw level: the raw signal is a noisy
     * 30Hz stream, and feeding it straight in was making quiet consonants
     * flicker between bars. */
    const ATTACK_MS = 60;
    const RELEASE_MS = 380;
    const HOLD_MS = 800;

    let meter = 0;
    let peak = 0;
    let peakAt = 0;

    const tick = (now: number) => {
      raf = requestAnimationFrame(tick);

      // Clamped so a backgrounded window returning after seconds away does not
      // jump the meter to its target in one frame.
      const dt = lastFrame === 0 ? 16.7 : Math.min(64, now - lastFrame);
      lastFrame = now;

      const raw = isIdle.current
        ? 0
        : Math.min(1, Math.max(0, external.current ? external.current.current : latest.current));

      // Rise fast, fall slow. Exponential toward the target with a time constant
      // per direction, so both are frame-rate independent.
      const tau = raw >= meter ? ATTACK_MS : RELEASE_MS;
      meter += (raw - meter) * (1 - Math.exp(-dt / tau));

      // Peak hold: the ceiling stays put for HOLD_MS after a syllable, then
      // releases. Published to the glow so a loud word leaves a visible trace
      // rather than vanishing the instant the speaker stops.
      if (meter >= peak) {
        peak = meter;
        peakAt = now;
      } else if (now - peakAt > HOLD_MS) {
        peak += (meter - peak) * (1 - Math.exp(-dt / RELEASE_MS));
      }

      const out = sink.current?.current;
      if (out) out.style.setProperty("--level", peak.toFixed(3));

      if (now - lastShift >= SHIFT_MS) {
        lastShift = now;
        target.copyWithin(0, 1);
        target[n - 1] = meter;
      }

      // Interpolate on EVERY frame, not on every sample. This is the whole
      // difference between smooth and mushy: a CSS transition chasing a 30 Hz
      // signal never arrives, but 60 fps interpolation toward a moving target is
      // continuous by construction.
      const chase = 1 - Math.exp(-dt / CHASE_MS);
      for (let i = 0; i < n; i++) {
        current[i] += (target[i] - current[i]) * chase;
        // sqrt approximates perceived loudness, so quiet speech still moves the
        // bar visibly while loud speech does not slam into the ceiling.
        const v =
          MIN + Math.sqrt(current[i]) * (1 - MIN) * (isShaped.current ? waveEnvelope(i, n) : 1);
        // A custom property rather than the transform itself, so the stylesheet
        // owns which axis this scales. Docked to a side edge the bar runs
        // vertically and the wave has to run with it, and the alternative — a
        // quarter-turn `rotate` on the container — needs the container's own
        // length in advance, which is a number this loop has no business
        // knowing. The write is the same cost either way.
        nodes[i].style.setProperty("--v", v.toFixed(4));
      }
    };

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [bars]);

  return (
    <div className="wave" data-idle={idle} ref={host} role="presentation">
      {Array.from({ length: bars }, (_, i) => (
        <span
          key={i}
          className="wave-bar"
          // The bar's place in the row, so the stylesheet can give it its slice
          // of the spectrum without a colour being computed per frame.
          style={{ "--k": i } as CSSProperties}
        />
      ))}
    </div>
  );
}
