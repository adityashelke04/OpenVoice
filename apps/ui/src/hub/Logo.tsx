/** The OpenVoice mark: seven rounded bars (spec 4.2, reference lines 412-420).
 *
 *  Geometry from assets/icon-source.png on its 1024 grid. Colour comes from CSS
 *  (`--brand`, `--brand-on-light`) and is never themed.
 *
 *  While the microphone is open the bars follow the live level, each one 40 ms
 *  behind its left neighbour, so the mark reads as a waveform travelling across
 *  it. A ring buffer of the last 400 ms of samples feeds that delay. The loop
 *  writes `style.transform` directly and never touches React state, and it only
 *  exists while listening: idle, the mark is a plain static SVG (idle CPU ~ 0).
 *
 *  Everything here interpolates per *frame*, never per sample. The engine
 *  throttles levels to one every 33 ms and that throttle is checked inside the
 *  audio callback, so the real stream lands at roughly 25 Hz against a 60-165 Hz
 *  display. Writing the newest sample straight to `scaleY` left the bars frozen
 *  on 74-86% of frames and then jumping by up to 0.65 of their travel in a single
 *  one — measured, and the reason the mark was reported as "laggy". A CSS
 *  transition cannot rescue this either: one chasing a 25 Hz signal never arrives.
 *
 *  Under prefers-reduced-motion and ?still=1 the loop never starts. Neither
 *  the media query nor MotionConfig reaches a hand-written transform, so the
 *  component asks both itself. */
import { useEffect, useRef } from "react";
import { isStill, REDUCED_MOTION, useMedia } from "./useMedia";

const BARS: [x: number, y: number, h: number][] = [
  [80, 424, 177], [210, 327, 370], [340, 160, 704], [470, 263, 498],
  [600, 112, 800], [730, 359, 306], [860, 439, 146],
];
const DELAY_MS = 40, WINDOW_MS = 400;

/** The shortest bar, as a fraction of its drawn height. The mark has to stay a
 *  mark in silence, so nothing ever collapses to nothing. */
const FLOOR = 0.35;

/* VU ballistics, as time constants rather than per-frame fractions, so the mark
 * moves at the same speed on a 60 Hz and a 165 Hz display.
 *
 * A meter that rises and falls at one rate reads as a progress bar. A real one
 * snaps up and eases down, and that asymmetry is what makes this read as an
 * instrument responding to a voice rather than a graph being plotted. Values are
 * the Flow Bar waveform's, which were tuned against this same signal, shortened
 * a little on release because seven bars carry less of a tail than thirty-two. */
const ATTACK_MS = 55;
const RELEASE_MS = 320;

/** Per-bar chase, on top of the meter. The delayed lookup below resolves to
 *  whichever frame's sample is nearest, which quantises by a frame at the edges;
 *  this smooths that out and doubles as the ease-in when the loop starts. */
const CHASE_MS = 45;

export function Logo({ levelRef, listening, still = isStill() }: { levelRef: { readonly current: number }; listening: boolean; still?: boolean }) {
  const svg = useRef<SVGSVGElement>(null);
  const reduce = useMedia(REDUCED_MOTION);
  const animate = listening && !still && !reduce;

  useEffect(() => {
    const rects = svg.current ? [...svg.current.querySelectorAll("rect")] : [];
    if (!animate) return;

    /** The smoothed meter, sampled once per frame, so the buffer is continuous
     *  before anything reads a delayed value out of it. */
    const samples: { t: number; v: number }[] = [];
    /** Where each bar actually is. Starts at its drawn height, so the mark eases
     *  out of its resting shape rather than popping down to the floor. */
    const current = new Float32Array(rects.length).fill(1);
    let meter = 0;
    let last = 0;
    let raf = 0;

    const at = (t: number) => {
      // Newest sample no later than t; the oldest one if the buffer is younger.
      for (let i = samples.length - 1; i >= 0; i--) if (samples[i].t <= t) return samples[i].v;
      return samples[0]?.v ?? 0;
    };

    const frame = (now: number) => {
      raf = requestAnimationFrame(frame);

      // Clamped: a window that was occluded and comes back seconds later must not
      // jump the meter to its target in one frame.
      const dt = last === 0 ? 16.7 : Math.min(64, now - last);
      last = now;

      const raw = Math.max(0, Math.min(1, levelRef.current || 0));
      // Rise fast, fall slow, both frame-rate independent.
      meter += (raw - meter) * (1 - Math.exp(-dt / (raw >= meter ? ATTACK_MS : RELEASE_MS)));

      samples.push({ t: now, v: meter });
      while (samples.length > 1 && samples[0].t < now - WINDOW_MS) samples.shift();

      const chase = 1 - Math.exp(-dt / CHASE_MS);
      rects.forEach((r, i) => {
        current[i] += (at(now - DELAY_MS * i) - current[i]) * chase;
        // sqrt approximates perceived loudness, so quiet speech still moves the
        // mark visibly while a loud word does not slam into the ceiling.
        const v = FLOOR + (1 - FLOOR) * Math.sqrt(Math.max(0, current[i]));
        r.style.transform = `scaleY(${Math.min(1, v).toFixed(4)})`;
      });
    };
    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      rects.forEach((r) => { r.style.transform = ""; });
    };
  }, [animate, levelRef]);

  return (
    <svg ref={svg} className="logo" viewBox="80 112 865 800" aria-hidden="true" data-live={animate || undefined}>
      {BARS.map(([x, y, h]) => <rect key={x} x={x} y={y} width="85" height={h} rx="42.5" />)}
    </svg>
  );
}
