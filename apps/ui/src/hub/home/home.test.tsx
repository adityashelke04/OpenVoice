import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { HomeScreen } from "./HomeScreen";
import { installTauri } from "../../test/tauri";
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

let tauri: ReturnType<typeof installTauri> | null = null;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  writeText().mockClear();
});
afterEach(() => {
  tauri?.uninstall();
  tauri = null;
  vi.useRealTimers();
  window.getSelection()?.removeAllRanges();
});

describe("Home", () => {
  it("shows the last dictation with Copy, Paste again and Fix a word", async () => {
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
    expect(buttons.map((b) => b.textContent)).toEqual(["CopyCtrl C", "Paste again", "Fix a word"]);
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

  it("Paste again calls paste_again with the row text and says Pasted", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const card = lastCard();
    await act(async () => { fireEvent.click(within(card).getByRole("button", { name: "Paste again" })); });
    expect(tauri.calls).toContainEqual({ cmd: "paste_again", args: { text: ROWS[0].final_text } });
    expect(await within(card).findByRole("button", { name: "Pasted" })).toBeTruthy();
  });

  it("Paste again that fell back to the clipboard leaves the label and adds no toast of its own", async () => {
    tauri = bridge(ROWS, { paste_again: () => "copied" });
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const card = lastCard();
    await act(async () => { fireEvent.click(within(card).getByRole("button", { name: "Paste again" })); });
    expect(within(card).getByRole("button", { name: "Paste again" })).toBeTruthy();
    expect(within(card).queryByRole("button", { name: "Pasted" })).toBeNull();
  });

  it("failed paste: amber card, Paste again is primary, clipboard note", async () => {
    tauri = bridge([FAILED_ROW, ...ROWS.slice(1)]);
    renderHome();
    await screen.findByText(FAILED_ROW.final_text);
    const card = lastCard();
    expect(card.classList.contains("fail")).toBe(true);
    const first = within(card).getAllByRole("button")[0];
    expect(first.textContent).toBe("Paste again");
    expect(first.className).toBe("btn warnp");
    expect(within(card).getAllByRole("button").map((b) => b.textContent)).toEqual(["Paste again", "Copy", "Fix a word"]);
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
    expect(row.getAttribute("aria-expanded")).toBe("false");
  });

  it("clicking a row expands it with the same actions; one open at a time", async () => {
    tauri = bridge();
    renderHome();
    await screen.findByText(ROWS[0].final_text);
    const a = screen.getByText("So we need to call useEffect here, then return null").closest(".row") as HTMLElement;
    const b = screen.getByText("kubectl get pods").closest(".row") as HTMLElement;
    expect(a.getAttribute("aria-expanded")).toBe("false");
    expect(a.getAttribute("tabindex")).toBe("0");
    fireEvent.click(a);
    expect(a.getAttribute("aria-expanded")).toBe("true");
    const more = a.querySelector(".row-more") as HTMLElement;
    expect(more).toBeTruthy();
    expect(within(more).getAllByRole("button").map((x) => x.textContent)).toEqual(["Copy", "Paste again", "Fix a word"]);
    expect(within(more).getAllByRole("button").every((x) => x.classList.contains("sm"))).toBe(true);
    await act(async () => { fireEvent.click(within(more).getByRole("button", { name: "Paste again" })); });
    expect(tauri.calls).toContainEqual({ cmd: "paste_again", args: { text: "So we need to call useEffect here, then return null" } });
    expect(a.getAttribute("aria-expanded")).toBe("true"); // an action inside does not collapse it
    // Keyboard: Enter on another row opens it and closes the first.
    fireEvent.keyDown(b, { key: "Enter" });
    expect(b.getAttribute("aria-expanded")).toBe("true");
    expect(a.getAttribute("aria-expanded")).toBe("false");
    expect(a.querySelector(".row-more")).toBeNull();
    fireEvent.keyDown(b, { key: " " });
    expect(b.getAttribute("aria-expanded")).toBe("false");
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
    // Ctrl+Shift+C is not a copy.
    field.blur();
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

  it("first run: no rows shows the get-started card", async () => {
    tauri = bridge([], { get_totals: () => ({ sessions: 0, words: 0, speakingMs: 0, topApp: null, activeDays: [] }) });
    renderHome();
    expect(await screen.findByText("Ready when you are")).toBeTruthy();
    expect(screen.getByText("Click into any text box, hold the key and talk.")).toBeTruthy();
    expect(screen.getByText("Your dictations will collect here")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Earlier dictations" })).toBeNull();
  });

  it("engine error: says what failed and offers Try again; Earlier still renders", async () => {
    tauri = bridge(ROWS, { retry_engine: () => true });
    renderHome({ error: "sherpa-onnx: failed to allocate 786432000 bytes" });
    expect(await screen.findByText("Not enough memory to load the speech model")).toBeTruthy();
    expect(screen.getByText("sherpa-onnx: failed to allocate 786432000 bytes")).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Try again" })); });
    expect(tauri.calls.some((c) => c.cmd === "retry_engine")).toBe(true);
    expect(await screen.findByRole("region", { name: "Earlier dictations" })).toBeTruthy();
  });

  it("loading: skeletons until both history and totals answer", async () => {
    tauri = installTauri({ get_history: () => new Promise(() => {}), get_totals: () => TOTALS });
    const { container } = renderHome();
    await act(async () => {});
    expect(container.querySelectorAll(".sk").length).toBeGreaterThan(0);
    expect(screen.queryByRole("heading", { name: "Earlier" })).toBeNull();
  });
});
