/** The clock is the Hub's single largest cost during dictation, so it is the one
 *  thing here worth a test of its own.
 *
 *  While listening, the hook used to tick `elapsedMs` into state five times a
 *  second for every caller. Only the Flow Bar shows a clock; the Hub does not,
 *  and paid for it with a full re-render of the window — sidebar, top bar and the
 *  whole history list — 5x/sec, measured at 60-120ms of blocked main thread per
 *  tick with a normal history. These assert the timer is opt-in and still works
 *  for the window that asked. */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { useLiveEngine } from "./useLiveEngine";
import { installTauri } from "../test/tauri";

let tauri: ReturnType<typeof installTauri>;

const READY = { state: "ready", model: "Standard", device: "CPU", shortcut: "Right Ctrl" };

beforeEach(() => {
  tauri = installTauri({ get_status: () => READY });
});
afterEach(() => {
  // Before the bridge goes, or React's own cleanup unlistens against nothing.
  cleanup();
  tauri.uninstall();
  vi.useRealTimers();
});

/** Mount, wait for the subscription, then put the engine into `listening`. */
async function listening(options?: { clock?: boolean }) {
  const hook = renderHook(() => useLiveEngine(options));
  await waitFor(() => expect(tauri.calls.some((c) => c.args?.event === "ov://event")).toBe(true));
  await act(async () => {
    tauri.emit("ov://event", { type: "Listening", session: 1, profile: "default" });
  });
  expect(hook.result.current.view.state).toBe("listening");
  return hook;
}

describe("useLiveEngine", () => {
  it("keeps the level out of state", async () => {
    const hook = await listening();
    const before = hook.result.current.view;
    await act(async () => {
      tauri.emit("ov://event", { type: "Level", rms: 0.4, peak: 0.6, elapsedMs: 0 });
    });
    expect(hook.result.current.levelRef.current).toBe(0.4);
    expect(hook.result.current.view).toBe(before);
  });

  it("ticks no clock for a caller that did not ask for one", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const hook = await listening();
    const before = hook.result.current.view;
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(hook.result.current.view).toBe(before);
    expect(hook.result.current.view.elapsedMs).toBe(0);
  });

  it("ticks the clock for a caller that asked", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const hook = await listening({ clock: true });
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(hook.result.current.view.elapsedMs).toBeGreaterThan(0);
  });

  it("stops the clock when listening stops", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const hook = await listening({ clock: true });
    await act(async () => {
      tauri.emit("ov://event", { type: "Idle" });
    });
    const before = hook.result.current.view;
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(hook.result.current.view).toBe(before);
  });
});
