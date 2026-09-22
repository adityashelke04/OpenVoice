import { afterEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { DictionaryScreen } from "./Dictionary";
import { useSettings } from "../hub/useSettings";
import { installTauri } from "../test/tauri";
import { DICTIONARY_12 } from "../test/fixtures";
import type { DictEntry, Settings } from "../engine/settings";

/** The formatter trace the twin's fixture stub returns for the reference phrase. */
const TRACE: [string, string][] = [
  ["fillers", "so we need to call use effect here comma then return null"],
  ["dictionary", "so we need to call useEffect here comma then return null"],
  ["commands", "so we need to call useEffect here, then return null"],
  ["capitalize", "So we need to call useEffect here, then return null"],
  ["end_period", "So we need to call useEffect here, then return null."],
];
const PHRASE = "um so we need to call use effect here comma then return null";

let tauri: ReturnType<typeof installTauri> | null = null;
afterEach(() => { tauri?.uninstall(); tauri = null; });

/** The real settings hook over a fake bridge, so a patch is what reaches `save_settings`. */
function Harness() {
  const { settings, patch } = useSettings();
  return settings ? <DictionaryScreen settings={settings} patch={patch} /> : null;
}

function mount(dictionary: DictEntry[] = DICTIONARY_12) {
  const settings = { config: {}, model: "", dictionary: structuredClone(dictionary), profiles: [] } as unknown as Settings;
  tauri = installTauri({
    get_settings: () => settings,
    save_settings: ({ settings: s }: { settings: Settings }) => s,
    preview_format: () => TRACE,
  });
  render(<Harness />);
  return tauri;
}

const saved = () => {
  const saves = tauri!.calls.filter((c) => c.cmd === "save_settings");
  return (saves[saves.length - 1]?.args.settings as Settings | undefined)?.dictionary;
};

describe("Dictionary screen", () => {
  it("leads with what the screen is for", async () => {
    mount();
    expect((await screen.findByText("Names, jargon and technical words often come out wrong. Tell OpenVoice what you meant once, and it will get it right from then on.")).className).toContain("lead");
  });

  it("has a Try a phrase card captioned with the style it uses", async () => {
    mount();
    const card = (await screen.findByText("Try a phrase")).closest("article")!;
    expect(card.className).toBe("card glass");
    expect(within(card).getByText("Uses the Messages style").className).toContain("cap");
    expect(within(card).getByText("What lands at your cursor")).toBeTruthy();
  });

  it("previews what OpenVoice heard and marks only what changed", async () => {
    const t = mount();
    const heard = await screen.findByLabelText("What OpenVoice heard");
    expect(heard.tagName).toBe("TEXTAREA");
    expect(heard.getAttribute("placeholder")).toBe("Type what OpenVoice wrote, for example: call use effect here");
    fireEvent.change(heard, { target: { value: PHRASE } });
    await waitFor(() => expect(document.querySelector(".trybox.out .txt")?.textContent).toBe("So we need to call useEffect here, then return null."));
    expect(t.calls.find((c) => c.cmd === "preview_format")?.args).toEqual({ text: PHRASE, profile: "prose" });
    const marks = [...document.querySelectorAll(".trybox.out mark")].map((m) => m.textContent);
    expect(marks).toEqual(["useEffect", ","]);
  });

  it("lists every correction as spoken forms, an arrow and the written form", async () => {
    mount();
    await screen.findByText("Your corrections");
    const terms = document.querySelectorAll(".term");
    expect(terms).toHaveLength(12);
    expect(terms[0].querySelector(".sp")?.textContent).toBe("use effect · you seffect");
    expect(terms[0].querySelector(".wr")?.textContent).toBe("useEffect");
    expect(terms[1].querySelector(".wr")?.textContent).toBe("kubectl");
    // The count next to the card title.
    expect(within(screen.getByText("Your corrections").closest(".card-head") as HTMLElement).getByText("12").className).toContain("cap");
  });

  it("removes a correction with its remove button", async () => {
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Remove kubectl" }));
    await waitFor(() => expect(saved()?.map((t) => t.written)).not.toContain("kubectl"));
    expect(saved()).toHaveLength(11);
  });

  it.each(["You said", "Write it as"])("adds a correction with Enter in the %s field", async (field) => {
    mount();
    fireEvent.change(await screen.findByLabelText("You said"), { target: { value: "tanstack" } });
    fireEvent.change(screen.getByLabelText("Write it as"), { target: { value: "TanStack" } });
    fireEvent.keyDown(screen.getByLabelText(field), { key: "Enter" });
    await waitFor(() => expect(saved()?.[0]).toEqual({ written: "TanStack", spoken: ["tanstack"], group: "code" }));
    expect((screen.getByLabelText("You said") as HTMLInputElement).value).toBe("");
  });

  it("adds a correction with the Add button", async () => {
    mount();
    fireEvent.change(await screen.findByLabelText("You said"), { target: { value: "vee test" } });
    fireEvent.change(screen.getByLabelText("Write it as"), { target: { value: "Vitest" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(saved()?.[0].written).toBe("Vitest"));
  });

  it("filters by spoken or written form", async () => {
    mount();
    const search = await screen.findByLabelText("Search corrections");
    fireEvent.change(search, { target: { value: "cuttle" } });
    expect([...document.querySelectorAll(".term .wr")].map((e) => e.textContent)).toEqual(["kubectl"]);
    fireEvent.change(search, { target: { value: "json" } });
    expect([...document.querySelectorAll(".term .wr")].map((e) => e.textContent)).toEqual(["JSON"]);
  });

  it("says so when the dictionary is empty", async () => {
    mount([]);
    expect(await screen.findByText("No corrections yet")).toBeTruthy();
    expect(screen.getByText("When OpenVoice mishears a word, add it here. A name, a piece of jargon, a product: anything it writes wrong more than once.")).toBeTruthy();
  });

  it("ends with the built-in terms caption", async () => {
    mount();
    expect((await screen.findByText("OpenVoice also knows 30 built-in terms like useEffect and kubectl. Yours always win. Changes apply to your next dictation.")).className).toContain("cap");
  });
});
