import { expect, it } from "vitest";
import { loadSettings } from "./settings";
import { installTauri } from "../test/tauri";

it("does not invoke a bridge removed while the API module is loading", async () => {
  const bridge = installTauri();
  const pending = loadSettings();
  bridge.uninstall();
  await expect(pending).resolves.toBeNull();
});
