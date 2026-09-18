import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { AppChip, Badge, Button, Card, Field, Keycap, Notice, Segmented, SelectField, SettingRow, Slider, StatusChip, Switch } from ".";
import { installTauri } from "../../test/tauri";

describe("Switch", () => {
  it("is a switch with aria-checked and toggles", () => {
    const on = vi.fn();
    render(<Switch checked={false} onChange={on} label="Sound feedback" />);
    const s = screen.getByRole("switch", { name: "Sound feedback" });
    expect(s.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(s);
    expect(on).toHaveBeenCalledWith(true);
  });
  it("carries the reference .on class when checked and turns off on click", () => {
    const on = vi.fn();
    render(<Switch checked onChange={on} label="Launch at login" />);
    const s = screen.getByRole("switch", { name: "Launch at login" });
    expect(s.className).toBe("switch on");
    fireEvent.click(s);
    expect(on).toHaveBeenCalledWith(false);
  });
});

describe("Segmented", () => {
  const opts = [{ value: "all", label: "All" }, { value: "editor", label: "Code" }, { value: "terminal", label: "Terminal" }] as const;
  it("marks the selected tab and moves with arrow keys", () => {
    const on = vi.fn();
    render(<Segmented value="all" options={[...opts]} onChange={on} label="Filter" />);
    expect(screen.getByRole("tablist", { name: "Filter" })).toBeTruthy();
    const all = screen.getByRole("tab", { name: "All" });
    expect(all.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(all, { key: "ArrowRight" });
    expect(on).toHaveBeenLastCalledWith("editor");
    expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Code" })); // focus follows
    fireEvent.keyDown(all, { key: "ArrowLeft" });
    expect(on).toHaveBeenLastCalledWith("terminal"); // wraps
    expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Terminal" }));
    fireEvent.click(screen.getByRole("tab", { name: "Terminal" }));
    expect(on).toHaveBeenLastCalledWith("terminal");
  });
  it("uses a roving tabindex, Home/End, and the lg size class", () => {
    const on = vi.fn();
    render(<Segmented value="editor" options={[...opts]} onChange={on} label="Filter" size="lg" />);
    expect(screen.getByRole("tablist").className).toBe("seg lg");
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((t) => t.tabIndex)).toEqual([-1, 0, -1]);
    expect(tabs[1].className).toBe("on");
    fireEvent.keyDown(tabs[1], { key: "End" });
    expect(on).toHaveBeenLastCalledWith("terminal");
    fireEvent.keyDown(tabs[1], { key: "Home" });
    expect(on).toHaveBeenLastCalledWith("all");
  });
});

describe("Button", () => {
  it("variants map to reference classes and show a key hint", () => {
    render(<><Button variant="primary" kbd="Ctrl C">Copy</Button><Button variant="warn">Paste again</Button><Button size="sm" disabled>Download</Button></>);
    expect(screen.getByRole("button", { name: /Copy/ }).className).toBe("btn primary");
    expect(screen.getByText("Ctrl C").className).toBe("k");
    expect(screen.getByRole("button", { name: "Paste again" }).className).toBe("btn warnp");
    const dl = screen.getByRole("button", { name: "Download" }) as HTMLButtonElement;
    expect(dl.disabled).toBe(true);
    expect(dl.className).toBe("btn sm");
  });
  it("defaults to type=button so it never submits a form by accident", () => {
    render(<Button>Save</Button>);
    expect(screen.getByRole("button", { name: "Save" }).getAttribute("type")).toBe("button");
  });
});

it("AppChip names the app and colours by profile", () => {
  render(<AppChip exe="Code.exe" profile="prose" />);
  const chip = screen.getByText("VS Code").closest(".app-chip")!;
  expect(chip.className).toBe("app-chip c-prose");
  expect(chip.querySelector("svg")).toBeTruthy();
});

describe("StatusChip", () => {
  it("says what happened in the reference copy", () => {
    render(<><StatusChip kind="clipboard" /><StatusChip kind="failed" /></>);
    expect(screen.getByText("Not pasted, on clipboard").closest("span")!.className).toBe("warn-chip");
    expect(screen.getByText("Failed").closest("span")!.className).toBe("fail-chip");
  });
});

describe("Field", () => {
  it("wraps a real input in label.field and passes input props through", () => {
    const on = vi.fn();
    render(<Field aria-label="What OpenVoice heard" mono placeholder="cube control" value="" onChange={on} />);
    const input = screen.getByRole("textbox", { name: "What OpenVoice heard" });
    expect(input.closest("label")!.className).toBe("field mono");
    fireEvent.change(input, { target: { value: "kube" } });
    expect(on).toHaveBeenCalled();
  });
});

describe("SelectField", () => {
  const options = [{ value: "a", label: "Microphone A" }, { value: "b", label: "Microphone B" }];
  it("shows the selected label and exposes a named native select", () => {
    const on = vi.fn();
    render(<SelectField value="a" options={options} onChange={on} label="Microphone" />);
    const sel = screen.getByRole("combobox", { name: "Microphone" }) as HTMLSelectElement;
    expect(sel.value).toBe("a");
    expect(sel.closest("label")!.className).toBe("field");
    // The visible copy duplicates the select's own value, so it is hidden from
    // assistive tech; the select alone carries name and value.
    expect(screen.getByText("Microphone A", { selector: "span" }).getAttribute("aria-hidden")).toBe("true");
    fireEvent.change(sel, { target: { value: "b" } });
    expect(on).toHaveBeenCalledWith("b");
  });
  it("honours a custom display and width", () => {
    render(<SelectField value="a" options={options} onChange={() => {}} label="Microphone" display="Default mic" width={220} />);
    expect(screen.getByText("Default mic")).toBeTruthy();
    expect((screen.getByRole("combobox").closest("label") as HTMLElement).style.width).toBe("220px");
  });
});

describe("Card, SettingRow, Keycap, Badge, Notice", () => {
  it("render the reference markup", () => {
    const { container } = render(
      <Card title="Microphone" icon={<svg />} right="Right side">
        <SettingRow label="Sound feedback" hint="A soft click when recording starts.">
          <Keycap>Right Ctrl</Keycap>
        </SettingRow>
        <Badge tone="ok">Active</Badge>
        <Badge>Downloaded</Badge>
      </Card>,
    );
    const card = container.querySelector("article")!;
    expect(card.className).toBe("card glass");
    expect(card.querySelector(".card-head > .card-title")!.textContent).toBe("Microphone");
    expect(card.querySelector(".card-head > .right.cap")!.textContent).toBe("Right side");
    expect(card.querySelector(".srow .lab")!.textContent).toBe("Sound feedback");
    expect(card.querySelector(".srow .hint")!.textContent).toBe("A soft click when recording starts.");
    expect(screen.getByText("Right Ctrl").className).toBe("keycap");
    expect(screen.getByText("Active").className).toBe("badge ok");
    expect(screen.getByText("Downloaded").className).toBe("badge");
  });
  it("Card accepts a className and omits the right slot when not given", () => {
    const { container } = render(<Card title="Try it" icon={null} className="tryout" as="section">x</Card>);
    const el = container.firstElementChild!;
    expect(el.tagName).toBe("SECTION");
    expect(el.className).toBe("card glass tryout");
    expect(el.querySelector(".right")).toBeNull();
  });
  it("Notice is a status region with an optional action", () => {
    render(<Notice tone="warn" action={<Button size="sm">Retry</Button>}>Engine is not running.</Notice>);
    const n = screen.getByRole("status");
    expect(n.className).toBe("notice glass warn");
    expect(n.textContent).toContain("Engine is not running.");
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });
});

describe("Slider", () => {
  const stops = [30, 60, 120, 300];
  const fmt = (v: number) => `${v}s`;
  it("is a named slider whose range spans its stops", () => {
    render(<Slider value={120} stops={stops} min={30} max={300} format={fmt} label="Maximum recording" onChange={() => {}} />);
    const s = screen.getByRole("slider", { name: "Maximum recording" });
    expect(s.getAttribute("aria-valuemin")).toBe("30");
    expect(s.getAttribute("aria-valuemax")).toBe("300");
  });
  it("reports the reachable range (first and last stop), not the track's", () => {
    const { container } = render(<Slider value={120} stops={[60, 120, 300]} min={30} max={300} format={fmt} label="Max" onChange={() => {}} />);
    const s = screen.getByRole("slider");
    expect(s.getAttribute("aria-valuemin")).toBe("60");
    expect(s.getAttribute("aria-valuemax")).toBe("300");
    // positioning still uses the track's min/max
    expect((container.querySelector(".slider .tr b") as HTMLElement).style.width).toBe(`${((120 - 30) / 270) * 100}%`);
  });
  it("positions by value on the track", () => {
    const { container } = render(<Slider value={120} stops={stops} min={30} max={300} format={fmt} label="Maximum recording" onChange={() => {}} />);
    const s = screen.getByRole("slider", { name: "Maximum recording" });
    expect(s.getAttribute("aria-valuenow")).toBe("120");
    expect(s.getAttribute("aria-valuetext")).toBe("120s");
    expect(container.querySelector(".slider .val")!.textContent).toBe("120s");
    const pct = `${((120 - 30) / 270) * 100}%`;
    expect((container.querySelector(".slider .tr b") as HTMLElement).style.width).toBe(pct);
    expect((container.querySelector(".slider .tr i") as HTMLElement).style.left).toBe(pct);
  });
  it("steps between stops with the arrow keys and clamps at the ends", () => {
    const on = vi.fn();
    const { rerender } = render(<Slider value={60} stops={stops} min={30} max={300} format={fmt} label="Max" onChange={on} />);
    const s = screen.getByRole("slider");
    fireEvent.keyDown(s, { key: "ArrowRight" });
    expect(on).toHaveBeenLastCalledWith(120);
    fireEvent.keyDown(s, { key: "ArrowLeft" });
    expect(on).toHaveBeenLastCalledWith(30);
    rerender(<Slider value={300} stops={stops} min={30} max={300} format={fmt} label="Max" onChange={on} />);
    on.mockClear();
    fireEvent.keyDown(s, { key: "ArrowRight" });
    expect(on).not.toHaveBeenCalled();
  });
  it("snaps a pointer press to the nearest stop", () => {
    const on = vi.fn();
    const { container } = render(<Slider value={60} stops={stops} min={30} max={300} format={fmt} label="Max" onChange={on} />);
    const tr = container.querySelector(".slider .tr") as HTMLElement;
    tr.getBoundingClientRect = () => ({ left: 0, width: 270, top: 0, height: 6, right: 270, bottom: 6, x: 0, y: 0, toJSON() {} });
    fireEvent.pointerDown(tr, { clientX: 260, pointerId: 1, button: 2 }); // right-click: ignored
    expect(on).not.toHaveBeenCalled();
    fireEvent.pointerDown(tr, { clientX: 260, pointerId: 1, button: 0 });
    expect(on).toHaveBeenLastCalledWith(300);
  });
});

describe("installTauri", () => {
  it("routes invoke to handlers, records calls and uninstalls", async () => {
    const t = installTauri({ get_user_name: () => "Aditya" });
    const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
    expect(await internals.invoke("get_user_name", {})).toBe("Aditya");
    expect(await internals.invoke("plugin:event|listen", {})).toBe(1);
    expect(await internals.invoke("unknown", {})).toBeNull();
    expect(t.calls.map((c) => c.cmd)).toEqual(["get_user_name", "plugin:event|listen", "unknown"]);
    t.uninstall();
    expect("__TAURI_INTERNALS__" in window).toBe(false);
  });
  it("supports the real listen/unlisten round trip, and only answers its own handlers", async () => {
    const t = installTauri({});
    const { listen } = await import("@tauri-apps/api/event");
    const unlisten = await listen("engine", () => {});
    await expect(unlisten()).resolves.toBeUndefined();
    expect(t.calls.map((c) => c.cmd)).toEqual(["plugin:event|listen", "plugin:event|unlisten"]);
    const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
    expect(await internals.invoke("toString", {})).toBeNull(); // inherited keys are not handlers
    t.uninstall();
    expect("__TAURI_EVENT_PLUGIN_INTERNALS__" in window).toBe(false);
  });
});
