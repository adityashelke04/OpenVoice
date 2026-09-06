/** The Flow Menu's rows.
 *
 * The rows are a list, and a list is the shape of thing that quietly loses an
 * entry or grows a second one. Two properties matter enough to hold down:
 * which command each row actually sends — a row that closes the menu without
 * doing anything is indistinguishable from a working one at a glance — and
 * where the separators fall, because the menu's region is measured from the
 * paint and a doubled divider is a visible seam in an overlay that is supposed
 * to look like one solid pill.
 */

import { describe, expect, it, vi } from "vitest";

import { flowMenuRows, type FlowMenuActions, type FlowMenuState } from "./useFlowMenu";

/** An idle bar with nothing running: the state the menu is opened in. */
const IDLE: FlowMenuState = {
  mini: false,
  live: false,
  working: false,
  autoCollapse: true,
};

function actions(): FlowMenuActions & { calls: [string, unknown][] } {
  const calls: [string, unknown][] = [];
  return {
    calls,
    call: (cmd, args) => calls.push([cmd, args]),
    close: vi.fn(),
    setMini: vi.fn(),
    setAutoCollapse: vi.fn(),
  };
}

const find = (rows: ReturnType<typeof flowMenuRows>, id: string) => {
  const row = rows.find((r) => r.id === id);
  if (!row) throw new Error(`no row "${id}" in [${rows.map((r) => r.id).join(", ")}]`);
  return row;
};

describe("Back to center", () => {
  it("is offered in the menu", () => {
    expect(find(flowMenuRows(IDLE, actions()), "recenter").label).toBe("Back to center");
  });

  it("sends the bar home", () => {
    const a = actions();
    find(flowMenuRows(IDLE, a), "recenter").run();
    expect(a.calls).toEqual([["overlay_reset_position", undefined]]);
  });

  it("closes the menu behind it", () => {
    const a = actions();
    find(flowMenuRows(IDLE, a), "recenter").run();
    expect(a.close).toHaveBeenCalledTimes(1);
  });

  // The bar is still docked to a side edge while the menu is open on it, and
  // "Back to center" is the way out of a dock the user regrets. Hiding it there
  // would remove the escape hatch at exactly the moment it is wanted.
  it("is offered while the bar is dictating", () => {
    const rows = flowMenuRows({ ...IDLE, live: true }, actions());
    expect(rows.some((r) => r.id === "recenter")).toBe(true);
  });

  // It opens the third group -- the one about where the bar is and how big it
  // is, as distinct from dictation above it. The row that used to open that
  // group has to give the separator up: two adjacent rows both asking for one
  // paint two dividers, and the menu's region is measured from that paint.
  it("opens the placement group without doubling its divider", () => {
    const rows = flowMenuRows(IDLE, actions());
    const at = rows.findIndex((r) => r.id === "recenter");
    expect(rows[at].sep).toBe(true);
    expect(rows[at + 1].id).toBe("mini");
    expect(rows[at + 1].sep).toBeFalsy();
  });
});

describe("the rest of the menu", () => {
  it("still sends every command it used to", () => {
    const a = actions();
    for (const id of ["dictate", "paste", "history", "snooze", "dictate-only"]) {
      find(flowMenuRows(IDLE, a), id).run();
    }
    expect(a.calls).toEqual([
      ["toggle_session", undefined],
      ["paste_last", undefined],
      ["show_hub_cmd", { tab: "home" }],
      ["overlay_snooze", { minutes: 60 }],
      ["overlay_always_visible", { on: false }],
    ]);
  });

  it("hides the dictate row while a transcript is still being written", () => {
    const rows = flowMenuRows({ ...IDLE, working: true }, actions());
    expect(rows.some((r) => r.id === "dictate")).toBe(false);
  });
});
