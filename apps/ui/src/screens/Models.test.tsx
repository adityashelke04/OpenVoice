import { afterEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ModelsScreen } from "./Models";
import { useSettings } from "../hub/useSettings";
import { installTauri } from "../test/tauri";
import type { ModelSpec, Settings } from "../engine/settings";

/** Mirrors `ov_asr::catalog` as the twin's fixture stub does: Standard bundled
 *  and selected, the other two offered for download. */
const MODELS: ModelSpec[] = [
  { id: "parakeet-tdt-0.6b-v2", kind: "transducer", downloadMb: 631, diskMb: 631, bundled: true, englishOnly: true, installed: true, selected: true },
  { id: "parakeet-tdt-0.6b-v3", kind: "transducer", downloadMb: 465, diskMb: 640, bundled: false, englishOnly: false, installed: false, selected: false },
  { id: "whisper-tiny.en", kind: "whisper", downloadMb: 112, diskMb: 112, bundled: false, englishOnly: true, installed: false, selected: false },
];

/** The same catalogue with Light already fetched: the only state in which a
 *  model can be deleted or switched to. */
const WITH_LIGHT_INSTALLED = MODELS.map((m) =>
  m.id === "whisper-tiny.en" ? { ...m, installed: true } : m,
);

let tauri: ReturnType<typeof installTauri> | null = null;
afterEach(() => { tauri?.uninstall(); tauri = null; });

/** The real settings hook over a fake bridge, so a patch is what reaches `save_settings`. */
function Harness() {
  const { settings, patch } = useSettings();
  return settings ? <ModelsScreen settings={settings} patch={patch} /> : null;
}

function mount(models: ModelSpec[] = MODELS, extra: Record<string, (args: never) => unknown> = {}) {
  const settings = { config: {}, model: "parakeet-tdt-0.6b-v2", dictionary: [], profiles: [] } as unknown as Settings;
  tauri = installTauri({
    get_settings: () => settings,
    save_settings: ({ settings: s }: { settings: Settings }) => s,
    list_models: () => models,
    get_download: () => null,
    ...extra,
  });
  render(<Harness />);
  return tauri;
}

/** The name sits as a bare text node beside the badges, so a card is found by
 *  the one string that is an element of its own. */
const IDS = {
  Standard: "parakeet-tdt-0.6b-v2",
  Multilingual: "parakeet-tdt-0.6b-v3",
  Light: "whisper-tiny.en",
};
const cards = () => [...document.querySelectorAll<HTMLElement>(".model")];
const card = async (name: keyof typeof IDS) =>
  (await screen.findByText(IDS[name], { selector: ".id" })).closest(".model") as HTMLElement;
const saved = () => {
  const saves = tauri!.calls.filter((c) => c.cmd === "save_settings");
  return (saves[saves.length - 1]?.args.settings as Settings | undefined)?.model;
};

