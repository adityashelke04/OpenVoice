import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { HistoryView } from "./HistoryView";
import { installTauri } from "../../test/tauri";
import { DICTIONARY, NOW, ROWS, TOTALS } from "../../test/fixtures";
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

/** Answers get_history the way the Rust side does: newest first, by profile, by substring, capped at limit. */
function bridge(rows: Row[] = ROWS) {
  return installTauri({
    get_history: (a: any) => {
      let out = a?.profile ? rows.filter((r) => r.profile === a.profile) : rows;
      if (a?.query) out = out.filter((r) => r.final_text.toLowerCase().includes(String(a.query).toLowerCase()));
      return out.slice(0, a?.limit ?? 200);
    },
    get_totals: () => TOTALS,
  });
}

/** Lets the stubbed invoke promises settle and React commit, without waitFor (timers are fake). */
const flush = () => act(async () => {
  // The first call imports @tauri-apps/api/core; later ones are microtasks.
  await vi.dynamicImportSettled();
  for (let i = 0; i < 10; i++) await Promise.resolve();
});

function renderHistory() {
  const onClose = vi.fn();
  const utils = render(<HistoryView view={VIEW} settings={SETTINGS} patch={vi.fn()} now={NOW} onClose={onClose} />);
  return { ...utils, onClose };
}

let tauri: ReturnType<typeof installTauri> | null = null;
const historyCalls = () => tauri!.calls.filter((c) => c.cmd === "get_history").map((c) => c.args);

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
});
afterEach(() => {
  tauri?.uninstall();
  tauri = null;
  vi.useRealTimers();
});

