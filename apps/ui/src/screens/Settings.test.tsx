import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { SettingsScreen } from "./Settings";
import { useSettings } from "../hub/useSettings";
import { __resetThemeStore } from "../hub/theme";
import { DEFAULT_REDACT_PATTERNS, type Config, type Settings } from "../engine/settings";
import { installTauri } from "../test/tauri";

/** The defaults `ov_core::config` ships, as the fixture stub serves them. */
const CONFIG = {
  version: 1,
  chord: { key: "right_ctrl", exclusive: true },
  activation: "push_to_talk",
  limits: { min_duration_ms: 250, max_duration_ms: 120_000, preroll_ms: 300, silence_rms: 0.01 },
  privacy: { retain_audio: false, audio_days: 7, history_days: 0, redact_patterns: [...DEFAULT_REDACT_PATTERNS] },
  updates: { check_on_launch: true },
  language: null,
  input_device: null,
  paste_threshold_chars: 120,
  sound_enabled: true,
} satisfies Config;

const MICS = ["Microphone Array (Realtek Audio)", "Headset (WH-1000XM4 Hands-Free)", "Yeti Nano"];

let tauri: ReturnType<typeof installTauri> | null = null;
const root = document.documentElement;

beforeEach(() => {
  localStorage.clear();
  __resetThemeStore();
  for (const k of ["theme", "mode", "modeChoice", "solid"]) delete root.dataset[k];
});
afterEach(() => { tauri?.uninstall(); tauri = null; });

/** The real settings hook over a fake bridge, so a patch is what reaches
 *  `save_settings` rather than what a spy says was intended. */
function Harness({ level = 0, listening = false }: { level?: number; listening?: boolean } = {}) {
  const { settings, patch, error } = useSettings();
  return settings ? (
    <SettingsScreen
      settings={settings}
      patch={patch}
      error={error}
      levelRef={{ current: level }}
      listening={listening}
    />
  ) : null;
}

function mount(
  config: Partial<Config> = {},
  extra: Record<string, (args: never) => unknown> = {},
  props: { level?: number; listening?: boolean } = {},
) {
  const settings = {
    config: structuredClone({ ...CONFIG, ...config }),
    model: "parakeet-tdt-0.6b-v2",
    dictionary: [],
    profiles: [],
  } as unknown as Settings;
  tauri = installTauri({
    get_settings: () => settings,
    save_settings: ({ settings: s }: { settings: Settings }) => s,
    list_microphones: () => MICS,
    restart_reasons: () => [],
    ...extra,
  });
  render(<Harness {...props} />);
  return tauri;
}

/** The config as the last save actually sent it. */
const saved = (): Config | undefined => {
  const saves = tauri!.calls.filter((c) => c.cmd === "save_settings");
  return (saves[saves.length - 1]?.args.settings as Settings | undefined)?.config;
};

/** A row by its visible label, so an assertion names what a reader sees. */
const row = async (label: string) =>
  (await screen.findByText(label, { selector: ".lab" })).closest(".srow") as HTMLElement;

