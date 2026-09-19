/** A stand-in for the Tauri IPC bridge in component tests.
 *
 * `@tauri-apps/api` talks to `window.__TAURI_INTERNALS__`; installing a fake one
 * lets real `invoke`/`listen` calls run in jsdom. `handlers` answer commands by
 * name (unknown commands resolve to null), event-plugin calls resolve to a listener
 * id, and every call is recorded in `calls` so a test can assert what was sent.
 *
 * The unlisten function `listen()` returns calls
 * `__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener` before it invokes
 * anything, so without that object any component that listens and then unmounts
 * throws during cleanup. */
/* eslint-disable @typescript-eslint/no-explicit-any */
export function installTauri(handlers: Record<string, (args: any) => unknown> = {}) {
  const calls: { cmd: string; args: any }[] = [];
  (window as any).__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "hub" }, currentWebview: { windowLabel: "hub", label: "hub" } },
    transformCallback: () => 1,
    invoke: async (cmd: string, args: any) => {
      calls.push({ cmd, args });
      if (cmd.startsWith("plugin:event|")) return 1;
      return Object.hasOwn(handlers, cmd) ? handlers[cmd](args) : null;
    },
  };
  (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  return {
    calls,
    uninstall: () => {
      delete (window as any).__TAURI_INTERNALS__;
      delete (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__;
    },
  };
}
