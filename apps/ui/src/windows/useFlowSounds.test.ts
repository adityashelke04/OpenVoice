import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { EngineEvent, Outcome } from "../engine/types";
import { installTauri } from "../test/tauri";
import { createSoundLifecycle, useFlowSounds } from "./useFlowSounds";

const audio = vi.hoisted(() => ({ play: vi.fn(), silence: vi.fn(), preload: vi.fn() }));
vi.mock("../ui/sound", () => ({ playFlowSound: audio.play, silenceFlowSounds: audio.silence, preloadFlowSounds: audio.preload }));

const start = (session = 1): EngineEvent => ({ type: "Listening", session, profile: "prose" });
const stop = (session = 1): EngineEvent => ({ type: "Transcribing", session, audioMs: 1000 });
const finish = (kind: Outcome["kind"], session = 1): EngineEvent => ({
  type: "Finished", session, outcome: { kind, detail: "" } as Outcome, text: "", latencyMs: 100,
});

describe("sound lifecycle", () => {
  it("acknowledges recording edges once, even in a single event batch", () => {
    const cue = createSoundLifecycle();
    expect([start(), start(), stop(), stop()].map(cue)).toEqual(["start", null, "stop", null]);
  });

  it("does not congratulate discarded or failed dictation", () => {
    for (const kind of ["cancelled", "too_short", "silent", "asr_failed", "capture_failed", "clipboard_fallback", "delivery_held"] as const) {
      const cue = createSoundLifecycle();
      cue(start());
      expect(cue(finish(kind))).toBe(kind === "cancelled" || kind === "too_short" || kind === "silent" ? "cancel" : "error");
      expect(cue(finish(kind))).toBeNull();
    }
  });

  it("only confirms a delivered session that this listener saw start", () => {
    const cue = createSoundLifecycle();
    expect(cue(finish("delivered"))).toBeNull();
    cue(start());
    cue(stop());
    expect(cue(finish("delivered"))).toBe("success");
    expect(cue(finish("delivered"))).toBeNull();
  });

  it("ignores older queued sessions while a newer microphone session is open", () => {
    const cue = createSoundLifecycle();
    cue(start(1));
    cue(stop(1));
    expect(cue(start(2))).toBe("start");
    expect(cue(finish("delivered", 1))).toBeNull();
    expect(cue(stop(1))).toBeNull();
    expect(cue(start(1))).toBeNull();
    expect(cue(stop(2))).toBe("stop");
  });
});

let bridge: ReturnType<typeof installTauri> | undefined;
afterEach(() => { cleanup(); bridge?.uninstall(); bridge = undefined; vi.clearAllMocks(); });

it("keeps muted events consumed and never replays them when sound is enabled", async () => {
  bridge = installTauri();
  const hook = renderHook(({ enabled }) => useFlowSounds(enabled), { initialProps: { enabled: false } });
  await waitFor(() => expect(bridge!.calls.some(c => c.args?.event === "ov://event")).toBe(true));
  act(() => bridge!.emit("ov://event", start()));
  hook.rerender({ enabled: true });
  expect(audio.play).not.toHaveBeenCalled();
  act(() => bridge!.emit("ov://event", stop()));
  expect(audio.play.mock.calls).toEqual([["stop"]]);
  hook.unmount();
  act(() => bridge!.emit("ov://event", finish("delivered")));
  expect(audio.play.mock.calls).toEqual([["stop"]]);
});

it("acknowledges entering hands-free once, without replaying on mount or unmute", () => {
  const hook = renderHook(({ enabled, latched }) => useFlowSounds(enabled, latched), {
    initialProps: { enabled: true, latched: true },
  });
  expect(audio.play).not.toHaveBeenCalled();
  hook.rerender({ enabled: true, latched: false });
  hook.rerender({ enabled: true, latched: true });
  expect(audio.play.mock.calls).toEqual([["lock"]]);
  hook.rerender({ enabled: false, latched: false });
  hook.rerender({ enabled: false, latched: true });
  hook.rerender({ enabled: true, latched: true });
  expect(audio.play.mock.calls).toEqual([["lock"]]);
});

it("silences pending or playing cues when muted and when the owner unmounts", () => {
  const hook = renderHook(({ enabled }) => useFlowSounds(enabled), { initialProps: { enabled: true } });
  hook.rerender({ enabled: false });
  expect(audio.silence).toHaveBeenCalledOnce();
  hook.rerender({ enabled: true });
  hook.unmount();
  expect(audio.silence).toHaveBeenCalledTimes(2);
});

it("prepares the local reference pack only after sound feedback is enabled", () => {
  const hook = renderHook(({ enabled }) => useFlowSounds(enabled), { initialProps: { enabled: false } });
  expect(audio.preload).not.toHaveBeenCalled();
  hook.rerender({ enabled: true });
  expect(audio.preload).toHaveBeenCalledOnce();
  expect(audio.play).not.toHaveBeenCalled();
});
