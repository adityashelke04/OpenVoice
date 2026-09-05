/** Test config, kept apart from `vite.config.ts`.
 *
 * The build config is what ships the app; putting a test block inside it means
 * every production build carries test settings it will never use. Separate file,
 * separate job.
 */
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // The hook under test schedules real timers against `window`, so it needs a
    // DOM. Nothing here renders to a screen.
    environment: "jsdom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
  },
});
