import { afterEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { AdvancedScreen } from "./Advanced";
import { installTauri } from "../test/tauri";
import { DICTIONARY_12 } from "../test/fixtures";
import type { Settings } from "../engine/settings";

const PHRASE = "um so we need to call use effect here comma then return null";

/** The trace the twin's fixture stub returns for the reference phrase under the
 *  `editor` profile: the real stages, and no `end_period` (editors do not get
 *  one), which is why the screen asks for that profile by name. */
const TRACE: [string, string][] = [
  ["raw", PHRASE],
  ["fillers", "so we need to call use effect here comma then return null"],
  ["dictionary", "so we need to call useEffect here comma then return null"],
  ["commands", "so we need to call useEffect here, then return null"],
  ["capitalize", "So we need to call useEffect here, then return null"],
];

const LOG = "C:\\Users\\you\\AppData\\Roaming\\OpenVoice\\openvoice.log";

let tauri: ReturnType<typeof installTauri> | null = null;
afterEach(() => { tauri?.uninstall(); tauri = null; });

function mount(extra: Record<string, (args: never) => unknown> = {}) {
  const settings = {
    config: { paste_threshold_chars: 120 },
    model: "parakeet-tdt-0.6b-v2",
    dictionary: structuredClone(DICTIONARY_12),
    profiles: [],
  } as unknown as Settings;
  tauri = installTauri({
    preview_format: ({ text }: { text: string }) =>
      text === PHRASE ? TRACE : [["raw", text], ["capitalize", text.toUpperCase()]],
    get_log_path: () => LOG,
    ...extra,
  });
  render(<AdvancedScreen settings={settings} />);
  return tauri;
}

const steps = () => [...document.querySelectorAll<HTMLElement>(".pipe .step")];
const cardNamed = async (title: string) =>
  (await screen.findByText(title)).closest("article") as HTMLElement;

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
    await waitFor(() => expect(steps()).toHaveLength(5));
    expect(t.calls.find((c) => c.cmd === "preview_format")?.args).toEqual({ text: PHRASE, profile: "editor" });
    const card = await cardNamed("How a sentence is rewritten");
    expect(within(card).getByText("Code editors style").className).toContain("cap");
  });

  it("names every stage the formatter ran", async () => {
    mount();
    await waitFor(() => expect(steps()).toHaveLength(5));
    expect(steps().map((s) => s.querySelector(".sn")?.textContent)).toEqual([
      "raw", "fillers", "dictionary", "commands", "capitalize",
    ]);
    expect(steps().map((s) => s.querySelector(".so")?.textContent)).toEqual(TRACE.map(([, out]) => out));
  });

  it("tags only the stages that changed the text", async () => {
    mount();
    await waitFor(() => expect(steps()).toHaveLength(5));
    // `fillers` drops a word, so it changed; `raw` is the starting point and
    // cannot have.
    expect(steps().map((s) => s.classList.contains("ch"))).toEqual([false, true, true, true, true]);
    expect(steps().map((s) => s.querySelector(".tg")?.textContent ?? null))
      .toEqual([null, "changed", "changed", "changed", "changed"]);
  });

  it("marks exactly what each stage added", async () => {
    mount();
    await waitFor(() => expect(steps()).toHaveLength(5));
    expect([...document.querySelectorAll(".pipe mark")].map((m) => m.textContent))
      .toEqual(["useEffect", ",", "S"]);
  });

  it("re-traces as the sentence is edited", async () => {
    const t = mount();
    await waitFor(() => expect(steps()).toHaveLength(5));
    fireEvent.change(await screen.findByLabelText("A sentence to trace"), { target: { value: "hello there" } });
    await waitFor(() => expect(steps()[1]?.querySelector(".so")?.textContent).toBe("HELLO THERE"));
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
