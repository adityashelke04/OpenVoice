import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { HomeScreen } from "./HomeScreen";
import { installTauri } from "../../test/tauri";
import { dismissToast, getToasts } from "../toast";
import { DICTIONARY, FAILED_ROW, NOW, ROWS, TOTALS } from "../../test/fixtures";
import type { LiveView } from "../../engine/useLiveEngine";
import type { Settings } from "../../engine/settings";
import type { Row } from "../../engine/stats";

/* eslint-disable @typescript-eslint/no-explicit-any */

const VIEW: LiveView = {
  state: "idle", profile: "prose", elapsedMs: 0, lastText: "", lastLatencyMs: null, notice: null,
  lastOutcome: null, session: null, error: null, download: null, sessions: 0,
  ready: { model: "parakeet-tdt-0.6b-v2", device: "CPU", shortcut: "Right Ctrl", mic: "Mic" },
};
const SETTINGS = { config: {} as Settings["config"], model: "", dictionary: DICTIONARY, profiles: [] } as Settings;

const writeText = () => navigator.clipboard.writeText as unknown as ReturnType<typeof vi.fn>;

/** The stub answers get_history the way the Rust side does: newest first, filtered by profile. */
function bridge(rows: Row[] = ROWS, extra: Record<string, (a: any) => unknown> = {}) {
  return installTauri({
    get_history: (a: any) => (a?.profile ? rows.filter((r) => r.profile === a.profile) : rows).slice(0, a?.limit ?? 200),
    get_totals: () => TOTALS,
    paste_again: () => "pasted",
    ...extra,
  });
}

function renderHome(view: Partial<LiveView> = {}) {
  const onOpenHistory = vi.fn();
  const patch = vi.fn();
  const utils = render(<HomeScreen view={{ ...VIEW, ...view }} settings={SETTINGS} patch={patch} now={NOW} onOpenHistory={onOpenHistory} />);
  return { ...utils, onOpenHistory, patch };
}

const lastCard = () => screen.getByRole("article", { name: "Your last dictation" });

/** The toast store's current contents (read without rendering, so it is safe inside waitFor). */
const toasts = () => [...getToasts()];
/** A row's disclosure button (time + text), found from its text. */
const toggleOf = (text: string) => (screen.getByText(text).closest(".row") as HTMLElement).querySelector(".row-toggle") as HTMLButtonElement;

let tauri: ReturnType<typeof installTauri> | null = null;
beforeEach(() => {
  // Date only, and advancing with real time so waitFor can still time out.
  vi.useFakeTimers({ toFake: ["Date"], shouldAdvanceTime: true });
  vi.setSystemTime(NOW);
  writeText().mockClear();
  toasts().forEach((t) => dismissToast(t.id));
});
afterEach(() => {
  tauri?.uninstall();
  tauri = null;
  vi.useRealTimers();
  window.getSelection()?.removeAllRanges();
});

