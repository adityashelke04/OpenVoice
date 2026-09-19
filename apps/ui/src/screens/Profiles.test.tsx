import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ProfilesScreen } from "./Profiles";
import { useSettings } from "../hub/useSettings";
import { installTauri } from "../test/tauri";
import type { Profile, Settings } from "../engine/settings";

/** The built-in profiles in the order `ov-format` stores them, which is not the
 *  order the screen shows them in. */
const PROFILES: Profile[] = [
  { name: "default", matches: [], capitalize: "sentence", end_period: false, fillers: "light", voice_commands: true, case_transforms: true, dictionaries: ["code"] },
  { name: "terminal", matches: ["WindowsTerminal.exe", "pwsh.exe"], capitalize: "force_lower", end_period: false, fillers: "light", voice_commands: true, case_transforms: true, dictionaries: ["shell", "code"] },
  { name: "editor", matches: ["Code.exe", "Cursor.exe"], capitalize: "sentence", end_period: false, fillers: "light", voice_commands: true, case_transforms: true, dictionaries: ["code"] },
  { name: "prose", matches: ["slack.exe", "Discord.exe", "Notion.exe", "chrome.exe", "msedge.exe", "firefox.exe", "olk.exe"], capitalize: "sentence", end_period: true, fillers: "aggressive", voice_commands: true, case_transforms: false, dictionaries: ["code"] },
];

const SAMPLE = "um so basically the deploy is done and you know we should ship it";
const TRACE: [string, string][] = [
  ["raw", SAMPLE],
  ["fillers", "the deploy is done and we should ship it"],
  ["capitalize", "The deploy is done and we should ship it"],
  ["end_period", "The deploy is done and we should ship it."],
];

let tauri: ReturnType<typeof installTauri> | null = null;
// Every mount and every tab switch starts a preview. Unmount, then let those
// in-flight calls land on the fake bridge before removing it; otherwise a test
// that ends right after a click leaves one to reject against a missing bridge.
afterEach(async () => {
  cleanup();
  await new Promise((r) => setTimeout(r, 20));
  tauri?.uninstall();
  tauri = null;
});

function Harness() {
  const { settings, patch } = useSettings();
  return settings ? <ProfilesScreen settings={settings} patch={patch} /> : null;
}

function mount() {
  const settings = { config: {}, model: "", dictionary: [], profiles: structuredClone(PROFILES) } as unknown as Settings;
  tauri = installTauri({
    get_settings: () => settings,
    save_settings: ({ settings: s }: { settings: Settings }) => s,
    preview_format: ({ text }: { text: string }) => (text === SAMPLE ? TRACE : [["raw", text], ["end", `${text}!`]]),
  });
  render(<Harness />);
  return tauri;
}

/** The named profile as the last `save_settings` call sent it. */
const saved = (name: string) => {
  const saves = tauri!.calls.filter((c) => c.cmd === "save_settings");
  const s = saves[saves.length - 1]?.args.settings as Settings | undefined;
  return s?.profiles.find((p) => p.name === name);
};

const rulesCard = () => screen.getByText("Rules").closest("article") as HTMLElement;
const row = (label: string) => within(rulesCard()).getByText(label).closest(".srow") as HTMLElement;

