import { useEffect, useRef } from "react";
import type { EngineEvent } from "../engine/types";
import { playFlowSound, preloadFlowSounds, silenceFlowSounds, type FlowSound } from "../ui/sound";

/** Consume events before React batches them, without acknowledging older queued work. */
export function createSoundLifecycle(): (event: EngineEvent) => FlowSound | null {
  let session = -1;
  let phase: "listening" | "working" | "finished" = "finished";
  return (event) => {
    if (event.type === "Listening" && event.session > session) {
      session = event.session;
      phase = "listening";
      return "start";
    }
    if (!("session" in event) || event.session !== session) return null;
    if (event.type === "Transcribing" && phase === "listening") {
      phase = "working";
      return "stop";
    }
    if (event.type !== "Finished" || phase === "finished") return null;
    phase = "finished";
    switch (event.outcome.kind) {
      case "delivered": return "success";
      case "cancelled":
      case "too_short":
      case "silent": return "cancel";
      default: return "error";
    }
  };
}

/** The overlay is the sole sound owner; never mount this in the Hub. */
export function useFlowSounds(enabled: boolean, latched = false) {
  const soundEnabled = useRef(enabled);
  soundEnabled.current = enabled;
  useEffect(() => {
    if (enabled) void preloadFlowSounds();
    else silenceFlowSounds();
  }, [enabled]);
  useEffect(() => silenceFlowSounds, []);
  const wasLatched = useRef(latched);
  useEffect(() => {
    const entering = latched && !wasLatched.current;
    wasLatched.current = latched;
    if (entering && enabled) void playFlowSound("lock");
  }, [enabled, latched]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const cueFor = createSoundLifecycle();
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      if (disposed) return;
      const stop = await listen<EngineEvent>("ov://event", ({ payload }) => {
        if (disposed) return;
        const cue = cueFor(payload);
        if (cue && soundEnabled.current) void playFlowSound(cue);
      });
      if (disposed) stop();
      else unlisten = stop;
    }).catch(() => { /* Sound must never interrupt dictation. */ });
    return () => { disposed = true; unlisten?.(); };
  }, []);
}
