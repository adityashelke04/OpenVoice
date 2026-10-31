import { useEffect, useRef, useState } from "react";

/** Completion is keyed to a new delivered result, never to sound preferences. */
export function useFlowConfirmation(state: string, outcome: string | null, session: number | null) {
  const shown = useRef<number | null | undefined>(state === "idle" && outcome === "delivered" ? session : undefined);
  const [confirm, setConfirm] = useState(false);
  useEffect(() => {
    setConfirm(false);
    if (state !== "idle") return;
    if (outcome !== "delivered" || shown.current === session) return;
    shown.current = session;
    setConfirm(true);
    const timer = window.setTimeout(() => setConfirm(false), 480);
    return () => window.clearTimeout(timer);
  }, [state, outcome, session]);
  return state === "idle" && confirm;
}
