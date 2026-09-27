import type { ReactElement } from "react";

/**
 * The Flow Bar's glyphs, drawn on one 16px grid with one stroke weight.
 *
 * Inline rather than from the Hub's icon package: the overlay is its own bundle
 * entry and is on screen all day, and sixteen paths cost less than pulling an
 * icon library into the window that has to stay lightest.
 */
export type FlowIconName =
  | "clipboard"
  | "warn"
  | "info"
  | "model"
  | "lock"
  | "close"
  | "mic"
  | "record"
  | "stop"
  | "history"
  | "settings"
  | "recenter"
  | "compact"
  | "full"
  | "shrink"
  | "snooze"
  | "eye";

const PATHS: Record<FlowIconName, ReactElement> = {
  clipboard: (
    <>
      <rect x="3.5" y="3" width="9" height="11" rx="2" />
      <path d="M6 3.2V2.5h4v.7" />
    </>
  ),
  warn: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 4.9v3.4" />
      <circle cx="8" cy="10.9" r=".5" fill="currentColor" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 7.4v3.6" />
      <circle cx="8" cy="5.1" r=".5" fill="currentColor" />
    </>
  ),
  model: <path d="M8 2.5v7m0 0L5 6.8M8 9.5l3-2.7M3 12.8h10" />,
  lock: (
    <>
      <rect x="3.5" y="7" width="9" height="6.5" rx="1.8" />
      <path d="M5.5 7V5.3a2.5 2.5 0 0 1 5 0V7" />
    </>
  ),
  close: <path d="M4.5 4.5l7 7m0-7l-7 7" />,
  mic: (
    <>
      <rect x="6" y="2" width="4" height="8" rx="2" />
      <path d="M3.5 8a4.5 4.5 0 0 0 9 0M8 12.5V14" />
    </>
  ),
  record: (
    <>
      <circle cx="8" cy="8" r="5.5" />
      <circle cx="8" cy="8" r="2.4" fill="currentColor" stroke="none" />
    </>
  ),
  stop: <rect x="4.5" y="4.5" width="7" height="7" rx="1.6" />,
  history: (
    <>
      <circle cx="8" cy="8" r="5.5" />
      <path d="M8 5v3l2 1.5" />
    </>
  ),
  settings: (
    <>
      <path d="M3 5h6m3 0h1M3 11h1m3 0h6" />
      <circle cx="10.5" cy="5" r="1.5" />
      <circle cx="5.5" cy="11" r="1.5" />
    </>
  ),
  recenter: (
    <>
      <circle cx="8" cy="8" r="2" />
      <path d="M8 2v2.5M8 11.5V14M2 8h2.5M11.5 8H14" />
    </>
  ),
  compact: <rect x="4.5" y="6.5" width="7" height="3" rx="1.5" />,
  full: <rect x="2" y="5.5" width="12" height="5" rx="2.5" />,
  shrink: <path d="M3 8h10M5.5 5.5L3 8l2.5 2.5M10.5 5.5L13 8l-2.5 2.5" />,
  snooze: (
    <>
      <path d="M12.8 9.6A5.5 5.5 0 1 1 6.4 3.2a4.3 4.3 0 0 0 6.4 6.4z" />
    </>
  ),
  eye: (
    <>
      <path d="M1.8 8S4 3.8 8 3.8 14.2 8 14.2 8 12 12.2 8 12.2 1.8 8 1.8 8z" />
      <circle cx="8" cy="8" r="1.8" />
    </>
  ),
};

export function FlowIcon({ name }: { name: FlowIconName }) {
  return (
    <svg
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
