import { afterEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { AdvancedScreen } from "./Advanced";
import { installTauri } from "../test/tauri";
import { DICTIONARY_12 } from "../test/fixtures";
import type { Settings } from "../engine/settings";

const PHRASE = "um so we need to call use effect here comma then return null";

/** What `preview_format` really answers for the reference phrase under the
 *  `editor` profile.
 *
 *  Every stage `ov-format` runs, in the order `default_rules` runs them, with
 *  `parse` first — see `crates/ov-format/src/lib.rs`. The screen's first
 *  version was written against a five-stage invention (`raw, fillers,
 *  dictionary, commands, capitalize`) that the mock in
 *  `scripts/screenshot-fixtures.mjs` also told, so the pixel diff and these
 *  tests agreed with each other and neither agreed with the app. Four of these
 *  eight stages leave the sentence alone, which is the whole reason the rail
 *  shows what changed by default. */
const TRACE: [string, string][] = [
  ["parse", PHRASE],
  ["repeats", PHRASE],
  ["fillers", "so we need to call use effect here comma then return null"],
  ["commands", "so we need to call use effect here, then return null"],
  ["dictionary", "so we need to call useEffect here, then return null"],
  ["case", "so we need to call useEffect here, then return null"],
  ["capitalize", "So we need to call useEffect here, then return null"],
  ["profile", "So we need to call useEffect here, then return null"],
];

/** The four stages above that actually rewrote the sentence. */
const CHANGED = ["fillers", "commands", "dictionary", "capitalize"];

const LOG = "C:\\Users\\you\\AppData\\Roaming\\OpenVoice\\openvoice.log";

let tauri: ReturnType<typeof installTauri> | null = null;
afterEach(() => { tauri?.uninstall(); tauri = null; });

function mount(trace: [string, string][] = TRACE) {
  const settings = {
    config: { paste_threshold_chars: 120 },
    model: "parakeet-tdt-0.6b-v2",
    dictionary: structuredClone(DICTIONARY_12),
    profiles: [],
  } as unknown as Settings;
  tauri = installTauri({
    preview_format: ({ text }: { text: string }) =>
      text === PHRASE ? trace : [["parse", text], ["capitalize", text.toUpperCase()]],
    get_log_path: () => LOG,
  });
  render(<AdvancedScreen settings={settings} />);
  return tauri;
}

const steps = () => [...document.querySelectorAll<HTMLElement>(".pipe .step")];
const names = () => steps().map((s) => s.querySelector(".sn")?.textContent);
const cardNamed = async (title: string) =>
  (await screen.findByText(title)).closest("article") as HTMLElement;
const showAll = () => screen.getByRole("button", { name: /stages|what changed/ });

describe("Advanced screen", () => {
  it("leads with what the screen is for", async () => {
    mount();
    const lead = await screen.findByText(
      "Nothing here is needed for everyday use. It exists so that when something looks wrong, you can see exactly why.",
    );
    expect(lead.className).toBe("lead");
  });

  it("traces the reference sentence through the code editors style", async () => {
    const t = mount();
    const input = await screen.findByLabelText("A sentence to trace");
    expect((input as HTMLInputElement).value).toBe(PHRASE);
    expect(input.closest(".field")?.className).toContain("mono");
    await waitFor(() => expect(steps()).toHaveLength(CHANGED.length));
    expect(t.calls.find((c) => c.cmd === "preview_format")?.args).toEqual({ text: PHRASE, profile: "editor" });
    const card = await cardNamed("How a sentence is rewritten");
    expect(within(card).getByText("Code editors style").className).toContain("cap");
  });

  it("shows only the stages that rewrote the sentence", async () => {
    mount();
    await waitFor(() => expect(names()).toEqual(CHANGED));
    expect(steps().map((s) => s.querySelector(".so")?.textContent)).toEqual(
      CHANGED.map((n) => TRACE.find(([stage]) => stage === n)?.[1]),
    );
    // Every row shown is a row that did something, so every row is tagged.
    expect(steps().map((s) => s.querySelector(".tg")?.textContent)).toEqual(CHANGED.map(() => "changed"));
    expect(steps().every((s) => s.classList.contains("ch"))).toBe(true);
  });

  it("offers the silent stages rather than hiding them", async () => {
    mount();
    await waitFor(() => expect(steps()).toHaveLength(4));
    const toggle = showAll();
    expect(toggle.textContent).toContain("Show all 8 stages");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
  });

  it("names every stage the formatter ran once the rail is opened", async () => {
    mount();
    await waitFor(() => expect(steps()).toHaveLength(4));
    fireEvent.click(showAll());
    expect(names()).toEqual(TRACE.map(([stage]) => stage));
    expect(steps().map((s) => s.querySelector(".so")?.textContent)).toEqual(TRACE.map(([, out]) => out));
    // `parse` is the starting point and cannot have changed anything; the three
    // rules that ran without effect are shown as having done nothing.
    expect(steps().map((s) => s.classList.contains("ch")))
      .toEqual([false, false, true, true, true, false, true, false]);
    expect(showAll().getAttribute("aria-expanded")).toBe("true");
  });

  it("closes the rail again", async () => {
    mount();
    await waitFor(() => expect(steps()).toHaveLength(4));
    fireEvent.click(showAll());
    expect(showAll().textContent).toContain("Show only what changed");
    fireEvent.click(showAll());
    expect(names()).toEqual(CHANGED);
  });

  it("marks exactly what each stage added", async () => {
    mount();
    await waitFor(() => expect(steps()).toHaveLength(4));
    // `fillers` only removes words, so it marks nothing; the order is the
    // engine's, which runs the spoken comma before the dictionary.
    expect([...document.querySelectorAll(".pipe mark")].map((m) => m.textContent))
      .toEqual([",", "useEffect", "S"]);
  });

  it("says so when no rule touched the sentence", async () => {
    mount([["parse", PHRASE], ["repeats", PHRASE], ["fillers", PHRASE]]);
    expect(await screen.findByText("Nothing was rewritten — this is exactly what you said.")).toBeTruthy();
    expect(steps()).toHaveLength(0);
    fireEvent.click(showAll());
    expect(names()).toEqual(["parse", "repeats", "fillers"]);
  });

  it("re-traces as the sentence is edited", async () => {
    const t = mount();
    await waitFor(() => expect(steps()).toHaveLength(4));
    fireEvent.change(await screen.findByLabelText("A sentence to trace"), { target: { value: "hello there" } });
    await waitFor(() => expect(steps()[0]?.querySelector(".so")?.textContent).toBe("HELLO THERE"));
    expect(t.calls.filter((c) => c.cmd === "preview_format").map((c) => c.args.text))
      .toEqual([PHRASE, "hello there"]);
  });

  it("shows where the log is, shortened to a path you can paste", async () => {
    mount();
    const card = await cardNamed("Files");
    const hint = await within(card).findByText("%APPDATA%\\OpenVoice\\openvoice.log");
    expect(getComputedStyle(hint).fontFamily).toBe("var(--mono)");
    expect(within(hint.closest(".srow") as HTMLElement).getByRole("button", { name: "Open folder" })).toBeTruthy();
  });

  it("opens the data folder from either row", async () => {
    const t = mount();
    const card = await cardNamed("Files");
    const buttons = within(card).getAllByRole("button", { name: "Open folder" });
    expect(buttons).toHaveLength(2);
    buttons.forEach((b) => fireEvent.click(b));
    await waitFor(() => expect(t.calls.filter((c) => c.cmd === "open_data_dir")).toHaveLength(2));
  });

  it("states the engine facts behind the everyday screens", async () => {
    mount();
    const card = await cardNamed("Engine");
    const row = (label: string) => within(card).getByText(label).closest(".srow") as HTMLElement;
    expect(within(row("Speech model")).getByText("parakeet-tdt-0.6b-v2").className).toContain("kv");
    expect(within(row("Paste instead of type above")).getByText("120 characters").className).toContain("kv");
    expect(within(row("Corrections")).getByText("12").className).toContain("kv");
  });
});