describe("Home", () => {
  it("shows the last dictation with Copy and Fix a word", async () => {
    tauri = bridge();
    renderHome();
    expect(await screen.findByText(ROWS[0].final_text)).toBeTruthy();
    const card = lastCard();
    expect(card.className).toBe("last glass");
    expect(within(card).getByText("Last dictation")).toBeTruthy();
    expect(within(card).getByText("Slack")).toBeTruthy();
    expect(within(card).getByText("2 min ago")).toBeTruthy();
    expect(within(card).getByText("Pasted").closest(".status-ok")).toBeTruthy();
    const buttons = within(card).getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual(["CopyCtrl C", "Fix a word"]);
    expect(buttons[0].className).toBe("btn primary");
    expect(within(card).getByText("49 words, 604 ms")).toBeTruthy();
  });

  it("Copy writes the clipboard and says Copied", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    // From here on timers are fake too (installing again over the Date-only
    // clock is a no-op, so the Date-only one is removed first).
    vi.useRealTimers();
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    const card = lastCard();
    await act(async () => { fireEvent.click(within(card).getByRole("button", { name: /^Copy/ })); });
    expect(writeText()).toHaveBeenCalledWith(ROWS[0].final_text);
    expect(within(card).getByRole("button", { name: "Copied" })).toBeTruthy();
    act(() => { vi.advanceTimersByTime(1399); });
    expect(within(card).getByRole("button", { name: "Copied" })).toBeTruthy();
    act(() => { vi.advanceTimersByTime(1); });
    expect(within(card).getByRole("button", { name: /^Copy/ }).textContent).toBe("CopyCtrl C");
  });

  it("Copy that fails says so in a danger toast and does not claim Copied", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    writeText().mockRejectedValueOnce(new Error("Document is not focused."));
    await act(async () => { fireEvent.click(within(lastCard()).getByRole("button", { name: /^Copy/ })); });
    const [t] = toasts();
    expect(t.tone).toBe("danger");
    expect(t.message).toContain("Document is not focused.");
    expect(within(lastCard()).queryByRole("button", { name: "Copied" })).toBeNull();
  });

  it("failed paste: amber card, Copy is the amber primary, clipboard note", async () => {
    tauri = bridge([FAILED_ROW, ...ROWS.slice(1)]);
    renderHome();
    await screen.findByText(FAILED_ROW.final_text);
    const card = lastCard();
    expect(card.classList.contains("fail")).toBe(true);
    const first = within(card).getAllByRole("button")[0];
    expect(first.textContent).toBe("CopyCtrl C");
    expect(first.className).toBe("btn warnp");
    expect(within(card).getAllByRole("button").map((b) => b.textContent)).toEqual(["CopyCtrl C", "Fix a word"]);
    expect(within(card).getByText("This didn’t paste into Outlook, so it’s on your clipboard.")).toBeTruthy();
    expect(within(card).getByText("29 words")).toBeTruthy();
    expect(within(card).getByText("just now")).toBeTruthy();
    expect(within(card).queryByText("Pasted")).toBeNull();
  });

  it("other failure wording", async () => {
    tauri = bridge([{ ...FAILED_ROW, outcome: "injection_failed" }, ...ROWS.slice(1)]);
    renderHome();
    await screen.findByText(FAILED_ROW.final_text);
    const card = lastCard();
    expect(card.classList.contains("fail")).toBe(true);
    expect(within(card).getByText("This didn’t reach Outlook. Copy it or paste it again.")).toBeTruthy();
    expect(within(card).queryByText(/on your clipboard/)).toBeNull();
  });

  it("Earlier filter asks the backend for that profile", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const earlier = screen.getByRole("region", { name: "Earlier dictations" });
    expect(within(earlier).getByRole("heading", { name: "Earlier" })).toBeTruthy();
    expect(within(earlier).getAllByRole("tab").map((t) => t.textContent)).toEqual(["All", "Code", "Terminal", "Messages"]);
    fireEvent.click(within(earlier).getByRole("tab", { name: "Code" }));
    await waitFor(() => expect(tauri!.calls.some((c) => c.cmd === "get_history" && c.args?.profile === "editor")).toBe(true));
    // Only the editor row is listed; the last dictation (prose) stays on its card.
    await waitFor(() => expect(within(earlier).queryByText("kubectl get pods")).toBeNull());
    expect(within(earlier).getByText("So we need to call useEffect here, then return null")).toBeTruthy();
    expect(within(earlier).getByRole("tab", { name: "Code" }).getAttribute("aria-selected")).toBe("true");
  });

  it("a filtered read that fails keeps the rows and the tab, and says why", async () => {
    tauri = bridge(ROWS, { get_history: (a: any) => (a?.profile ? Promise.reject("database is locked") : ROWS) });
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const earlier = screen.getByRole("region", { name: "Earlier dictations" });
    fireEvent.click(within(earlier).getByRole("tab", { name: "Code" }));
    await waitFor(() => expect(toasts().map((t) => [t.tone, t.message])).toEqual([["danger", "database is locked"]]));
    expect(within(earlier).getByText("kubectl get pods")).toBeTruthy();
    expect(within(earlier).getByText("git status")).toBeTruthy();
    await waitFor(() => expect(within(earlier).getByRole("tab", { name: "All" }).getAttribute("aria-selected")).toBe("true"));
  });

  it("Earlier leaves the last dictation out even when the filter matches it", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const earlier = screen.getByRole("region", { name: "Earlier dictations" });
    fireEvent.click(within(earlier).getByRole("tab", { name: "Messages" }));
    await waitFor(() => expect(within(earlier).queryByText("kubectl get pods")).toBeNull());
    expect(within(earlier).queryByText(ROWS[0].final_text)).toBeNull();
    expect(within(earlier).getByText("Moving the standup to 10:30 tomorrow, same link as always.")).toBeTruthy();
  });

  it("total opens the History view", async () => {
    tauri = bridge();
    const { onOpenHistory } = renderHome();
    await screen.findByText(ROWS[0].final_text);
    fireEvent.click(screen.getByRole("button", { name: /128 total/ }));
    expect(onOpenHistory).toHaveBeenCalled();
  });

  it("Earlier groups by day and shows heard / not pasted chips", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const earlier = screen.getByRole("region", { name: "Earlier dictations" });
    const days = [...earlier.querySelectorAll(".day")].map((d) => d.textContent);
    expect(days).toEqual(["Today", "Yesterday", "Wed 16 Sep"]);
    const effect = within(earlier).getByText("So we need to call useEffect here, then return null").closest(".row") as HTMLElement;
    expect(within(effect).getByText("2:39 PM").tagName).toBe("TIME");
    expect(effect.querySelector(".heard")!.textContent).toBe("heard use effect");
    expect(effect.querySelector(".heard s")!.textContent).toBe("use effect");
    expect(within(effect).getByText("VS Code")).toBeTruthy();
    const git = within(earlier).getByText("git status").closest(".row") as HTMLElement;
    expect(within(git).getByText("Not pasted, on clipboard")).toBeTruthy();
    expect(within(git).getByText("git status").className).toBe("t code");
    expect(within(git).getByText("PowerShell")).toBeTruthy();
    // The last dictation is on the card, not repeated in the list.
    expect(within(earlier).queryByText(ROWS[0].final_text)).toBeNull();
  });

  it("a row's copy button copies that row without expanding it", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const row = screen.getByText("kubectl get pods").closest(".row") as HTMLElement;
    await act(async () => { fireEvent.click(within(row).getByRole("button", { name: "Copy" })); });
    expect(writeText()).toHaveBeenCalledWith("kubectl get pods");
    expect(toggleOf("kubectl get pods").getAttribute("aria-expanded")).toBe("false");
  });

  it("rows are disclosures: time and text are a button that controls the open panel", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const row = screen.getByText("So we need to call useEffect here, then return null").closest(".row") as HTMLElement;
    const toggle = within(row).getByRole("button", { name: /2:39 PM\s*So we need to call useEffect/ });
    expect(toggle.className).toBe("row-toggle");
    expect(toggle.getAttribute("type")).toBe("button"); // a real button: Enter and Space work natively
    expect(toggle.querySelector("time")!.textContent).toBe("2:39 PM");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    const more = document.getElementById(toggle.getAttribute("aria-controls")!)!;
    expect(more.className).toBe("row-more");
    expect(row.contains(more)).toBe(true);
    expect(more.hidden).toBe(true);
    // The copy button is a sibling of the toggle, never inside it.
    expect(toggle.querySelector("button")).toBeNull();
    expect(within(row).getByRole("button", { name: "Copy" }).closest(".row-toggle")).toBeNull();
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(more.hidden).toBe(false);
  });

  it("clicking a row expands it with the same actions; one open at a time", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const a = toggleOf("So we need to call useEffect here, then return null");
    const b = toggleOf("kubectl get pods");
    const rowA = a.closest(".row") as HTMLElement;
    expect(a.getAttribute("aria-expanded")).toBe("false");
    // A click on the row's bare background toggles too (mouse convenience).
    fireEvent.click(rowA);
    expect(a.getAttribute("aria-expanded")).toBe("true");
    const more = rowA.querySelector(".row-more") as HTMLElement;
    expect(within(more).getAllByRole("button").map((x) => x.textContent)).toEqual(["Copy", "Fix a word"]);
    expect(within(more).getAllByRole("button").every((x) => x.classList.contains("sm"))).toBe(true);
    await act(async () => { fireEvent.click(within(more).getByRole("button", { name: "Copy" })); });
    expect(writeText()).toHaveBeenCalledWith("So we need to call useEffect here, then return null");
    expect(a.getAttribute("aria-expanded")).toBe("true"); // an action inside does not collapse it
    // Opening another row closes the first.
    fireEvent.click(b);
    expect(b.getAttribute("aria-expanded")).toBe("true");
    expect(a.getAttribute("aria-expanded")).toBe("false");
    expect((rowA.querySelector(".row-more") as HTMLElement).hidden).toBe(true);
    expect(within(rowA.querySelector(".row-more") as HTMLElement).queryAllByRole("button")).toHaveLength(0);
    fireEvent.click(b);
    expect(b.getAttribute("aria-expanded")).toBe("false");
  });

  it("an open row and its Fix panel stay reachable: the list scrolls them into view", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const earlier = screen.getByRole("region", { name: "Earlier dictations" });
    const list = earlier.querySelector(".rows") as HTMLElement;
    let scrollTop = 0;
    Object.defineProperty(list, "scrollTop", { configurable: true, get: () => scrollTop, set: (v: number) => { scrollTop = v; } });
    // Layout: the list shows 0..300; the Notion row sits at 260..308, and with
    // Fix a word open it reaches down to 460.
    let openBottom = 380;
    const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this === list) return { top: 0, bottom: 300 } as DOMRect;
      if (this.classList.contains("row") && this.classList.contains("open")) return { top: 260 - scrollTop, bottom: openBottom - scrollTop } as DOMRect;
      return { top: 0, bottom: 0 } as DOMRect;
    });
    try {
      const toggle = toggleOf("The resampler runs at 16 kHz mono, so the sidecar never has to guess.");
      fireEvent.click(toggle);
      expect(list.classList.contains("scrolling")).toBe(true);
      expect([...list.children].some((k) => (k as HTMLElement).hidden)).toBe(false); // no fitting while open
      expect(scrollTop).toBe(80); // bottom edge brought into view
      const more = toggle.closest(".row")!.querySelector(".row-more") as HTMLElement;
      openBottom = 460;
      fireEvent.click(within(more).getByRole("button", { name: "Fix a word" }));
      expect(within(more).getByRole("button", { name: "Save" })).toBeTruthy();
      expect(scrollTop).toBe(160);
      // Closing returns to the fitted, unscrolled list.
      fireEvent.click(toggle);
      expect(list.classList.contains("scrolling")).toBe(false);
      expect(scrollTop).toBe(0);
    } finally {
      rect.mockRestore();
    }
  });

  it("Fix a word teaches the dictionary from the heard words", async () => {
    tauri = bridge();
    const { patch } = renderHome();
    await screen.findByText(ROWS[0].final_text);
    fireEvent.click(within(lastCard()).getByRole("button", { name: "Fix a word" }));
    fireEvent.click(within(lastCard()).getByRole("button", { name: "json" }));
    fireEvent.change(within(lastCard()).getByRole("textbox", { name: "Write it as" }), { target: { value: "JSON" } });
    expect((within(lastCard()).getByRole("textbox", { name: "You said" }) as HTMLInputElement).value).toBe("json");
    fireEvent.click(within(lastCard()).getByRole("button", { name: "Save" }));
    expect(patch).toHaveBeenCalledTimes(1);
    const doc = structuredClone(SETTINGS);
    patch.mock.calls[0][0](doc);
    expect(doc.dictionary.find((d) => d.written === "JSON")!.spoken).toContain("json");
    expect(within(lastCard()).queryByRole("textbox", { name: "You said" })).toBeNull();
  });

  it("Ctrl+C copies the card only with no selection and focus outside inputs", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    // Nothing selected, focus on the page: the card is copied.
    await act(async () => { fireEvent.keyDown(document.body, { key: "c", code: "KeyC", ctrlKey: true }); });
    expect(writeText()).toHaveBeenCalledTimes(1);
    expect(writeText()).toHaveBeenLastCalledWith(ROWS[0].final_text);
    // A text selection: the browser's own copy wins.
    const range = document.createRange();
    range.selectNodeContents(screen.getByText("kubectl get pods"));
    window.getSelection()!.addRange(range);
    await act(async () => { fireEvent.keyDown(document.body, { key: "c", code: "KeyC", ctrlKey: true }); });
    expect(writeText()).toHaveBeenCalledTimes(1);
    window.getSelection()!.removeAllRanges();
    // Focus in a field: that field's copy wins.
    fireEvent.click(within(lastCard()).getByRole("button", { name: "Fix a word" }));
    const field = within(lastCard()).getByRole("textbox", { name: "You said" });
    field.focus();
    await act(async () => { fireEvent.keyDown(field, { key: "c", code: "KeyC", ctrlKey: true }); });
    expect(writeText()).toHaveBeenCalledTimes(1);
    // Focus in the Earlier list: that row is what the user is on, not the card.
    field.blur();
    toggleOf("kubectl get pods").focus();
    await act(async () => { fireEvent.keyDown(toggleOf("kubectl get pods"), { key: "c", code: "KeyC", ctrlKey: true }); });
    expect(writeText()).toHaveBeenCalledTimes(1);
    toggleOf("kubectl get pods").blur();
    // Ctrl+Shift+C is not a copy.
    await act(async () => { fireEvent.keyDown(document.body, { key: "C", code: "KeyC", ctrlKey: true, shiftKey: true }); });
    expect(writeText()).toHaveBeenCalledTimes(1);
  });

  it("stats column: 152 wpm, 3.8×, 2h 56m, 6 days, 9,540, About a dissertation chapter", async () => {
    tauri = bridge();
    const { container } = renderHome();
    await screen.findByText(ROWS[0].final_text);
    const stats = [...container.querySelectorAll("aside.stats .stat")].map((s) => s.textContent);
    expect(stats).toEqual([
      "Speaking speed152wpm3.8× faster than typing (40 wpm)",
      "Saved2h56m",
      "Streak6days",
      "Words dictated9,540About a dissertation chapter",
    ]);
    expect(container.querySelectorAll(".week i")).toHaveLength(7);
    expect([...container.querySelectorAll(".week i")].map((i) => i.className)).toEqual(["", "on", "on", "on", "on", "on", "on"]);
    const you = container.querySelector(".speedbar .mk.you") as HTMLElement;
    expect(you.style.left).toBe("76%");
    expect((container.querySelector(".speedbar .mk:not(.you)") as HTMLElement).style.left).toBe("20%");
    expect((container.querySelector(".speedbar .fill") as HTMLElement).style.width).toBe("76%");
  });

  it("few dictations: the speed caption says it settles", async () => {
    tauri = bridge(ROWS, { get_totals: () => ({ ...TOTALS, sessions: 2 }) });
    const { container } = renderHome();
    await screen.findByText(ROWS[0].final_text);
    expect(container.querySelector("aside.stats .stat .s")!.textContent).toBe("Settles after a few more dictations");
  });

  it("refetches when a new dictation lands", async () => {
    tauri = bridge();
    const { rerender } = renderHome();
    await screen.findByText(ROWS[0].final_text);
    const before = tauri.calls.filter((c) => c.cmd === "get_history").length;
    rerender(<HomeScreen view={{ ...VIEW, sessions: 1 }} settings={SETTINGS} patch={() => {}} now={NOW} onOpenHistory={() => {}} />);
    await waitFor(() => expect(tauri!.calls.filter((c) => c.cmd === "get_history").length).toBeGreaterThan(before));
  });

  it("first run: the get-started card, the empty panel and the Try saying tile", async () => {
    tauri = bridge([], { get_totals: () => ({ sessions: 0, words: 0, speakingMs: 0, topApp: null, activeDays: [] }) });
    renderHome({ ready: { ...VIEW.ready!, shortcut: "Right Alt" } });
    const card = await screen.findByRole("article", { name: "Get started" });
    expect(within(card).getByText("Ready when you are")).toBeTruthy();
    expect(within(card).getByRole("heading", { name: "Click into any text box, hold the key and talk." })).toBeTruthy();
    expect(within(card).getByText("Let go and your words appear where your cursor is. They also land here, so you can copy or paste them again.")).toBeTruthy();
    // The keycap names the real shortcut, not the reference's default.
    expect(card.querySelector(".bigkey .keycap")?.textContent).toBe("Right Alt");
    expect(within(card).getByText("hold while you speak")).toBeTruthy();
    expect(screen.getByText("Your dictations will collect here")).toBeTruthy();
    expect(screen.getByText("Nothing leaves this PC. History is a plain file you can open, copy or delete.")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Earlier dictations" })).toBeNull();
    expect(screen.getByText("Try saying")).toBeTruthy();
    expect(screen.getByText("\"call use effect here comma then return null\"")).toBeTruthy();
    expect(screen.getByText("In VS Code that becomes useEffect, with the comma.")).toBeTruthy();
    const speed = screen.getByText("Speaking speed").closest(".stat") as HTMLElement;
    expect(speed.querySelector(".v")?.textContent).toBe("-");
    expect(within(speed).getByText("Appears after a few dictations")).toBeTruthy();
    // No streak, saved or words tiles before there is anything to count.
    expect(screen.queryByText("Streak")).toBeNull();
  });

  it("engine error (memory): red card, plain copy, raw error, and Earlier still renders", async () => {
    tauri = bridge(ROWS, { retry_engine: () => true });
    renderHome({ error: "sherpa-onnx: failed to allocate 786432000 bytes" });
    const card = await screen.findByRole("article", { name: "Speech engine problem" });
    expect(card.className).toBe("last glass err");
    expect(within(card).getByText("Speech engine")).toBeTruthy();
    expect(within(card).getByRole("heading", { name: "Not enough memory to load the speech model" })).toBeTruthy();
    const body = within(card).getByText(/The speech model needs about 750 MB of memory/);
    expect(body.textContent).toBe("The speech model needs about 750 MB of memory. Close something large, a game, a browser with many tabs, another AI tool, and try again.");
    expect(card.textContent).not.toMatch(/\u2014/);
    expect(within(card).getByText("sherpa-onnx: failed to allocate 786432000 bytes").className).toBe("err-raw");
    expect(await screen.findByRole("region", { name: "Earlier dictations" })).toBeTruthy();
    expect(screen.getByText("Speaking speed")).toBeTruthy();
  });

  it("Try again calls retry_engine and holds for 4 s as Trying again…", async () => {
    tauri = bridge(ROWS, { retry_engine: () => true });
    renderHome({ error: "sherpa-onnx: failed to allocate 786432000 bytes" });
    await screen.findByRole("article", { name: "Speech engine problem" });
    vi.useRealTimers();
    vi.useFakeTimers();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Try again" })); });
    expect(tauri.calls.filter((c) => c.cmd === "retry_engine")).toHaveLength(1);
    expect((screen.getByRole("button", { name: "Trying again…" }) as HTMLButtonElement).disabled).toBe(true);
    await act(async () => { vi.advanceTimersByTime(3_900); });
    expect((screen.getByRole("button", { name: "Trying again…" }) as HTMLButtonElement).disabled).toBe(true);
    await act(async () => { vi.advanceTimersByTime(100); });
    expect((screen.getByRole("button", { name: "Try again" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("a retry that answers after the card is gone schedules nothing", async () => {
    let answer: (v: boolean) => void = () => {};
    tauri = bridge(ROWS, { retry_engine: () => new Promise<boolean>((r) => { answer = r; }) });
    const { unmount } = renderHome({ error: "sherpa-onnx: failed to allocate 786432000 bytes" });
    await screen.findByRole("article", { name: "Speech engine problem" });
    vi.useRealTimers();
    vi.useFakeTimers();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Try again" })); });
    await act(async () => { await vi.dynamicImportSettled(); });
    unmount();
    await act(async () => { answer(true); for (let i = 0; i < 5; i++) await Promise.resolve(); });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("Open log folder calls open_data_dir", async () => {
    tauri = bridge(ROWS, { open_data_dir: () => null });
    renderHome({ error: "sherpa-onnx: failed to allocate 786432000 bytes" });
    const open = await screen.findByRole("button", { name: "Open log folder" });
    await act(async () => { fireEvent.click(open); });
    expect(tauri.calls.some((c) => c.cmd === "open_data_dir")).toBe(true);
  });

  it.each([
    ["no speech model found in the resources folder"],
    // The real error ov-asr raises when the model folder is missing (crates/ov-asr/src/locate.rs).
    ["the parakeet-tdt-0.6b-v2 model is not installed. Expected it in C:\\Users\\x\\AppData\\Roaming\\ai.openvoice\\models\\parakeet-tdt-0.6b-v2."],
  ])("engine error (missing model): %s", async (error) => {
    tauri = bridge();
    renderHome({ error });
    expect(await screen.findByRole("heading", { name: "The speech model could not be found" })).toBeTruthy();
    expect(screen.getByText("The speech model is missing from the installation. Reinstalling OpenVoice will restore it; it ships inside the installer, so this needs no download.")).toBeTruthy();
    expect(screen.getByText(error)).toBeTruthy();
  });

  it("engine error (anything else): the generic copy", async () => {
    tauri = bridge();
    renderHome({ error: "the audio device went away" });
    expect(await screen.findByRole("heading", { name: "The speech engine could not start" })).toBeTruthy();
    expect(screen.getByText("The full details are in the log file.")).toBeTruthy();
  });

  it("loading: the reference's skeleton shapes until both history and totals answer", async () => {
    tauri = installTauri({ get_history: () => new Promise(() => {}), get_totals: () => TOTALS });
    const { container } = renderHome();
    await act(async () => {});
    // The 210px hero with four bars, the list panel, three stat blocks.
    expect(container.querySelectorAll(".sk")).toHaveLength(5);
    expect(container.querySelector(".sk.sk-hero")?.querySelectorAll(".skl")).toHaveLength(4);
    // Announced once as busy, not read out as a pile of empty boxes.
    expect(container.querySelector("[aria-busy='true']")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Earlier" })).toBeNull();
    expect(screen.queryByRole("article")).toBeNull();
  });

  it("loading: totals that never answer keep the skeleton too", async () => {
    tauri = installTauri({ get_history: () => ROWS, get_totals: () => new Promise(() => {}) });
    const { container } = renderHome();
    await act(async () => {});
    expect(container.querySelectorAll(".sk")).toHaveLength(5);
    expect(screen.queryByText(ROWS[0].final_text)).toBeNull();
  });
});

