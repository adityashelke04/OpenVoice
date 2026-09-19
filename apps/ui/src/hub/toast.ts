/** A tiny toast store for the Hub.
 *
 *  Engine notices ("Nothing to paste yet", a clipboard fallback) used to sit as a
 *  banner at the top of the main pane, which pushed every screen down by a card
 *  and stayed until dismissed. They are transient facts, so they now arrive as a
 *  toast in the corner and leave by themselves after five seconds. Module state
 *  plus useSyncExternalStore: any component can push, one <Toasts> renders. */
import { useSyncExternalStore } from "react";

export type ToastTone = "info" | "warn" | "danger";
export interface Toast { id: number; tone: ToastTone; message: string }

export const TOAST_MS = 5000;

let toasts: Toast[] = [];
let nextId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();
const subscribers = new Set<() => void>();
const notify = () => subscribers.forEach((f) => f());

export function pushToast(t: { tone: ToastTone; message: string }): number {
  const id = nextId++;
  toasts = [...toasts, { id, ...t }];
  timers.set(id, setTimeout(() => dismissToast(id), TOAST_MS));
  notify();
  return id;
}

export function dismissToast(id: number) {
  const timer = timers.get(id);
  if (timer !== undefined) clearTimeout(timer);
  timers.delete(id);
  if (!toasts.some((t) => t.id === id)) return;
  toasts = toasts.filter((t) => t.id !== id);
  notify();
}

function subscribe(f: () => void) {
  subscribers.add(f);
  return () => { subscribers.delete(f); };
}

/** The toasts showing right now, outside React (tests, and code that must not subscribe). */
export const getToasts = (): readonly Toast[] => toasts;

export function useToasts(): Toast[] {
  return useSyncExternalStore(subscribe, () => toasts, () => toasts);
}