describe("Speech model screen", () => {
  it("leads with what the screen is for", async () => {
    mount();
    // Matched on the run after the <strong>: getByText sees an element's own
    // text nodes, not its descendants'.
    const lead = await screen.findByText(/is included and works offline from the moment/);
    expect(lead.className).toContain("lead");
    expect(lead.textContent).toBe(
      "Standard is included and works offline from the moment you install. The other two are optional and only downloaded if you ask. Everything runs on this machine either way.",
    );
  });

  it("lists every model in the catalogue as its own card", async () => {
    mount();
    await card("Standard");
    expect(cards()).toHaveLength(3);
    expect(cards().map((c) => c.querySelector(".id")?.textContent)).toEqual([
      "parakeet-tdt-0.6b-v2",
      "parakeet-tdt-0.6b-v3",
      "whisper-tiny.en",
    ]);
  });

  it("marks the model in use, and offers it no action", async () => {
    mount();
    const standard = await card("Standard");
    expect(standard.className).toContain("active");
    expect(standard.querySelector(".nm")?.firstChild?.textContent).toBe("Standard");
    const badges = [...standard.querySelectorAll(".badge")].map((b) => b.textContent);
    expect(badges).toEqual(["In use", "Included", "English only"]);
    expect(standard.querySelector(".badge.ok")?.textContent).toBe("In use");
    expect(within(standard).getByText("On disk")).toBeTruthy();
    expect(within(standard).getByText("631 MB")).toBeTruthy();
    expect(standard.querySelector(".act")?.querySelector("button")).toBeNull();
  });

  it("describes a model that is not here yet by what it would cost to fetch", async () => {
    mount();
    const multi = await card("Multilingual");
    expect(multi.className).not.toContain("active");
    expect(multi.querySelector(".nm")?.firstChild?.textContent).toBe("Multilingual");
    expect([...multi.querySelectorAll(".badge")].map((b) => b.textContent)).toEqual(["25 languages"]);
    expect(within(multi).getByText("Download", { selector: ".k" })).toBeTruthy();
    expect(within(multi).getByText("465 MB")).toBeTruthy();
    expect(within(multi).getByText("~0.6 s")).toBeTruthy();
  });

  it("fetches a model by its own id when Download is pressed", async () => {
    const t = mount();
    const multi = await card("Multilingual");
    fireEvent.click(within(multi).getByRole("button", { name: "Download" }));
    await waitFor(() => expect(t.calls.find((c) => c.cmd === "download_model")).toBeTruthy());
    expect(t.calls.find((c) => c.cmd === "download_model")?.args).toEqual({ id: "parakeet-tdt-0.6b-v3" });
  });

  it("counts the transfer up as a percentage while it runs", async () => {
    let finish = () => {};
    const t = mount(MODELS, {
      download_model: () => new Promise<void>((resolve) => { finish = resolve; }),
      get_download: () => ({ model: "parakeet-tdt-0.6b-v3", done: 93, total: 465 }),
    });
    const multi = await card("Multilingual");
    fireEvent.click(within(multi).getByRole("button", { name: "Download" }));
    await waitFor(
      () => expect(multi.querySelector(".act button")?.textContent).toBe("20%"),
      { timeout: 3000 },
    );
    expect(t.calls.some((c) => c.cmd === "get_download")).toBe(true);
    // Let the transfer settle inside the test: `download` re-reads the catalogue
    // afterwards, and that invoke must land before the fake bridge is removed.
    finish();
    await waitFor(() => expect(t.calls.filter((c) => c.cmd === "list_models")).toHaveLength(2));
  });

  it("offers to delete a model that was downloaded and is not in use", async () => {
    const t = mount(WITH_LIGHT_INSTALLED);
    const light = await card("Light");
    expect(light.querySelector(".nm")?.firstChild?.textContent).toBe("Light");
    expect([...light.querySelectorAll(".badge")].map((b) => b.textContent)).toEqual([
      "On this computer",
      "English only",
    ]);
    expect(within(light).getByText("On disk")).toBeTruthy();
    fireEvent.click(within(light).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(t.calls.find((c) => c.cmd === "delete_model")).toBeTruthy());
    expect(t.calls.find((c) => c.cmd === "delete_model")?.args).toEqual({ id: "whisper-tiny.en" });
    await waitFor(() => expect(t.calls.filter((c) => c.cmd === "list_models")).toHaveLength(2));
  });

  it("switches to an installed model and says a restart is needed to use it", async () => {
    const t = mount(WITH_LIGHT_INSTALLED);
    const light = await card("Light");
    fireEvent.click(light.querySelector(".model-select") as HTMLElement);
    await waitFor(() => expect(saved()).toBe("whisper-tiny.en"));

    const notice = document.querySelector(".notice.warn") as HTMLElement;
    expect(notice).toBeTruthy();
    expect(notice.textContent).toContain("Light");
    fireEvent.click(within(notice).getByRole("button", { name: "Restart now" }));
    await waitFor(() => expect(t.calls.some((c) => c.cmd === "restart_app")).toBe(true));
  });

  it("does not let a model that is not here yet be selected", async () => {
    mount();
    const multi = await card("Multilingual");
    expect((multi.querySelector(".model-select") as HTMLButtonElement).disabled).toBe(true);
  });

  it("says so while the catalogue is still being read", () => {
    const settings = { config: {}, model: "parakeet-tdt-0.6b-v2", dictionary: [], profiles: [] } as unknown as Settings;
    tauri = installTauri({
      get_settings: () => settings,
      list_models: () => new Promise(() => {}),
    });
    render(<Harness />);
    return waitFor(() => expect(screen.getByText("Reading the model list…")).toBeTruthy());
  });

  it("ends with the caption that qualifies the timings", async () => {
    mount();
    const cap = await screen.findByText(
      "Timings are measured on one particular laptop and are there for comparison, not as a promise.",
    );
    expect(cap.className).toContain("cap");
  });
});
