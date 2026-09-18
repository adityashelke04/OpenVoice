import { useRef, type KeyboardEvent, type PointerEvent } from "react";

/** Reference §6 `.slider`, made real: a stepped slider that only lands on `stops`
 *  (Maximum recording has three meaningful values, not a continuum). The fill and
 *  thumb sit at `(value - min) / (max - min)` on a linear track, so uneven stops
 *  keep their honest positions. ArrowLeft/Down and ArrowRight/Up step one stop,
 *  Home/End jump to the ends; a press or drag snaps to the nearest stop. */
export function Slider({ value, stops, min, max, format, label, onChange }: {
  value: number; stops: number[]; min: number; max: number; format: (v: number) => string; label: string; onChange: (v: number) => void;
}) {
  const track = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const pct = `${((value - min) / (max - min)) * 100}%`;

  const nearestIndex = (v: number) =>
    stops.reduce((best, s, i) => (Math.abs(s - v) < Math.abs(stops[best] - v) ? i : best), 0);

  const set = (v: number) => { if (v !== value) onChange(v); };

  const onKey = (e: KeyboardEvent) => {
    const i = nearestIndex(value);
    const next = e.key === "ArrowRight" || e.key === "ArrowUp" ? Math.min(stops.length - 1, i + 1)
      : e.key === "ArrowLeft" || e.key === "ArrowDown" ? Math.max(0, i - 1)
      : e.key === "Home" ? 0
      : e.key === "End" ? stops.length - 1
      : -1;
    if (next < 0) return;
    e.preventDefault();
    set(stops[next]);
  };

  const fromPointer = (clientX: number) => {
    const r = track.current?.getBoundingClientRect();
    if (!r || r.width === 0) return;
    const f = Math.min(1, Math.max(0, (clientX - r.left) / r.width));
    set(stops[nearestIndex(min + f * (max - min))]);
  };

  const down = (e: PointerEvent<HTMLDivElement>) => {
    dragging.current = true;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    fromPointer(e.clientX);
  };
  const move = (e: PointerEvent<HTMLDivElement>) => { if (dragging.current) fromPointer(e.clientX); };
  const up = () => { dragging.current = false; };

  return (
    <div
      className="slider"
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={format(value)}
      onKeyDown={onKey}
    >
      <div className="tr" ref={track} onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up}>
        <b style={{ width: pct }} />
        <i style={{ left: pct }} />
      </div>
      <span className="val" aria-hidden>{format(value)}</span>
    </div>
  );
}
