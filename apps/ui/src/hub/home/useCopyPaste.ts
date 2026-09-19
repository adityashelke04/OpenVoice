/** Copy, Paste again and Fix a word: the three things you can do with a
 *  dictation, on the Last dictation card and in every expanded Earlier row.
 *
 *  Copy and Paste again answer in place (spec 4.8): the label becomes "Copied"
 *  or "Pasted" with a check for 1400 ms. A copy that says nothing is
 *  indistinguishable from one that failed. When Paste again cannot reach the
 *  app it leaves the text on the clipboard and the backend says so in its own
 *  notice, so the label stays and nothing more is said here. Only an outright
 *  failure raises a toast. */
import { useCallback, useEffect, useRef, useState } from "react";
import { pasteAgain } from "../api";
import { pushToast } from "../toast";

export const MORPH_MS = 1400;

export interface CopyPaste {
  copied: boolean;
  pasted: boolean;
  copy: () => Promise<void>;
  paste: () => Promise<void>;
}

export function useCopyPaste(text: string): CopyPaste {
  const [copied, setCopied] = useState(false);
  const [pasted, setPasted] = useState(false);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  useEffect(() => () => timers.current.forEach(clearTimeout), []);

  const flash = useCallback((set: (v: boolean) => void) => {
    set(true);
    timers.current.push(setTimeout(() => set(false), MORPH_MS));
  }, []);

  const copy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(text);
      flash(setCopied);
    } catch (e) {
      pushToast({ tone: "danger", message: String(e) });
    }
  }, [text, flash]);

  const paste = useCallback(async () => {
    try {
      if ((await pasteAgain(text)) === "pasted") flash(setPasted);
    } catch (e) {
      pushToast({ tone: "danger", message: String(e) });
    }
  }, [text, flash]);

  return { copied, pasted, copy, paste };
}
