/** The Ctrl K command palette (spec 6.10): jump to a screen, act on the last
 *  dictation, change theme or mode, open the data folder, or search history
 *  without leaving whatever screen is on.
 *
 *  `cmdk`'s `Command.Dialog` supplies the modal semantics (focus trap,
 *  `aria-modal`, restoring focus to whatever was focused before it opened) on
 *  top of Radix Dialog. Its own `open` prop is fixed `true` for as long as this
 *  component is in the tree at all: whether it is in the tree is instead
 *  decided by this component's `open` prop, gated by `AnimatePresence`, so
 *  closing plays the 160 ms exit animation before Radix's unmount (and the
 *  focus-return that comes with it) runs. `shouldFilter` is off: "Go to" and
 *  "Actions" are filtered by hand against the typed text so the match is a
 *  plain, predictable substring rather than cmdk's fuzzy score, and "History"
 *  is never filtered client-side at all — it already is the backend's answer.
 *
 *  The Escape that closes the palette calls `preventDefault()` before Radix's
 *  own document-level Escape handling, or any window listener such as
 *  HistoryView's "Escape goes back to Home", can see it: one keypress must
 *  close one thing, not the palette and whatever screen sits behind it.
 */
import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Command } from "cmdk";
import { AnimatePresence, motion } from "motion/react";
import {
  ClockCounterClockwise, Copy, Desktop, FolderOpen, Moon, Palette, Sun,
} from "@phosphor-icons/react";
import { AppChip } from "./ui";
import { getHistory } from "./api";
import { NAV, type ScreenId } from "./nav";
import { copyText } from "./home/useCopyPaste";
import { openDataDir } from "../engine/settings";
import { rowKey } from "./history";
import { clockTime } from "./time";
import { THEMES, useTheme, type ThemeName } from "./theme";
import type { Row } from "../engine/stats";
import "./palette.css";

const HISTORY_DEBOUNCE_MS = 120;
const HISTORY_MIN_CHARS = 2;
const HISTORY_LIMIT = 8;

const THEME_LABEL: Record<ThemeName, string> = { glacier: "Glacier", graphite: "Graphite", lagoon: "Lagoon" };

export interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  onNavigate: (id: ScreenId) => void;
  lastRow?: Row;
  onCopyLast: () => void;
}

