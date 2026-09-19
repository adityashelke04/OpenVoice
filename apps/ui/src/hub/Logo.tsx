/** The OpenVoice mark: seven rounded bars (spec 4.2, reference lines 412-420).
 *
 *  Geometry from assets/icon-source.png on its 1024 grid. Colour comes from CSS
 *  (`--brand`, `--brand-on-light`) and is never themed.
 *
 *  While the microphone is open the bars follow the live level, each one 40 ms
 *  behind its left neighbour, so the mark reads as a waveform travelling across
 *  it. A ring buffer of the last 400 ms of samples feeds that delay. The loop
 *  writes `style.transform` directly and never touches React state, and it only
 *  exists while listening: idle, the mark is a plain static SVG (idle CPU ~ 0). */
import { useEffect, useRef } from "react";

const BARS: [x: number, y: number, h: number][] = [
  [80, 424, 177], [210, 327, 370], [340, 160, 704], [470, 263, 498],
  [600, 112, 800], [730, 359, 306], [860, 439, 146],
];
const DELAY_MS = 40, WINDOW_MS = 400;

export function Logo({ levelRef, listening }: { levelRef: { readonly current: number }; listening: boolean }) {
  const svg = useRef<SVGSVGElement>(null);

  useEffect(() => {
    const rects = svg.current ? [...svg.current.querySelectorAll("rect")] : [];
    if (!listening) return;
    const samples: { t: number; v: number }[] = [];
    let raf = 0;
    const at = (t: number) => {
      // Newest sample no later than t; the oldest one if the buffer is younger.
      for (let i = samples.length - 1; i >= 0; i--) if (samples[i].t <= t) return samples[i].v;
      return samples[0]?.v ?? 0;
    };
    const frame = (now: number) => {
      samples.push({ t: now, v: Math.max(0, Math.min(1, levelRef.current || 0)) });
      while (samples.length > 1 && samples[0].t < now - WINDOW_MS) samples.shift();
      rects.forEach((r, i) => { r.style.transform = `scaleY(${0.35 + 0.65 * at(now - DELAY_MS * i)})`; });
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      rects.forEach((r) => { r.style.transform = ""; });
    };
  }, [listening, levelRef]);

  return (
    <svg ref={svg} className="logo" viewBox="80 112 865 800" aria-hidden="true" data-live={listening || undefined}>
      {BARS.map(([x, y, h]) => <rect key={x} x={x} y={y} width="85" height={h} rx="42.5" />)}
    </svg>
  );
}
