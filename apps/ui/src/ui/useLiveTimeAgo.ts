import { useEffect, useState } from "react";

/**
 * Converts a millisecond timestamp or Date into a human-readable relative time string.
 *
 * Floor, not round, at every step. Rounding twice makes 55 minutes read "1h ago"
 * and 20 hours read "1d ago" — an elapsed time should never claim more time has
 * passed than actually has.
 */
export function formatTimeAgo(timestamp: number | Date | null | undefined): string {
  if (timestamp == null) return "";
  const ms = typeof timestamp === "number" ? timestamp : timestamp.getTime();
  if (isNaN(ms) || ms <= 0) return "";

  const diffMs = Math.max(0, Date.now() - ms);
  const mins = Math.floor(diffMs / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

/**
 * Hook that produces a relative time string and self-refreshes every `intervalMs` (default 30s)
 * without requiring a full page or component tree reload.
 *
 * @param timestamp - The timestamp in milliseconds or a Date object.
 * @param intervalMs - Polling interval in ms (defaults to 30,000 ms / 30 seconds).
 */
export function useLiveTimeAgo(
  timestamp: number | Date | null | undefined,
  intervalMs = 30_000,
): string {
  const [timeAgo, setTimeAgo] = useState<string>(() => formatTimeAgo(timestamp));

  useEffect(() => {
    // Immediate calculation on timestamp change
    setTimeAgo(formatTimeAgo(timestamp));

    if (timestamp == null) return;

    const timer = window.setInterval(() => {
      setTimeAgo(formatTimeAgo(timestamp));
    }, intervalMs);

    return () => clearInterval(timer);
  }, [timestamp, intervalMs]);

  return timeAgo;
}