describe("Settings: Dictation", () => {
  it("sets the shortcut as a keycap and rebinds it through the field", async () => {
    mount();
    const shortcut = await row("Shortcut");
    expect(shortcut.querySelector(".keycap")?.textContent).toBe("Right Ctrl");

    fireEvent.change(within(shortcut).getByLabelText("Shortcut"), { target: { value: "right_alt" } });
    await waitFor(() => expect(saved()?.chord.key).toBe("right_alt"));
  });

  it("switches how dictation starts between hold and toggle", async () => {
    mount();
    const how = await row("How it starts");
    expect(within(how).getByRole("tab", { name: "Hold" }).getAttribute("aria-selected")).toBe("true");

    fireEvent.click(within(how).getByRole("tab", { name: "Press to toggle" }));
    await waitFor(() => expect(saved()?.activation).toBe("toggle"));

    fireEvent.click(within(how).getByRole("tab", { name: "Hold" }));
    await waitFor(() => expect(saved()?.activation).toBe("push_to_talk"));
  });

  it("offers the system default first, then every microphone the machine has", async () => {
    mount();
    const mic = await row("Microphone");
    const select = within(mic).getByLabelText("Microphone") as HTMLSelectElement;
    await waitFor(() => expect(select.options).toHaveLength(MICS.length + 1));
    expect([...select.options].map((o) => o.textContent)).toEqual(["System default", ...MICS]);
    expect(select.value).toBe("System default");

    fireEvent.change(select, { target: { value: "Yeti Nano" } });
    await waitFor(() => expect(saved()?.input_device).toBe("Yeti Nano"));
  });

  it("returns to the system default by choosing it, not by storing its name", async () => {
    mount({ input_device: "Yeti Nano" });
    const mic = await row("Microphone");
    fireEvent.change(within(mic).getByLabelText("Microphone"), { target: { value: "System default" } });
    await waitFor(() => expect(saved()?.input_device).toBeNull());
  });

  it("shows the live level as lit bars in the meter", async () => {
    mount({}, {}, { level: 0.44 });
    const mic = await row("Microphone");
    const bars = () => [...mic.querySelectorAll(".meter i")];
    expect(bars()).toHaveLength(6);
    // sqrt(0.44) * 6 rounds to 4: the meter is perceptual, not linear.
    await waitFor(() => expect(bars().filter((b) => !b.classList.contains("off"))).toHaveLength(4));
  });

  it("turns the start and finish tone off", async () => {
    mount();
    const sound = await row("Sound feedback");
    const toggle = within(sound).getByRole("switch");
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(toggle);
    await waitFor(() => expect(saved()?.sound_enabled).toBe(false));
  });

  it("steps the recording limit between one, two and five minutes", async () => {
    mount();
    const max = await row("Maximum recording");
    const slider = within(max).getByRole("slider");
    expect(slider.getAttribute("aria-valuetext")).toBe("2 min");
    expect(slider.getAttribute("aria-valuemin")).toBe("60000");
    expect(slider.getAttribute("aria-valuemax")).toBe("300000");

    fireEvent.keyDown(slider, { key: "ArrowRight" });
    await waitFor(() => expect(saved()?.limits.max_duration_ms).toBe(300_000));

    fireEvent.keyDown(slider, { key: "ArrowLeft" });
    await waitFor(() => expect(saved()?.limits.max_duration_ms).toBe(120_000));
  });
});

describe("Settings: Appearance", () => {
  it("applies a theme to the whole window when its card is chosen", async () => {
    mount();
    const graphite = await screen.findByRole("button", { name: /Graphite/ });
    expect(graphite.className).toContain("tcard");
    expect(graphite.getAttribute("aria-pressed")).toBe("false");

    fireEvent.click(graphite);
    await waitFor(() => expect(root.dataset.theme).toBe("graphite"));
    expect(graphite.getAttribute("aria-pressed")).toBe("true");
    expect(localStorage.getItem("ov.theme")).toBe("graphite");
  });

  it("carries each card's own theme so its preview is that theme, not this one", async () => {
    mount();
    const glacier = await screen.findByRole("button", { name: /Glacier/ });
    expect(glacier.dataset.theme).toBe("glacier");
    // The resolved mode, so a preview is never a dark card on a light page.
    expect(glacier.dataset.mode).toBe("light");
    fireEvent.click(within(await row("Light or dark")).getByRole("tab", { name: "Dark" }));
    await waitFor(() => expect(glacier.dataset.mode).toBe("dark"));
    // The name has to stay readable against the page, not the card's theme.
    expect((glacier.closest(".themes") as HTMLElement).style.getPropertyValue("--page-ink")).toBe("var(--ink)");
  });

  it("chooses light, system or dark", async () => {
    mount();
    const mode = await row("Light or dark");
    fireEvent.click(within(mode).getByRole("tab", { name: "Dark" }));
    await waitFor(() => expect(root.dataset.mode).toBe("dark"));
    fireEvent.click(within(mode).getByRole("tab", { name: "Light" }));
    await waitFor(() => expect(root.dataset.mode).toBe("light"));
    expect(localStorage.getItem("ov.mode")).toBe("light");
  });

  it("drops the glass for solid panels", async () => {
    mount();
    const solid = await row("Reduce transparency");
    fireEvent.click(within(solid).getByRole("switch"));
    await waitFor(() => expect(root.dataset.solid).toBe(""));
    fireEvent.click(within(solid).getByRole("switch"));
    await waitFor(() => expect(root.dataset.solid).toBeUndefined());
  });
});

