import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { CommandPalette } from "./CommandPalette";
import { installTauri } from "../test/tauri";
import { setMedia } from "../test/setup";
import { __resetThemeStore } from "./theme";
import type { Row } from "../engine/stats";

const LAST_ROW: Row = {
  created_at: 1, outcome: "delivered", raw_text: "hi", final_text: "hello there",
  profile: "prose", target_app: "slack.exe", audio_ms: 900, latency_ms: 120,
};
const HIST_ROW: Row = {
  created_at: 2, outcome: "delivered", raw_text: "cube control get pods", final_text: "kubectl get pods",
  profile: "terminal", target_app: "WindowsTerminal.exe", audio_ms: 1600, latency_ms: 288,
};

/** A thin harness: an "opener" button owns focus and open state, the way the
 *  search button and Ctrl+K really open the palette. */
function Harness({ onNavigate = () => {}, onCopyLast = () => {}, onPasteLast = () => {}, lastRow }: {
  onNavigate?: (id: string) => void;
  onCopyLast?: () => void;
  onPasteLast?: () => void;
  lastRow?: Row;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div>
      <button type="button" onClick={() => setOpen(true)}>Search</button>
      <CommandPalette
        open={open}
        onClose={() => setOpen(false)}
        onNavigate={onNavigate as never}
        lastRow={lastRow}
        onCopyLast={onCopyLast}
        onPasteLast={onPasteLast}
      />
    </div>
  );
}

function openPalette() {
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
}

const placeholder = "Type a command or search what you've said";

let tauri: ReturnType<typeof installTauri> | null = null;
function withTauri(handlers: Parameters<typeof installTauri>[0]) {
  tauri = installTauri(handlers);
  return tauri;
}

describe("CommandPalette", () => {
  beforeEach(() => {
    // Reduced motion so the enter/exit animation is instant and closing
    // (and the focus it returns) can be asserted without racing a transition.
    setMedia("(prefers-reduced-motion: reduce)", true);
    __resetThemeStore();
  });
  afterEach(() => {
    tauri?.uninstall();
    tauri = null;
    setMedia("(prefers-reduced-motion: reduce)", false);
  });

  it("opens with the search placeholder", () => {
    render(<Harness />);
    openPalette();
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.getByPlaceholderText(placeholder)).toBeTruthy();
  });

  it("typing dict then Enter navigates to dictionary and closes", async () => {
    const onNavigate = vi.fn();
    render(<Harness onNavigate={onNavigate} />);
    openPalette();
    const input = screen.getByPlaceholderText(placeholder);
    fireEvent.change(input, { target: { value: "dict" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onNavigate).toHaveBeenCalledWith("dictionary");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("Theme: Lagoon sets data-theme on the document", () => {
    render(<Harness />);
    openPalette();
    fireEvent.click(screen.getByText("Theme: Lagoon"));
    expect(document.documentElement.dataset.theme).toBe("lagoon");
  });

  it("Light sets mode light", () => {
    render(<Harness />);
    openPalette();
    fireEvent.click(screen.getByText("Light"));
    expect(document.documentElement.dataset.mode).toBe("light");
    expect(document.documentElement.dataset.modeChoice).toBe("light");
  });

  it("typing ku queries get_history({query, limit: 8}) and lists the match under History", async () => {
    const { calls } = withTauri({
      get_history: (args: { query?: string; limit?: number }) =>
        [HIST_ROW].filter((r) => !args.query || r.final_text.toLowerCase().includes(String(args.query).toLowerCase())).slice(0, args.limit ?? 200),
    });
    render(<Harness />);
    openPalette();
    const input = screen.getByPlaceholderText(placeholder);
    fireEvent.change(input, { target: { value: "ku" } });
    await waitFor(() => expect(screen.getByText("kubectl get pods")).toBeTruthy());
    const call = calls.find((c) => c.cmd === "get_history" && c.args?.query === "ku");
    expect(call?.args).toMatchObject({ query: "ku", limit: 8 });
    const group = screen.getByText("History").closest("[cmdk-group]") as HTMLElement;
    expect(within(group).getByText("kubectl get pods")).toBeTruthy();
  });

  it("Enter on a History row copies it", async () => {
    withTauri({ get_history: () => [HIST_ROW] });
    render(<Harness />);
    openPalette();
    const input = screen.getByPlaceholderText(placeholder);
    fireEvent.change(input, { target: { value: "ku" } });
    await waitFor(() => expect(screen.getByText("kubectl get pods")).toBeTruthy());
    fireEvent.click(screen.getByText("kubectl get pods"));
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledWith("kubectl get pods"));
  });

  it("Ctrl+Enter on a History row pastes it again", async () => {
    const { calls } = withTauri({ get_history: () => [HIST_ROW], paste_again: () => "pasted" });
    render(<Harness />);
    openPalette();
    const input = screen.getByPlaceholderText(placeholder);
    fireEvent.change(input, { target: { value: "ku" } });
    await waitFor(() => expect(screen.getByText("kubectl get pods")).toBeTruthy());
    // The History row is the last item in the list; End moves cmdk's own
    // highlight to it without triggering a select the way a click would.
    fireEvent.keyDown(input, { key: "End" });
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(calls.some((c) => c.cmd === "paste_again" && c.args?.text === "kubectl get pods")).toBe(true));
  });

  it("Copy last dictation calls onCopyLast and closes", async () => {
    const onCopyLast = vi.fn();
    render(<Harness lastRow={LAST_ROW} onCopyLast={onCopyLast} />);
    openPalette();
    fireEvent.click(screen.getByText("Copy last dictation"));
    expect(onCopyLast).toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("hides the last-dictation actions when there is no last row", () => {
    render(<Harness />);
    openPalette();
    expect(screen.queryByText("Copy last dictation")).toBeNull();
    expect(screen.queryByText("Paste last dictation again")).toBeNull();
  });

  it("Escape closes the palette, marks the event handled, and returns focus to the opener", async () => {
    render(<Harness />);
    const opener = screen.getByRole("button", { name: "Search" });
    opener.focus();
    openPalette();
    const input = screen.getByPlaceholderText(placeholder);
    // A window-level listener like HistoryView's own Escape handler must see
    // this event as already handled, or one Escape would close two things.
    let seenDefaultPrevented: boolean | null = null;
    const onWindowEscape = (e: KeyboardEvent) => { if (e.key === "Escape") seenDefaultPrevented = e.defaultPrevented; };
    window.addEventListener("keydown", onWindowEscape);
    fireEvent.keyDown(input, { key: "Escape" });
    window.removeEventListener("keydown", onWindowEscape);
    expect(seenDefaultPrevented).toBe(true);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });
});
