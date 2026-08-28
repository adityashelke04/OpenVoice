import { useEffect, useRef, useState } from "react";

export interface CountUpOptions {
  /** Duration of the count-up animation in milliseconds. Defaults to 700ms. */
  duration?: number;
  /** Number of decimal places to keep. Defaults to 0 (integer). */
  decimals?: number;
  /** Optional initial starting number before the first target is set. Defaults to 0. */
  start?: number;
}

/**
 * Smoothly interpolates numeric values when data updates.
 *
 * Designed for hero metrics and dashboards where sudden integer jumps feel jarring.
 * Features:
 * - Exponential cubic-out easing for natural deceleration.
 * - Handles mid-animation target changes seamlessly without jumping back to zero.
 * - Respects `prefers-reduced-motion` for accessibility.
 * - Clean cleanup on unmount with zero memory leaks.
 */
export function useCountUp(
  target: number,
  options?: number | CountUpOptions,
): number;
export function useCountUp(
  target: number | null | undefined,
  options?: number | CountUpOptions,
): number | null;
export function useCountUp(
  target: number | null | undefined,
  options?: number | CountUpOptions,
): number | null {
  const duration =
    typeof options === "number" ? options : (options?.duration ?? 700);
  const decimals = typeof options === "object" ? (options.decimals ?? 0) : 0;
  const initialStart = typeof options === "object" ? (options.start ?? 0) : 0;

  const [displayValue, setDisplayValue] = useState<number | null>(() => {
    if (target == null || isNaN(target)) return null;
    return initialStart;
  });

  const currentValRef = useRef<number>(initialStart);
  const rafRef = useRef<number>(0);

  useEffect(() => {
    if (target == null || isNaN(target)) {
      setDisplayValue(null);
      currentValRef.current = initialStart;
      return;
    }

    const prefersReducedMotion =
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches;

    if (prefersReducedMotion || duration <= 0) {
      const finalVal =
        decimals > 0 ? Number(target.toFixed(decimals)) : Math.round(target);
      setDisplayValue(finalVal);
      currentValRef.current = target;
      return;
    }

    const startVal = currentValRef.current;
    const targetVal = target;
    const startTime = performance.now();

    const tick = (now: number) => {
      const elapsed = now - startTime;
      const progress = Math.min(1, Math.max(0, elapsed / duration));
      // Cubic ease-out: starts briskly and settles smoothly
      const eased = 1 - Math.pow(1 - progress, 3);
      const current = startVal + (targetVal - startVal) * eased;
      currentValRef.current = current;

      const formatted =
        decimals > 0 ? Number(current.toFixed(decimals)) : Math.round(current);
      setDisplayValue(formatted);

      if (progress < 1) {
        rafRef.current = requestAnimationFrame(tick);
      } else {
        currentValRef.current = targetVal;
      }
    };

    rafRef.current = requestAnimationFrame(tick);

    return () => {
      if (rafRef.current) {
        cancelAnimationFrame(rafRef.current);
      }
    };
  }, [target, duration, decimals, initialStart]);

  return displayValue;
}
