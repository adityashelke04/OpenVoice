import { useEffect, useRef } from "react";

/** Reference `.meter`: six bars beside the microphone picker, lit by the live
 *  input level so choosing a device can be checked by speaking into it.
 *
 *  The level is read from a ref and written straight to the bars' class lists,
 *  never through React state: it arrives about thirty times a second, and
 *  rendering the Settings screen that often was what made the old meter the most
 *  expensive thing in an idle window.
 *
 *  It costs one animation frame on mount (the level can land after this mounts,
 *  so reading it synchronously would show an empty meter), and then nothing at
 *  all until the microphone is actually open. No interval, no idle loop. */
const HEIGHTS = [6, 10, 15, 12, 8, 5];

export function MicMeter({ levelRef, listening }: {
  levelRef?: { readonly current: number };
  listening?: boolean;
}) {
  const meter = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const bars = meter.current ? [...meter.current.children] : [];
    let raf = 0;
    const frame = () => {
      // Perceptual, not linear: a square root spreads speech across the bars
      // instead of leaving the top half dark until someone shouts.
      const level = Math.min(1, Math.max(0, levelRef?.current ?? 0));
      const lit = Math.round(Math.sqrt(level) * bars.length);
      bars.forEach((b, i) => b.classList.toggle("off", i >= lit));
      if (listening) raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [levelRef, listening]);

  return (
    <div className="meter" ref={meter} aria-hidden="true">
      {HEIGHTS.map((h) => <i key={h} className="off" style={{ height: h }} />)}
    </div>
  );
}
