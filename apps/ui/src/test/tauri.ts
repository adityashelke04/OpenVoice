/** A stand-in for the Tauri IPC bridge in component tests.
 *
 * `@tauri-apps/api` talks to `window.__TAURI_INTERNALS__`; installing a fake one
 * lets real `invoke`/`listen` calls run in jsdom. `handlers` answer commands by
 * name (unknown commands resolve to null), event-plugin calls resolve to a listener
 * id, and every call is recorded in `calls` so a test can assert what was sent.
 *
 * `emit` delivers an event to whatever the code under test passed to `listen`,
 * which is the only way to drive anything the engine pushes rather than answers —
 * the live state machine, the level stream. Tauri parks each listener's callback
 * under a numeric id from `transformCallback` and sends that id with the
 * subscription, so the fake has to keep the same books the real bridge does.
 *
 * The unlisten function `listen()` returns calls
 * `__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener` before it invokes
 * anything, so without that object any component that listens and then unmounts
 * throws during cleanup. */
/* eslint-disable @typescript-eslint/no-explicit-any */
export function installTauri(handlers: Record<string, (args: any) => unknown> = {}) {
  const calls: { cmd: string; args: any }[] = [];
  const callbacks = new Map<number, (payload: unknown) => void>();
  const subscriptions = new Map<string, number[]>();
  let nextId = 1;

  (window as any).__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "hub" }, currentWebview: { windowLabel: "hub", label: "hub" } },
    transformCallback: (cb: (payload: unknown) => void) => {
      const id = nextId++;
      callbacks.set(id, cb);
      return id;
    },
    invoke: async (cmd: string, args: any) => {
      calls.push({ cmd, args });
      if (cmd === "plugin:event|listen") {
        const to = subscriptions.get(args.event) ?? [];
        to.push(args.handler);
        subscriptions.set(args.event, to);
        return nextId++;
      }
      if (cmd.startsWith("plugin:event|")) return 1;
      return Object.hasOwn(handlers, cmd) ? handlers[cmd](args) : null;
    },
  };
  (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };

  return {
    calls,
    /** Deliver `payload` to every listener attached to `event`. */
    emit(event: string, payload: unknown) {
      for (const id of subscriptions.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
    },
    uninstall: () => {
      callbacks.clear();
      subscriptions.clear();
      delete (window as any).__TAURI_INTERNALS__;
      delete (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__;
    },
  };
}
