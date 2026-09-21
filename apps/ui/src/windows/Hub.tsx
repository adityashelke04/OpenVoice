/** The Hub window: the shell (reference §2-§3) and a screen router.
 *
 * The backdrop (ambient blobs + grain) is painted once and never moves; Mica
 * replaces it where Windows provides one (html[data-material="mica"], see
 * hub/material.ts, which main.tsx wires before the first paint). Everything the
 * sidebar and top bar show comes from the live engine and one `get_user_name`
 * call; each screen owns its own data.
 *
 * Screens are plain state, not routes: the Hub is one window with no address
 * bar, and the only ways in are the sidebar, Ctrl+1..6, the Flow Bar's
 * `hub-navigate` event and, once at start, `?screen=` (used by the twin harness
 * and handy in dev).
 */

import { useEffect, useState } from "react";
import { MotionConfig } from "motion/react";
import { useLiveEngine } from "../engine/useLiveEngine";
import { DictionaryScreen } from "../screens/Dictionary";
import { ProfilesScreen } from "../screens/Profiles";
import { AdvancedScreen } from "../screens/Advanced";
import { ModelsScreen } from "../screens/Models";
import { SettingsScreen } from "../screens/Settings";
import { getHistory, getUserName } from "../hub/api";
import { isScreenId, type ScreenId } from "../hub/nav";
import { Sidebar, type EngineState } from "../hub/Sidebar";
import { TopBar } from "../hub/TopBar";
import { Toasts } from "../hub/Toasts";
import { pushToast } from "../hub/toast";
import { useHubKeys } from "../hub/useHubKeys";
import { isStill } from "../hub/useMedia";
import { useSettings } from "../hub/useSettings";
import { HomeScreen } from "../hub/home/HomeScreen";
import { HistoryView } from "../hub/home/HistoryView";
import { useCopyPaste } from "../hub/home/useCopyPaste";
import { CommandPalette } from "../hub/CommandPalette";
import type { Row } from "../engine/stats";
import "../hub/shell.css";

export type { ScreenId };

/** The screen to open on: `?screen=<id>` when it names one, else Home. */
export function initialScreen(search: string = typeof location === "undefined" ? "" : location.search): ScreenId {
  const s = new URLSearchParams(search).get("screen");
  return isScreenId(s) ? s : "home";
}

/** `?still=1`: no screen-enter motion at all, for screenshots and the twin. */
const STILL = isStill();

/** The clock Home's "2 min ago" reads: it wakes on each minute boundary, and
 *  only while Home or History is on screen, so no other screen pays for it. */
function useMinuteNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    let id = 0;
    const schedule = () => {
      id = window.setTimeout(() => { setNow(Date.now()); schedule(); }, 60_000 - (Date.now() % 60_000) + 50);
    };
    // Coming back to Home after a while must not show a stale "2 min ago".
    id = window.setTimeout(() => { setNow(Date.now()); schedule(); }, 0);
    return () => clearTimeout(id);
  }, [active]);
  return now;
}

/** The clock the greeting reads. It only has to be right to the hour, so it
 *  wakes once at the top of each hour rather than ticking. */
function useHourlyNow(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const next = new Date(now);
    next.setHours(now.getHours() + 1, 0, 0, 0);
    const id = window.setTimeout(() => setNow(new Date()), Math.max(1000, next.getTime() - now.getTime()));
    return () => clearTimeout(id);
  }, [now]);
  return now;
}

