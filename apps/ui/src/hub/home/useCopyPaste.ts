/** Copy and Fix a word: what you can do with a dictation, on the Last
 *  dictation card and in every expanded Earlier row.
 *
 *  Copy answers in place (spec 4.8): the label becomes "Copied" with a check
 *  for 1400 ms, because a copy that says nothing is indistinguishable from one
 *  that failed. Only an outright failure raises a toast.
 *
 *  Paste again lived here until the owner dropped it (2026-09-19); Copy is now
 *  the one way to get text back out. */
import { useCallback, useEffect, useRef, useState } from "react";
import { pushToast } from "../toast";

export const MORPH_MS = 1400;

export interface CopyPaste {
  copied: boolean;
  copy: () => Promise<void>;
}

/** Copy a dictation to the clipboard. Shared by the hook below and by the
 *  command palette's History results, which act on whichever row is
 *  highlighted rather than one fixed `text` a hook could own. */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch (e) {
    pushToast({ tone: "danger", message: String(e) });
    throw e;
  }
}

export function useCopyPaste(text: string): CopyPaste {
  const [copied, setCopied] = useState(false);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  useEffect(() => () => timers.current.forEach(clearTimeout), []);

  const flash = useCallback((set: (v: boolean) => void) => {
    set(true);
    timers.current.push(setTimeout(() => set(false), MORPH_MS));
  }, []);

  const copy = useCallback(async () => {
    try {
      await copyText(text);
      flash(setCopied);
    } catch {
      // copyText already pushed the toast.
    }
  }, [text, flash]);

  return { copied, copy };
}