describe("Writing style screen", () => {
  it("leads with what the screen is for", async () => {
    mount();
    const lead = await screen.findByText(/^The same words should look different depending on where they land\./);
    expect(lead.textContent).toBe("The same words should look different depending on where they land. A chat message wants a capital letter and a full stop; a terminal command must have neither.");
    expect(lead.className).toBe("lead");
  });

  it("shows the four styles in a fixed order, whatever order they are stored in", async () => {
    mount();
    const tabs = within(await screen.findByRole("tablist", { name: "Writing style" })).getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual(["Messages and documents", "Code editors", "Terminals", "Everything else"]);
    expect(screen.getByRole("tablist", { name: "Writing style" }).className).toBe("seg lg");
    // Messages is open first, as in the reference.
    expect(tabs[0].getAttribute("aria-selected")).toBe("true");
  });

  it("captions the Rules card with where each style applies", async () => {
    mount();
    await screen.findByText("Rules");
    const caption = () => rulesCard().querySelector(".card-head .right")?.textContent;
    expect(caption()).toBe("Chat, email, notes, browsers");
    const captions: Record<string, string> = {
      "Code editors": "VS Code, Cursor and other editors",
      Terminals: "Command prompts",
      "Everything else": "Anything not listed elsewhere",
    };
    for (const [tab, text] of Object.entries(captions)) {
      fireEvent.click(screen.getByRole("tab", { name: tab }));
      expect(caption()).toBe(text);
    }
  });

  it("binds each switch to the open style", async () => {
    mount();
    await screen.findByText("Rules");
    const sw = (name: string) => screen.getByRole("switch", { name });

    expect(sw("Capitalise sentences").getAttribute("aria-checked")).toBe("true");
    fireEvent.click(sw("Capitalise sentences"));
    await waitFor(() => expect(saved("prose")?.capitalize).toBe("force_lower"));

    fireEvent.click(sw("End with a full stop"));
    await waitFor(() => expect(saved("prose")?.end_period).toBe(false));

    fireEvent.click(sw("Spoken punctuation"));
    await waitFor(() => expect(saved("prose")?.voice_commands).toBe(false));

    expect(sw("Spoken naming styles").getAttribute("aria-checked")).toBe("false");
    fireEvent.click(sw("Spoken naming styles"));
    await waitFor(() => expect(saved("prose")?.case_transforms).toBe(true));

    // Capitalise back on writes "sentence", and only the open style changed.
    fireEvent.click(sw("Capitalise sentences"));
    await waitFor(() => expect(saved("prose")?.capitalize).toBe("sentence"));
    expect(saved("terminal")?.capitalize).toBe("force_lower");
  });

  it("edits the style whose tab is open", async () => {
    mount();
    await screen.findByText("Rules");
    fireEvent.click(screen.getByRole("tab", { name: "Terminals" }));
    const cap = screen.getByRole("switch", { name: "Capitalise sentences" });
    expect(cap.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(cap);
    await waitFor(() => expect(saved("terminal")?.capitalize).toBe("sentence"));
    expect(saved("prose")?.capitalize).toBe("sentence");
  });

  it("sets filler removal with an Off / Light / Aggressive control", async () => {
    mount();
    await screen.findByText("Rules");
    const seg = within(row("Remove filler words")).getByRole("tablist", { name: "Remove filler words" });
    const tabs = within(seg).getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual(["Off", "Light", "Aggressive"]);
    expect(tabs[2].getAttribute("aria-selected")).toBe("true");
    fireEvent.click(tabs[0]);
    await waitFor(() => expect(saved("prose")?.fillers).toBe("off"));
    fireEvent.click(within(seg).getByRole("tab", { name: "Light" }));
    await waitFor(() => expect(saved("prose")?.fillers).toBe("light"));
  });

  it("keeps the short hint on screen and the full original as the row's tooltip", async () => {
    mount();
    await screen.findByText("Rules");
    const expected: [string, string, string][] = [
      ["Capitalise sentences", "Start each sentence with a capital letter.", "Start each sentence with a capital letter."],
      ["End with a full stop", "Add one if you did not say it.", "Add one if you did not say it."],
      ["Remove filler words", "Light removes um and uh. Aggressive also removes like, you know and basically.", "“Light” removes um and uh. “Aggressive” also removes like, you know and basically — useful, but those words are occasionally what you meant."],
      ["Spoken punctuation", "Say \"comma\" or \"new line\" and get the symbol.", "Say “comma”, “new line” or “open bracket” and get the symbol. Say “literally comma” to get the word."],
      ["Spoken naming styles", "Say \"camel case user name\" and get userName.", "Say “camel case user name” and get userName. Mostly useful when writing code."],
    ];
    const rows = rulesCard().querySelectorAll(".srow");
    expect(rows).toHaveLength(5);
    expected.forEach(([label, hint, title], i) => {
      expect(rows[i].querySelector(".lab")?.textContent).toBe(label);
      expect(rows[i].querySelector(".hint")?.textContent).toBe(hint);
      expect(rows[i].getAttribute("title")).toBe(title);
    });
  });

  it("previews the open style's last formatting stage for its sample", async () => {
    const t = mount();
    const card = (await screen.findByText("What that does")).closest("article") as HTMLElement;
    expect(within(card).getByText("You say")).toBeTruthy();
    expect(within(card).getByText("OpenVoice writes")).toBeTruthy();
    expect(card.querySelector(".trybox:not(.out) .txt")?.textContent).toBe(SAMPLE);
    await waitFor(() => expect(card.querySelector(".trybox.out .txt")?.textContent).toBe("The deploy is done and we should ship it."));
    expect(t.calls.find((c) => c.cmd === "preview_format")?.args).toEqual({ text: SAMPLE, profile: "prose" });

    fireEvent.click(screen.getByRole("tab", { name: "Terminals" }));
    const sample = "cube control get pods dash dash all dash namespaces";
    expect(card.querySelector(".trybox:not(.out) .txt")?.textContent).toBe(sample);
    await waitFor(() => expect(card.querySelector(".trybox.out .txt")?.textContent).toBe(`${sample}!`));
  });

  it("lists the apps a style is used in as exe chips", async () => {
    mount();
    const card = (await screen.findByText("Used in these apps")).closest("article") as HTMLElement;
    expect([...card.querySelectorAll(".chips .exe")].map((c) => c.textContent)).toEqual(
      ["slack.exe", "Discord.exe", "Notion.exe", "chrome.exe", "msedge.exe", "firefox.exe", "olk.exe"],
    );
  });

  it("says the fallback style covers everything else", async () => {
    mount();
    await screen.findByText("Used in these apps");
    fireEvent.click(screen.getByRole("tab", { name: "Everything else" }));
    const card = screen.getByText("Used in these apps").closest("article") as HTMLElement;
    expect(card.querySelectorAll(".exe")).toHaveLength(0);
    expect(within(card).getByText("Anything not matched by another style uses this one.")).toBeTruthy();
  });

  it("does not use the legacy screen styles", async () => {
    mount();
    await screen.findByText("Rules");
    expect(document.querySelector(".screen, .style-tabs, .t-title")).toBeNull();
    expect(document.querySelector("section.scroll .style-grid .style-side")).toBeTruthy();
  });
});