describe("History view", () => {
  it("one glass panel: filter, search field naming the total, day-grouped rows including the newest", async () => {
    tauri = bridge();
    const { container } = renderHistory();
    await flush();
    const panel = container.querySelector("section.scroll > section.hist.glass") as HTMLElement;
    expect(panel).toBeTruthy();
    expect(within(panel).getByRole("tablist", { name: "Filter by kind of app" })).toBeTruthy();
    expect(within(panel).getByPlaceholderText("Search 128 dictations")).toBeTruthy();
    expect([...panel.querySelectorAll(".day")].map((d) => d.textContent)).toEqual(["Today", "Yesterday", "Wed 16 Sep"]);
    const times = [...panel.querySelectorAll(".row time")].map((t) => t.textContent);
    expect(times).toHaveLength(9);
    expect(times[0]).toBe("2:41 PM");
    // The same disclosure rows as Home.
    expect(panel.querySelectorAll(".row .row-toggle[aria-expanded]")).toHaveLength(9);
  });

  it("search is debounced: typing kube asks once, 180 ms after the last key", async () => {
    tauri = bridge();
    renderHistory();
    await flush();
    const before = historyCalls().length;
    const input = screen.getByPlaceholderText("Search 128 dictations");
    for (const v of ["k", "ku", "kub", "kube"]) {
      fireEvent.change(input, { target: { value: v } });
      await act(async () => { vi.advanceTimersByTime(50); });
    }
    await act(async () => { vi.advanceTimersByTime(129); });
    expect(historyCalls().length).toBe(before);
    await act(async () => { vi.advanceTimersByTime(1); });
    await flush();
    const after = historyCalls().slice(before);
    expect(after).toHaveLength(1);
    expect(after[0]).toMatchObject({ query: "kube", limit: 200 });
    expect(screen.getByText("kubectl get pods")).toBeTruthy();
    expect(screen.queryByText("git status")).toBeNull();
  });

  it("search answers in relevance order are listed newest first, one heading per day", async () => {
    // FTS returns by rank; here the oldest comes back first.
    const ranked = [ROWS[8], ROWS[1], ROWS[6], ROWS[3]];
    tauri = installTauri({ get_history: (a: any) => (a?.query ? ranked : ROWS), get_totals: () => TOTALS });
    const { container } = renderHistory();
    await flush();
    fireEvent.change(screen.getByPlaceholderText("Search 128 dictations"), { target: { value: "the" } });
    await act(async () => { vi.advanceTimersByTime(180); });
    await flush();
    expect([...container.querySelectorAll(".day")].map((d) => d.textContent)).toEqual(["Today", "Wed 16 Sep"]);
    expect([...container.querySelectorAll(".row time")].map((t) => t.textContent)).toEqual(["2:39 PM", "11:08 AM", "5:52 PM", "9:44 AM"]);
  });

  it("a filter tab asks the backend for that profile", async () => {
    tauri = bridge();
    renderHistory();
    await flush();
    await act(async () => { fireEvent.click(screen.getByRole("tab", { name: "Terminal" })); });
    await flush();
    expect(historyCalls().at(-1)).toMatchObject({ profile: "terminal" });
    expect(screen.getByText("kubectl get pods")).toBeTruthy();
    expect(screen.queryByText("Moving the standup to 10:30 tomorrow, same link as always.")).toBeNull();
  });

  it("a search with no results says so, with the query", async () => {
    tauri = bridge();
    renderHistory();
    await flush();
    fireEvent.change(screen.getByPlaceholderText("Search 128 dictations"), { target: { value: "zzz" } });
    await act(async () => { vi.advanceTimersByTime(180); });
    await flush();
    expect(screen.getByText("Nothing matches “zzz”")).toBeTruthy();
  });

  it("Escape returns to Home", async () => {
    tauri = bridge();
    const { onClose } = renderHistory();
    await flush();
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Escape in a search with text clears it first, then a second Escape returns", async () => {
    tauri = bridge();
    const { onClose } = renderHistory();
    await flush();
    const input = screen.getByPlaceholderText("Search 128 dictations") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "kube" } });
    input.focus();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(input.value).toBe("");
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("scrolling to the bottom of a full page asks for the next 200", async () => {
    const many: Row[] = Array.from({ length: 500 }, (_, i) => ({ ...ROWS[1], created_at: NOW - (i + 1) * 60_000, final_text: `dictation ${i}` }));
    tauri = bridge(many);
    const { container } = renderHistory();
    await flush();
    expect(container.querySelectorAll(".row")).toHaveLength(200);
    const list = container.querySelector(".hist .rows") as HTMLElement;
    Object.defineProperty(list, "scrollHeight", { configurable: true, value: 8000 });
    Object.defineProperty(list, "clientHeight", { configurable: true, value: 500 });
    list.scrollTop = 7500;
    fireEvent.scroll(list);
    await flush();
    expect(historyCalls().at(-1)).toMatchObject({ limit: 400 });
    expect(container.querySelectorAll(".row")).toHaveLength(400);
  });

  it("Escape in a row's Fix a word field closes the fix panel only: no leaving, no clearing the search", async () => {
    tauri = bridge();
    const { onClose } = renderHistory();
    await flush();
    const search = screen.getByPlaceholderText("Search 128 dictations") as HTMLInputElement;
    fireEvent.change(search, { target: { value: "kube" } });
    await act(async () => { vi.advanceTimersByTime(180); });
    await flush();
    const row = screen.getByText("kubectl get pods").closest(".row") as HTMLElement;
    fireEvent.click(row.querySelector(".row-toggle")!);
    const fixButton = within(row).getByRole("button", { name: "Fix a word" });
    fireEvent.click(fixButton);
    const said = within(row).getByRole("textbox", { name: "You said" }) as HTMLInputElement;
    said.focus();
    fireEvent.change(said, { target: { value: "cube cont" } });
    fireEvent.keyDown(said, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    expect(search.value).toBe("kube");
    expect(historyCalls().at(-1)).toMatchObject({ query: "kube" });
    expect(within(row).queryByRole("textbox", { name: "You said" })).toBeNull();
    expect(document.activeElement).toBe(within(row).getByRole("button", { name: "Fix a word" }));
  });

  it("Escape in any other editable field is left to that field", async () => {
    tauri = bridge();
    const { onClose } = renderHistory();
    await flush();
    const other = document.createElement("textarea");
    document.body.appendChild(other);
    other.focus();
    fireEvent.keyDown(other, { key: "Escape" });
    other.remove();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("a slow answer to an older search never replaces a newer one", async () => {
    let releaseKu: (rows: Row[]) => void = () => {};
    tauri = installTauri({
      get_history: (a: any) => a?.query === "ku"
        ? new Promise<Row[]>((r) => { releaseKu = r; })
        : a?.query ? ROWS.filter((r) => r.final_text.includes(a.query)) : ROWS,
      get_totals: () => TOTALS,
    });
    renderHistory();
    await flush();
    const input = screen.getByPlaceholderText("Search 128 dictations");
    fireEvent.change(input, { target: { value: "ku" } });
    await act(async () => { vi.advanceTimersByTime(180); });
    fireEvent.change(input, { target: { value: "kube" } });
    await act(async () => { vi.advanceTimersByTime(180); });
    await flush();
    expect(screen.getByText("kubectl get pods")).toBeTruthy();
    await act(async () => { releaseKu([ROWS[5]]); });
    await flush();
    expect(screen.getByText("kubectl get pods")).toBeTruthy();
    expect(screen.queryByText(ROWS[5].final_text)).toBeNull();
  });

  it("scroll events before the next page lands ask for it once", async () => {
    const many: Row[] = Array.from({ length: 500 }, (_, i) => ({ ...ROWS[1], created_at: NOW - (i + 1) * 60_000, final_text: `dictation ${i}` }));
    tauri = bridge(many);
    const { container } = renderHistory();
    await flush();
    const list = container.querySelector(".hist .rows") as HTMLElement;
    Object.defineProperty(list, "scrollHeight", { configurable: true, value: 8000 });
    Object.defineProperty(list, "clientHeight", { configurable: true, value: 500 });
    list.scrollTop = 7500;
    fireEvent.scroll(list);
    fireEvent.scroll(list);
    fireEvent.scroll(list);
    await flush();
    expect(historyCalls().map((a) => a.limit)).toEqual([200, 400]);
  });

  it("a new filter or search starts the list at the top", async () => {
    tauri = bridge();
    const { container } = renderHistory();
    await flush();
    const list = container.querySelector(".hist .rows") as HTMLElement;
    list.scrollTop = 300;
    await act(async () => { fireEvent.click(screen.getByRole("tab", { name: "Code" })); });
    await flush();
    expect(list.scrollTop).toBe(0);
    list.scrollTop = 300;
    fireEvent.change(screen.getByPlaceholderText("Search 128 dictations"), { target: { value: "use" } });
    await act(async () => { vi.advanceTimersByTime(180); });
    await flush();
    expect(list.scrollTop).toBe(0);
  });

  it("a short last page asks for nothing more", async () => {
    tauri = bridge();
    const { container } = renderHistory();
    await flush();
    const before = historyCalls().length;
    const list = container.querySelector(".hist .rows") as HTMLElement;
    Object.defineProperty(list, "scrollHeight", { configurable: true, value: 500 });
    Object.defineProperty(list, "clientHeight", { configurable: true, value: 500 });
    fireEvent.scroll(list);
    await flush();
    expect(historyCalls().length).toBe(before);
  });
});
