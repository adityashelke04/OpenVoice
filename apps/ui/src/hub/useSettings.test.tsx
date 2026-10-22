import { afterEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useSettings } from "./useSettings";
import { installTauri } from "../test/tauri";

let tauri: ReturnType<typeof installTauri> | null = null;
afterEach(() => { tauri?.uninstall(); tauri = null; vi.useRealTimers(); });

function Probe() {
  const { settings } = useSettings();
  return <div>{settings ? `model ${settings.model}` : "loading"}</div>;
}

describe("useSettings", () => {
  // The Hub's webview can call get_settings before Rust has registered its
  // state; that first call rejects. Giving up on it left every screen but Home
  // on skeletons until the app was restarted.
  it("retries a load that fails at startup", async () => {
    let attempts = 0;
    tauri = installTauri({
      get_settings: () => {
        attempts += 1;
        if (attempts < 3) throw "state not managed for field `state` on command `get_settings`";
        return { model: "parakeet", config: {}, dictionary: [], profiles: [] };
      },
    });
    render(<Probe />);
    expect(screen.getByText("loading")).toBeTruthy();
    await waitFor(() => expect(screen.getByText("model parakeet")).toBeTruthy(), { timeout: 3000 });
    expect(attempts).toBe(3);
  });
});

function Editor() {
  const { settings, patch, error } = useSettings();
  return <><div>{settings?.model}</div><div>{String(settings?.config.sound_enabled)}</div><div>{error}</div>
    <button onClick={() => void patch(s => { s.model = "new"; })}>Model</button>
    <button onClick={() => void patch(s => { s.config.sound_enabled = false; })}>Mute</button></>;
}

it("serializes successive edits without losing the earlier change", async () => {
  let finish: (value: unknown) => void = () => {};
  let count = 0;
  let stored = { model: "old", config: { sound_enabled: true }, dictionary: [], profiles: [] };
  tauri = installTauri({
    get_settings: () => stored,
    save_settings: ({ settings }) => ++count === 1 ? new Promise(resolve => { finish = value => { stored = value as typeof stored; resolve(value); }; }) : (stored = settings),
  });
  render(<Editor />);
  await screen.findByText("old");
  fireEvent.click(screen.getByText("Model"));
  fireEvent.click(screen.getByText("Mute"));
  await waitFor(() => expect(count).toBeGreaterThan(0));
  expect(count).toBe(1);
  await act(async () => { finish({ model: "new", config: { sound_enabled: true }, dictionary: [], profiles: [] }); });
  await waitFor(() => expect(screen.getByText("false")).toBeTruthy());
  expect(screen.getByText("new")).toBeTruthy();
  const saves = tauri.calls.filter(c => c.cmd === "save_settings");
  expect(saves.at(-1)?.args.settings).toMatchObject({ model: "new", config: { sound_enabled: false } });
});

it("consumes saved settings in another window", async () => {
  tauri = installTauri({ get_settings: () => ({ model: "old", config: { sound_enabled: true }, dictionary: [], profiles: [] }) });
  render(<Editor />);
  await screen.findByText("old");
  await waitFor(() => expect(tauri!.calls.some(c => c.cmd === "plugin:event|listen")).toBe(true));
  act(() => tauri!.emit("settings-saved", { model: "old", config: { sound_enabled: false }, dictionary: [], profiles: [] }));
  expect(screen.getByText("false")).toBeTruthy();
});


it("keeps persisted settings after live application fails and on the next save", async () => {
  let stored = { model: "old", config: { sound_enabled: true }, dictionary: [], profiles: [] };
  tauri = installTauri({
    get_settings: () => stored,
    save_settings: ({ settings }) => (stored = settings),
    restart_reasons: () => ["hotkey rebind failed"],
  });
  render(<Editor />);
  await screen.findByText("old");
  fireEvent.click(screen.getByText("Model"));
  await screen.findByText("Saved. Restart OpenVoice to apply: hotkey rebind failed.");
  expect(screen.getByText("new")).toBeTruthy();
  fireEvent.click(screen.getByText("Mute"));
  await screen.findByText("false");
  expect(stored.model).toBe("new");
});

it("ignores out-of-order saved events", async () => {
  tauri = installTauri({ get_settings: () => ({ revision: 1, model: "old", config: {}, dictionary: [], profiles: [] }) });
  render(<Editor />);
  await screen.findByText("old");
  await waitFor(() => expect(tauri!.calls.some(c => c.cmd === "plugin:event|listen")).toBe(true));
  act(() => {
    tauri!.emit("settings-saved", { revision: 3, model: "new", config: {}, dictionary: [], profiles: [] });
    tauri!.emit("settings-saved", { revision: 2, model: "stale", config: {}, dictionary: [], profiles: [] });
  });
  expect(screen.getByText("new")).toBeTruthy();
});

it("shows a read-only settings load warning before an edit is attempted", async () => {
  tauri = installTauri({ get_settings: () => ({ model: "old", config: {}, dictionary: [], profiles: [], load_warning: "Settings unreadable; original preserved; fix permissions and retry." }) });
  render(<Editor />);
  expect(await screen.findByText("Settings unreadable; original preserved; fix permissions and retry.")).toBeTruthy();
});