export function CommandPalette({ open, onClose, onNavigate, lastRow, onCopyLast }: CommandPaletteProps) {
  const { setTheme, setMode } = useTheme();
  const [query, setQuery] = useState("");
  const [historyRows, setHistoryRows] = useState<Row[]>([]);
  // The cmdk-highlighted item's value, for Ctrl+Enter. cmdk only calls
  // `onValueChange` when `value` is controlled (its own setState skips the
  // callback otherwise), so this has to be state, not a passive ref.
  const [activeValue, setActiveValue] = useState("");
  // cmdk's Dialog cannot forward `onCloseAutoFocus` to Radix's own Content (it
  // only takes `open`/`onOpenChange` from us), so the return-focus-to-opener
  // half of "Escape closes and returns focus" is done by hand: remember what
  // was focused when the palette opened, and restore it once the exit
  // animation actually finishes (not merely once `open` goes false).
  const opener = useRef<HTMLElement | null>(null);

  // A fresh search every time it opens: reopening the palette on a different
  // screen must not show yesterday's query or its stale results.
  useEffect(() => {
    if (!open) return;
    opener.current = document.activeElement as HTMLElement | null;
    setQuery("");
    setHistoryRows([]);
    setActiveValue("");
  }, [open]);

  useEffect(() => {
    const q = query.trim();
    if (q.length < HISTORY_MIN_CHARS) {
      setHistoryRows([]);
      return;
    }
    let live = true;
    const id = window.setTimeout(() => {
      getHistory({ query: q, limit: HISTORY_LIMIT }).then((rows) => { if (live) setHistoryRows(rows); }, () => {});
    }, HISTORY_DEBOUNCE_MS);
    return () => { live = false; clearTimeout(id); };
  }, [query]);

  const q = query.trim().toLowerCase();
  const matches = (label: string) => !q || label.toLowerCase().includes(q);
  const select = (fn: () => void) => { fn(); onClose(); };

  interface Entry { value: string; label: string; icon?: ReactNode; hint?: string; onSelect: () => void }

  // Plain arrays, not inline JSX conditionals: with `shouldFilter` off, cmdk's
  // own "hide a group with nothing in it" check never runs (it depends on the
  // client-side filter this palette does not use), so a group that would be
  // empty must never be rendered at all rather than rendered with no items
  // under its heading.
  const goItems: Entry[] = [
    ...NAV.map(({ id, label, Icon, key }): Entry => ({
      value: `go:${id}`, label, icon: <Icon aria-hidden />, hint: `Ctrl ${key}`, onSelect: () => onNavigate(id),
    })),
    { value: "go:history", label: "History", icon: <ClockCounterClockwise aria-hidden />, onSelect: () => onNavigate("history") },
  ].filter((i) => matches(i.label));

  const actionItems: Entry[] = [
    ...(lastRow ? [{ value: "action:copy-last", label: "Copy last dictation", icon: <Copy aria-hidden />, onSelect: onCopyLast }] : []),
    ...THEMES.map((t): Entry => ({
      value: `action:theme-${t}`, label: `Theme: ${THEME_LABEL[t]}`, icon: <Palette aria-hidden />, onSelect: () => setTheme(t),
    })),
    { value: "action:mode-light", label: "Light", icon: <Sun aria-hidden />, onSelect: () => setMode("light") },
    { value: "action:mode-dark", label: "Dark", icon: <Moon aria-hidden />, onSelect: () => setMode("dark") },
    { value: "action:mode-system", label: "Match Windows", icon: <Desktop aria-hidden />, onSelect: () => setMode("system") },
    { value: "action:open-data-dir", label: "Open data folder", icon: <FolderOpen aria-hidden />, onSelect: () => void openDataDir().catch(() => {}) },
  ].filter((i) => matches(i.label));

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
      return;
    }
  };

  return (
    <AnimatePresence onExitComplete={() => { opener.current?.focus?.(); opener.current = null; }}>
      {open && (
        <Command.Dialog
          open
          onOpenChange={(o) => { if (!o) onClose(); }}
          label="Command palette"
          shouldFilter={false}
          value={activeValue}
          onValueChange={setActiveValue}
          onKeyDown={onKeyDown}
          overlayClassName="palette-overlay"
          contentClassName="palette-wrap"
        >
          <motion.div
            className="palette glass"
            initial={{ opacity: 0, scale: 0.98 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.98 }}
            transition={{ duration: 0.16 }}
          >
            <Command.Input autoFocus value={query} onValueChange={setQuery} placeholder="Type a command or search what you've said" />
            <Command.List>
              <Command.Empty>No results</Command.Empty>

              {goItems.length > 0 && (
                <Command.Group heading="Go to">
                  {goItems.map((i) => (
                    <Command.Item key={i.value} value={i.value} onSelect={() => select(i.onSelect)}>
                      {i.icon}
                      {i.label}
                      {i.hint && <span className="hint">{i.hint}</span>}
                    </Command.Item>
                  ))}
                </Command.Group>
              )}

              {actionItems.length > 0 && (
                <Command.Group heading="Actions">
                  {actionItems.map((i) => (
                    <Command.Item key={i.value} value={i.value} onSelect={() => select(i.onSelect)}>
                      {i.icon}
                      {i.label}
                    </Command.Item>
                  ))}
                </Command.Group>
              )}

              {q.length >= HISTORY_MIN_CHARS && historyRows.length > 0 && (
                <Command.Group heading="History">
                  {historyRows.map((row) => (
                    <Command.Item
                      key={rowKey(row)}
                      value={`hist:${rowKey(row)}`}
                      onSelect={() => select(() => void copyText(row.final_text))}
                    >
                      <time>{clockTime(row.created_at)}</time>
                      <span className={row.profile === "terminal" ? "p-text code" : "p-text"}>{row.final_text}</span>
                      <AppChip exe={row.target_app} profile={row.profile} />
                    </Command.Item>
                  ))}
                </Command.Group>
              )}
            </Command.List>
          </motion.div>
        </Command.Dialog>
      )}
    </AnimatePresence>
  );
}
