/** The Hub's global shortcuts (spec 6.1): Ctrl+1..6 switch sections in sidebar
 *  order, Ctrl+K opens the palette, Escape closes whatever is open. Keys are
 *  matched by position (`e.code`), so they work on any keyboard layout.
 *
 *  Ctrl+digit is ignored mid-composition: an IME (Japanese, Chinese, Korean)
 *  uses digits to pick candidates, and jumping screens while the user is choosing
 *  a word would throw away what they were typing. */
import { useEffect, useRef } from "react";
import { NAV, type ScreenId } from "./nav";

export interface HubKeys {
  onScreen: (id: ScreenId) => void;
  onPalette: () => void;
  onEscape: () => void;
}

export function useHubKeys(handlers: HubKeys) {
  // Latest handlers without re-binding the listener on every render.
  const ref = useRef(handlers);
  ref.current = handlers;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing || e.keyCode === 229) return;
      if (e.key === "Escape") { ref.current.onEscape(); return; }
      // Holding the chord must not re-fire it (the palette would reopen, or a
      // screen re-enter, once per auto-repeat).
      if (e.repeat || !e.ctrlKey || e.altKey || e.metaKey || e.shiftKey) return;
      // By physical key (`code`), not by character (`key`): on AZERTY the top
      // row types & é " ' without Shift, so Ctrl+"1" never arrives as "1". The
      // numpad is not the top row and is left alone.
      if (e.code === "KeyK") {
        e.preventDefault();
        ref.current.onPalette();
        return;
      }
      const n = NAV.find((x) => e.code === `Digit${x.key}`);
      if (n) {
        e.preventDefault();
        ref.current.onScreen(n.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