export function Hub() {
  const { view, levelRef, dismissNotice } = useLiveEngine();
  const { settings, patch, error: settingsError } = useSettings();
  const [screen, setScreen] = useState<ScreenId>(() => initialScreen());
  const [userName, setUserName] = useState<string | null>(null);
  const [palette, setPalette] = useState(false);
  // The palette's "Copy last dictation" / "Paste last dictation again": a
  // one-shot read when it opens, not a second history-polling loop next to
  // Home's own.
  const [lastRow, setLastRow] = useState<Row | undefined>(undefined);
  const now = useHourlyNow();
  // History reads it too: its day headings ("Today", "Yesterday") turn over at midnight.
  const minute = useMinuteNow(screen === "home" || screen === "history");

  useEffect(() => {
    let live = true;
    getUserName().then((n) => live && setUserName(n), () => {});
    return () => { live = false; };
  }, []);

  useEffect(() => {
    if (!palette) return;
    let live = true;
    getHistory({ limit: 1 }).then((rows) => { if (live) setLastRow(rows[0]); }, () => {});
    return () => { live = false; };
  }, [palette]);
  const lastRowText = lastRow?.final_text ?? "";
  const lastAct = useCopyPaste(lastRowText);

  // The Flow Bar's menu names destinations (Microphone, History, Settings), and
  // this is what makes those labels true. An unknown name leaves the Hub where
  // it is, as the Rust side documents.
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    let un: (() => void) | undefined;
    let gone = false;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const off = await listen<string>("hub-navigate", ({ payload }) => { if (isScreenId(payload)) setScreen(payload); });
      if (gone) off(); else un = off;
    })();
    return () => { gone = true; un?.(); };
  }, []);

  // Engine notices are transient, so they are toasts now rather than a banner
  // that pushed the whole screen down until someone dismissed it.
  useEffect(() => {
    if (!view.notice) return;
    const { level, message } = view.notice;
    pushToast({ tone: level === "error" ? "danger" : level, message });
    dismissNotice();
  }, [view.notice, dismissNotice]);

  useHubKeys({
    onScreen: setScreen,
    onPalette: () => setPalette(true),
    onEscape: () => setPalette(false),
  });

  // Three states, not two. "Starting…" while a fatal error is on screen would
  // tell the user the opposite of the truth.
  const engine: EngineState = view.error ? "error" : view.ready ? "ready" : "starting";

  let body;
  if (screen === "home") {
    body = <HomeScreen view={view} settings={settings} patch={patch} now={minute} onOpenHistory={() => setScreen("history")} />;
  } else if (screen === "history") {
    body = <HistoryView view={view} settings={settings} patch={patch} now={minute} onClose={() => setScreen("home")} />;
  } else if (!settings) {
    // Settings arrive asynchronously; a blank pane for a beat reads as broken.
    body = (
      <div className="legacy-scroll legacy-screen">
        <div className="sk" style={{ height: 140 }} />
        <div className="sk" style={{ height: 140 }} />
      </div>
    );
  } else if (screen === "dictionary") {
    // Redesigned screens own their `section.scroll`, as Home does.
    body = <DictionaryScreen settings={settings} patch={patch} />;
  } else if (screen === "style") {
    body = <ProfilesScreen settings={settings} patch={patch} />;
  } else if (screen === "models") {
    body = <ModelsScreen settings={settings} patch={patch} />;
  } else if (screen === "settings") {
    body = (
      <SettingsScreen
        settings={settings}
        patch={patch}
        error={settingsError}
        levelRef={levelRef}
        listening={view.state === "listening"}
      />
    );
  } else {
    body = (
      <div className="legacy-scroll legacy-screen">
        {screen === "advanced" && <AdvancedScreen settings={settings} />}
      </div>
    );
  }

  return (
    <MotionConfig reducedMotion={STILL ? "always" : "user"}>
      <a className="skip-link" href="#main">Skip to content</a>
      <div className="ambient" aria-hidden="true"><i /><i /><i /><i /></div>
      <div className="grain" aria-hidden="true" />
      <div className="app" data-palette={palette ? "open" : undefined}>
        <Sidebar
          screen={screen}
          onNavigate={setScreen}
          engine={engine}
          shortcut={view.ready?.shortcut ?? "Right Ctrl"}
          levelRef={levelRef}
          listening={view.state === "listening"}
        />
        <main className={STILL ? "main" : "main anim"} id="main" tabIndex={-1}>
          <TopBar screen={screen} userName={userName} now={now} onSearch={() => setPalette(true)} />
          {body}
        </main>
      </div>
      <Toasts />
      <CommandPalette
        open={palette}
        onClose={() => setPalette(false)}
        onNavigate={setScreen}
        lastRow={lastRow}
        onCopyLast={() => void lastAct.copy()}
      />
    </MotionConfig>
  );
}
