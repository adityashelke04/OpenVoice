/** The Flow Menu's rows.
 *
 * Modelled on Wispr Flow's, which offers Hide for 1 hour, Settings, Microphone,
 * transcript history and Paste last transcript — and is the part of their bar
 * that makes it a control surface rather than a status light. The two items this
 * menu used to have could open the Hub and hide the bar, which meant every other
 * thing a person might want mid-dictation required finding the Hub first.
 *
 * Destinations route to a named Hub section (see `show_hub_cmd`), so the labels
 * name where they actually go.
 *
 * # Why this is its own file, and why the rows are a pure function
 *
 * It lived inside `Overlay.tsx`, which is 1800 lines, and the rows were
 * therefore unreachable from a test: the only way to ask what a row does was to
 * mount the whole overlay. The rows are the menu's entire contract — which
 * command each one sends, and where the dividers fall — so they are worth
 * holding down directly.
 *
 * `flowMenuRows` takes its side effects as arguments rather than importing
 * them. That is not ceremony: it means a test can watch what a row actually
 * sends without mocking the Tauri IPC boundary, and a row that closes the menu
 * while sending nothing stops being indistinguishable from one that works.
 */

import { useCallback } from "react";

/** One row of the Flow Menu. `sep` renders a divider before the item. */
export type MenuRow = { id: string; label: string; run: () => void; sep?: boolean };

/** What the bar is doing. Decides labels and which rows are offered at all. */
export type FlowMenuState = {
  mini: boolean;
  live: boolean;
  working: boolean;
  autoCollapse: boolean;
};

/** Everything a row can do to the world outside the menu. */
export type FlowMenuActions = {
  call: (cmd: string, args?: Record<string, unknown>) => void;
  close: () => void;
  setMini: (b: boolean) => void;
  setAutoCollapse: (b: boolean) => void;
};

/**
 * The menu's rows, in the order they are painted.
 *
 * Three groups, separated by the `sep` flag on the row that opens each: what the
 * bar can do with your voice, where to go, and where the bar itself lives.
 */
export function flowMenuRows(s: FlowMenuState, a: FlowMenuActions): MenuRow[] {
  const { mini, live, working, autoCollapse } = s;
  const { call, close, setMini, setAutoCollapse } = a;

  return [
    {
      id: "dictate",
      label: live ? "Stop dictating" : "Start dictating",
      run: () => {
        call("toggle_session");
        close();
      },
    },
    {
      id: "paste",
      label: "Paste last transcript",
      run: () => {
        call("paste_last");
        close();
      },
    },
    {
      id: "history",
      label: "Transcript history",
      sep: true,
      run: () => {
        call("show_hub_cmd", { tab: "home" });
        close();
      },
    },
    {
      id: "mic",
      label: "Microphone",
      run: () => {
        call("show_hub_cmd", { tab: "settings" });
        close();
      },
    },
    {
      id: "settings",
      label: "Settings",
      run: () => {
        call("show_hub_cmd", { tab: "settings" });
        close();
      },
    },
    {
      // Opens the placement group, because it is the one row here that answers
      // "the bar is in the wrong place" — the reason a person right-clicks it at
      // all once they have dragged it somewhere they regret. `overlay_reset_position`
      // clears the remembered anchor and re-parks, which also undocks a bar
      // stuck against a side edge.
      //
      // Named for the destination rather than the mechanism. The tray calls the
      // same command "Reset Flow Bar position", which is accurate and reads like
      // a settings page; over somebody else's window, read once, in a hurry,
      // "Back to center" is the thing the user actually wants.
      id: "recenter",
      label: "Back to center",
      sep: true,
      run: () => {
        call("overlay_reset_position");
        close();
      },
    },
    {
      id: "mini",
      label: mini ? "Full bar" : "Compact bar",
      run: () => {
        setMini(!mini);
        call("overlay_set_mini", { on: !mini });
        close();
      },
    },
    {
      // Named for what the bar does, not for the mechanism. "Auto-collapse" is
      // a description of an implementation; "get out of the way" is the thing
      // the user actually wants, and the label has to survive being read once,
      // in a hurry, over somebody else's window.
      id: "auto-collapse",
      label: autoCollapse ? "Stay full size" : "Shrink when idle",
      run: () => {
        setAutoCollapse(!autoCollapse);
        call("overlay_set_auto_collapse", { on: !autoCollapse });
        close();
      },
    },
    {
      // Named for what it does rather than for how long, because an hour is a
      // detail and "you will not see this again today" is the decision.
      id: "snooze",
      label: "Hide for an hour",
      run: () => {
        call("overlay_snooze", { minutes: 60 });
        close();
      },
    },
    {
      id: "dictate-only",
      label: "Only show while dictating",
      run: () => {
        call("overlay_always_visible", { on: false });
        close();
      },
    },
  ].filter((r) => !(working && r.id === "dictate"));
}

/** `flowMenuRows` bound to the overlay's own `setMenu` and IPC helper. */
export function useFlowMenu(
  v: FlowMenuState & {
    call: (cmd: string, args?: Record<string, unknown>) => void;
    setMenu: (b: boolean) => void;
    setMini: (b: boolean) => void;
    setAutoCollapse: (b: boolean) => void;
  },
): MenuRow[] {
  const { mini, live, working, autoCollapse, call, setMenu, setMini, setAutoCollapse } = v;
  const close = useCallback(() => setMenu(false), [setMenu]);
  return flowMenuRows(
    { mini, live, working, autoCollapse },
    { call, close, setMini, setAutoCollapse },
  );
}
