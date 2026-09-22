import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

afterEach(() => cleanup());

// jsdom has no matchMedia; tests override `matches` per query through this map.
export const media: Record<string, boolean> = {};
const listeners = new Map<string, Set<(e: { matches: boolean }) => void>>();
window.matchMedia = ((query: string) => ({
  get matches() { return media[query] ?? false; },
  media: query,
  addEventListener: (_: string, f: (e: { matches: boolean }) => void) => { (listeners.get(query) ?? listeners.set(query, new Set()).get(query)!).add(f); },
  removeEventListener: (_: string, f: (e: { matches: boolean }) => void) => { listeners.get(query)?.delete(f); },
  onchange: null, addListener() {}, removeListener() {}, dispatchEvent: () => false,
})) as unknown as typeof window.matchMedia;
export function setMedia(query: string, matches: boolean) {
  media[query] = matches;
  listeners.get(query)?.forEach((f) => f({ matches }));
}

Object.defineProperty(navigator, "clipboard", { value: { writeText: vi.fn(async () => {}) }, configurable: true });
globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver;
// jsdom has no layout, so cmdk's own scroll-selected-item-into-view calls throw.
Element.prototype.scrollIntoView ??= () => {};
