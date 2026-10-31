import { useEffect, useRef } from "react";
import type { CSSProperties } from "react";

/** Wispr's measured gain and smoothing, normalized for the display refresh rate. */
export function flowAmplitude(previous: number, sample: number, dt: number) {
  const target = Number.isFinite(sample) ? Math.min(1, Math.max(0, sample)) : 0;
  return previous + (target - previous) * (1 - Math.pow(.85, Math.max(0, dt) / (1000 / 60)));
}

/** Ten instantaneous voice bars. No history-fill delay and no React frame updates. */
export function FlowWave({ level = 0, levelRef, active, publish }: {
  level?: number;
  levelRef?: { current: number };
  active: boolean;
  publish?: { current: HTMLElement | null };
}) {
  const host = useRef<HTMLDivElement>(null);
  const latest = useRef({ level, levelRef, publish });
  latest.current = { level, levelRef, publish };
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    if (!active) { el.style.setProperty("--amplitude", "0"); return; }
    let raf = 0, last = 0, amplitude = 0;
    const tick = (now: number) => {
      const source = latest.current;
      amplitude = flowAmplitude(amplitude, normalizeFlowRms(source.levelRef?.current ?? source.level), last ? Math.min(64, now - last) : 1000 / 60);
      last = now;
      el.style.setProperty("--amplitude", amplitude.toFixed(4));
      source.publish?.current?.style.setProperty("--level", amplitude.toFixed(4));
      raf = requestAnimationFrame(tick);
    };
    const visibility = () => {
      cancelAnimationFrame(raf);
      last = 0;
      if (!document.hidden) raf = requestAnimationFrame(tick);
    };
    visibility();
    document.addEventListener("visibilitychange", visibility);
    return () => {
      document.removeEventListener("visibilitychange", visibility);
      cancelAnimationFrame(raf);
      latest.current.publish?.current?.style.removeProperty("--level");
    };
  }, [active]);
  return <div className="wave flow-wave" ref={host} data-active={active} aria-hidden>
    {Array.from({ length: 10 }, (_, i) => <span className="wave-bar" key={i} style={{
      "--shape": 1 - Math.pow(i - 4.5, 2) / 48,
      "--delay": `${i < 5 ? i / 10 : (i - 10) / 10}s`,
    } as CSSProperties}><i /></span>)}
  </div>;
}

/** OpenVoice emits linear RMS; the reference renderer expects normalized dB.
 * A fixed -60..-12 dBFS window keeps quiet speech responsive without learning a
 * silent packet as an excessively low floor and saturating ordinary speech. */
export function normalizeFlowRms(rms: number) {
  if (!Number.isFinite(rms) || rms <= 0) return 0;
  return Math.min(1, Math.max(0, (20 * Math.log10(rms) + 60) / 48));
}
