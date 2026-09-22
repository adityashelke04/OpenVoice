import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
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