describe("Settings: Privacy", () => {
  it("asks how long to keep recordings only once they are kept at all", async () => {
    mount();
    expect(screen.queryByText("Delete recordings after", { selector: ".lab" })).toBeNull();

    const keep = await row("Keep recordings");
    fireEvent.click(within(keep).getByRole("switch"));
    await waitFor(() => expect(saved()?.privacy.retain_audio).toBe(true));
    expect(await screen.findByText("Delete recordings after", { selector: ".lab" })).toBeTruthy();
  });

  it("empties the redaction patterns when secrets are no longer hidden, and restores them", async () => {
    mount();
    const hide = await row("Hide secrets in history");
    const toggle = within(hide).getByRole("switch");
    expect(toggle.getAttribute("aria-checked")).toBe("true");

    fireEvent.click(toggle);
    await waitFor(() => expect(saved()?.privacy.redact_patterns).toEqual([]));

    fireEvent.click(toggle);
    await waitFor(() => expect(saved()?.privacy.redact_patterns).toEqual([...DEFAULT_REDACT_PATTERNS]));
  });

  it("keeps history forever, or for a while", async () => {
    mount();
    const history = await row("Keep history for");
    const select = within(history).getByLabelText("Keep history for") as HTMLSelectElement;
    expect(select.value).toBe("Forever");

    fireEvent.change(select, { target: { value: "30 days" } });
    await waitFor(() => expect(saved()?.privacy.history_days).toBe(30));
  });

  it("says outright that nothing leaves the machine, and opens the folder that holds it", async () => {
    const t = mount();
    const sends = await row("Sends nothing anywhere");
    expect(sends.querySelector(".badge.ok")?.textContent).toBe("Local only");

    const data = await row("Your data");
    fireEvent.click(within(data).getByRole("button", { name: "Open folder" }));
    await waitFor(() => expect(t.calls.some((c) => c.cmd === "open_data_dir")).toBe(true));
  });
});

describe("Settings: Updates", () => {
  it("names the version this is", async () => {
    mount();
    const version = await row("Version");
    expect(version.querySelector(".kv")?.textContent).toMatch(/^\d+\.\d+\.\d+$/);
  });

  it("makes no request until asked, and then says what it found", async () => {
    const t = mount({}, {
      check_for_update: () => ({ available: false, version: null, notes: null, currentVersion: "1.0.1" }),
    });
    const check = await row("Check now");
    expect(t.calls.some((c) => c.cmd === "check_for_update")).toBe(false);

    fireEvent.click(within(check).getByRole("button", { name: "Check now" }));
    await waitFor(() => expect(t.calls.some((c) => c.cmd === "check_for_update")).toBe(true));
    expect(await screen.findByText("You are on the latest version (1.0.1).")).toBeTruthy();
  });

  it("offers the install only when there is one, and never downloads by itself", async () => {
    const t = mount({}, {
      check_for_update: () => ({ available: true, version: "1.1.0", notes: null, currentVersion: "1.0.1" }),
    });
    const check = await row("Check now");
    fireEvent.click(within(check).getByRole("button", { name: "Check now" }));

    const install = await within(check).findByRole("button", { name: "Install 1.1.0" });
    expect(t.calls.some((c) => c.cmd === "install_update")).toBe(false);
    fireEvent.click(install);
    await waitFor(() => expect(t.calls.some((c) => c.cmd === "install_update")).toBe(true));
  });

  it("reports a failed check rather than silently doing nothing", async () => {
    mount({}, { check_for_update: () => { throw new Error("offline"); } });
    const check = await row("Check now");
    fireEvent.click(within(check).getByRole("button", { name: "Check now" }));
    expect(await screen.findByText(/offline/)).toBeTruthy();
  });

  it("stops asking on launch when told to", async () => {
    mount();
    const launch = await row("Check on launch");
    fireEvent.click(within(launch).getByRole("switch"));
    await waitFor(() => expect(saved()?.updates.check_on_launch).toBe(false));
  });
});

describe("Settings: restart", () => {
  it("says which saved changes are still waiting on a restart, and offers one", async () => {
    const t = mount({}, { restart_reasons: () => ["your microphone", "the speech model"] });
    const notice = (await screen.findByRole("status")) as HTMLElement;
    expect(notice.textContent).toContain("your microphone and the speech model");

    fireEvent.click(within(notice).getByRole("button", { name: "Restart now" }));
    await waitFor(() => expect(t.calls.some((c) => c.cmd === "restart_app")).toBe(true));
  });

  it("stays quiet when nothing is waiting", async () => {
    mount();
    await row("Shortcut");
    expect(screen.queryByRole("status")).toBeNull();
  });
});
