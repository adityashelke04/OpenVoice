/** Settings shared by the Hub and Overlay, updated only after persistence. Moved here from screens/Settings.tsx so the shell, not one screen,
 *  owns it; screens/Settings.tsx re-exports it for the Flow Bar. */
import { useCallback, useEffect, useRef, useState } from "react";
import { loadSettings, saveSettings, restartReasons, type Settings as S } from "../engine/settings";

export function useSettings() {
  const [settings, setSettings] = useState<S | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const current = useRef<S | null>(null);
  const queue = useRef(Promise.resolve());
  const revision = useRef(0);
  const accept = useCallback((s: S) => {
    if ((s.revision ?? 0) < (current.current?.revision ?? 0)) return;
    current.current = s;
    setSettings(s);
  }, []);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let live = true;
    let stop: (() => void) | undefined;
    void import("@tauri-apps/api/event").then(({ listen }) => listen<S>("settings-saved", e => {
      revision.current++;
      accept(e.payload);
    })).then(unlisten => { if (live) stop = unlisten; else unlisten(); });
    return () => { live = false; stop?.(); };
  }, [accept]);

  // Retried, not fired once. The Hub's webview can reach IPC before the Rust
  // side has registered its state, and that first call rejects; a single
  // attempt left every screen but Home on skeletons until the app restarted.
  useEffect(() => {
    let live = true;
    let id = 0;
    let delay = 150;
    const attempt = () => {
      const mine = revision.current;
      loadSettings().then(
        (s) => { if (live && s && mine === revision.current) accept(s); },
        () => {
          if (!live) return;
          id = window.setTimeout(attempt, delay);
          delay = Math.min(delay * 2, 2000);
        },
      );
    };
    attempt();
    return () => { live = false; clearTimeout(id); };
  }, [accept]);

  // Queue edits, then apply each to the latest persisted document. A slow reply
  // cannot overwrite a newer edit or restore a value that was saved on disk.
  const patch = useCallback((fn: (s: S) => void): Promise<boolean> => {
    setSaving(true);
    const task = queue.current.then(async () => {
      if (!current.current) return false;
      setError(null);
      try {
        const latest = await loadSettings() ?? current.current;
        const next = structuredClone(latest);
        fn(next);
        const saved = await saveSettings(next);
        if (!saved) throw new Error("Settings service unavailable");
        revision.current++;
        accept(saved);
        const reasons = await restartReasons().catch(() => null);
        if (reasons?.length) setError(`Saved. Restart OpenVoice to apply: ${reasons.join(", ")}.`);
        return true;
      } catch (e) {
        setError(`Could not save settings. Check disk space and folder permissions, then retry. ${String(e)}`);
        return false;
      }
    });
    const tail = task.then(() => {});
    queue.current = tail;
    void tail.then(() => { if (queue.current === tail) setSaving(false); });
    return task;
  }, [accept]);

  return { settings, patch, saving, error: error ?? settings?.load_warning ?? null };
}
