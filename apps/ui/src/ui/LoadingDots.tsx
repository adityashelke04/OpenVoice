/** Kinetic loading indicators.
 *
 * GPU-accelerated harmonic wave loading dots and kinetic ticking ellipsis
 * attached to loading/in-progress messages.
 *
 * Rules and reasoning live in DESIGN.md:
 * - Green appears only on live state or record actions.
 * - Loading and waiting states use neutral/body/warn tones.
 * - GPU composited transforms/opacity only with will-change.
 */

import type { HTMLAttributes, ReactNode } from "react";
import type { Tone } from "./index";
import "./loading-dots.css";

export type LoadingTone = Tone | "body" | "mute" | "ink";
export type LoadingSize = "xs" | "sm" | "md" | "lg";
export type LoadingVariant = "wave" | "bounce" | "pulse" | "fade";

export interface LoadingDotsProps extends HTMLAttributes<HTMLSpanElement> {
  size?: LoadingSize;
  tone?: LoadingTone;
  variant?: LoadingVariant;
  label?: string;
}

/**
 * 3-dot kinetic loading indicator with staggered harmonic wave bounce.
 *
 * Staggered with 0s, 0.16s, 0.32s animation delays and cubic-bezier easing.
 */
export function LoadingDots({
  size = "sm",
  tone = "neutral",
  variant = "wave",
  label = "Loading",
  className = "",
  ...rest
}: LoadingDotsProps) {
  return (
    <span
      className={`loading-dots ${className}`.trim()}
      data-size={size}
      data-tone={tone}
      data-variant={variant}
      role="status"
      aria-label={label}
      {...rest}
    >
      <span className="loading-dot" aria-hidden="true" />
      <span className="loading-dot" aria-hidden="true" />
      <span className="loading-dot" aria-hidden="true" />
    </span>
  );
}

export interface TickingEllipsisProps extends HTMLAttributes<HTMLSpanElement> {
  text?: string;
  tone?: LoadingTone;
  size?: LoadingSize;
  suffix?: ReactNode;
  children?: ReactNode;
}

/**
 * Kinetic ticking dots attached to loading and in-progress messages.
 *
 * Replaces static ellipsis glyphs with animated kinetic dots that
 * step/bounce smoothly in harmonic rhythm.
 */
export function TickingEllipsis({
  text,
  tone,
  size,
  suffix,
  children,
  className = "",
  ...rest
}: TickingEllipsisProps) {
  return (
    <span
      className={`ticking-ellipsis ${className}`.trim()}
      data-tone={tone}
      data-size={size}
      {...rest}
    >
      {text && <span className="ticking-text">{text}</span>}
      <span className="ticking-dots" aria-hidden="true">
        <span className="ticking-dot" />
        <span className="ticking-dot" />
        <span className="ticking-dot" />
      </span>
      {suffix && <span className="ticking-suffix">{suffix}</span>}
      {children}
    </span>
  );
}
