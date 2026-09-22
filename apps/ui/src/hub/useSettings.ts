/** Settings as the Hub edits them: loaded once, patched optimistically, then
 *  reconciled with what the Rust store actually wrote (it validates and can
 *  reject). Moved here from screens/Settings.tsx so the shell, not one screen,
 *  owns it; screens/Settings.tsx re-exports it for the Flow Bar. */
import { useCallback, useEffect, useState } from "react";
import { loadSettings, saveSettings, type Settings as S } from "../engine/settings";

export function useSettings() {
  const [settings, setSettings] = useState<S | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadSettings().then((s) => s && setSettings(s));
  }, []);

  // Stable identity, or memoized rows re-render on every Home render anyway.
  const patch = useCallback(async (fn: (s: S) => void) => {
    if (!settings) return;
    // Optimistic, then reconciled with whatever the store actually wrote — the
    // Rust side validates and can reject.
    const next = structuredClone(settings);
    fn(next);
    setSettings(next);
    setSaving(true);
    setError(null);
    try {
      const saved = await saveSettings(next);
      if (saved) setSettings(saved);
    } catch (e) {
      setError(String(e));
      setSettings(settings);
    } finally {
      setSaving(false);
    }
  }, [settings]);

  return { settings, patch, saving, error };
}
