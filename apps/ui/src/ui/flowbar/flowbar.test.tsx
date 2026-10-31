import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { FlowBar } from "./FlowBar";

const bar = (c: HTMLElement) => c.querySelector<HTMLElement>(".flowbar")!;

describe("FlowBar at rest", () => {
  it("shows a quiet microphone and no instruction text", () => {
    const { container } = render(<FlowBar live={false} elapsed="0:00" />);
    expect(container.querySelector(".flowbar-ready svg")).toBeTruthy();
    expect(container.textContent).not.toMatch(/hold/i);
    expect(container.querySelector(".kbd")).toBeNull();
  });

  it("is a button named for its shortcut, only when it can start a dictation", () => {
    let n = 0;
    const { rerender } = render(
      <FlowBar live={false} elapsed="0:00" hint="Ctrl+Alt+Space" onToggle={() => n++} />,
    );
    const go = screen.getByRole("button", { name: "Start dictating" });
    expect(go.getAttribute("title")).toBe("Hold Ctrl+Alt+Space, or click to dictate");
    fireEvent.click(go);
    expect(n).toBe(1);
    rerender(<FlowBar live={false} elapsed="0:00" />);
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("keeps the mark when docked to a side edge", () => {
    const { container } = render(<FlowBar live={false} elapsed="0:00" edge="left" />);
    expect(bar(container).dataset.column).toBe("true");
    expect(container.querySelector(".flowbar-ready svg")).toBeTruthy();
  });

  it("keeps the mark when compact", () => {
    const { container } = render(<FlowBar live={false} mini elapsed="0:00" />);
    expect(container.querySelector(".flowbar-ready svg")).toBeTruthy();
  });
});

describe("FlowBar while listening", () => {
  it("renders ten responsive bars and keeps the held-session surface minimal", () => {
    let cancelled = 0;
    const { container } = render(
      <FlowBar live latched level={0.5} elapsed="0:04" onCancel={() => cancelled++} />,
    );
    expect(container.querySelectorAll(".flowbar-wave .wave-bar")).toHaveLength(10);
    expect(container.querySelector(".flowbar-time")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Discard this dictation" }));
    expect(cancelled).toBe(1);
    expect(container.querySelector(".flowbar-mark")).toBeNull();
  });

  it("compact: ten bars, no timer, no discard", () => {
    const { container } = render(
      <FlowBar live mini level={0.5} elapsed="0:04" onCancel={() => {}} />,
    );
    expect(container.querySelectorAll(".wave-bar")).toHaveLength(10);
    expect(screen.queryByText("0:04")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("latched says hands-free, and a held session does not", () => {
    const { rerender } = render(<FlowBar live latched level={0.5} elapsed="1:12" />);
    expect(screen.getByRole("img", { name: "Hands-free" })).toBeTruthy();
    rerender(<FlowBar live level={0.5} elapsed="1:12" />);
    expect(screen.queryByRole("img", { name: "Hands-free" })).toBeNull();
  });
});

describe("FlowBar between and after", () => {
  it("distinguishes local transcription from text delivery", () => {
    const { rerender } = render(<FlowBar live={false} working workingPhase="transcribing" elapsed="0:00" />);
    expect(screen.getByRole("status", { name: "Transcribing" })).toBeTruthy();
    rerender(<FlowBar live={false} working workingPhase="injecting" elapsed="0:00" />);
    expect(screen.getByRole("status", { name: "Pasting" })).toBeTruthy();
    expect(screen.queryByText("Transcribing")).toBeNull();
  });
  it("processing retains the waveform with an eight-tick spinner", () => {
    const { container } = render(<FlowBar live={false} working elapsed="0:00" />);
    expect(screen.getByRole("status", { name: "Writing" })).toBeTruthy();
    expect(container.querySelectorAll(".flowbar-spinner i")).toHaveLength(8);
  });

  it("working, compact: the same informative spinner", () => {
    const { container } = render(<FlowBar live={false} working mini elapsed="0:00" />);
    expect(container.querySelectorAll(".flowbar-spinner i")).toHaveLength(8);
    expect(screen.queryByText("Writing")).toBeNull();
  });

  it.each([
    [{ failed: true, message: "No text was produced" }, "warn"],
    [{ status: "error" as const }, "warn"],
    [{ message: "Copied to clipboard" }, "clipboard"],
    [{ message: "Discarded" }, "info"],
    [{ status: "loading" as const }, "model"],
  ])("%o gets the %s icon", (props, icon) => {
    const { container } = render(<FlowBar live={false} elapsed="0:00" {...props} />);
    expect(container.querySelector(".flowbar-icon")?.getAttribute("data-icon")).toBe(icon);
  });

  it("says what happened in words", () => {
    render(<FlowBar live={false} failed message="No text was produced" elapsed="0:00" />);
    expect(screen.getByText("No text was produced")).toBeTruthy();
  });

  it("carries an action beside a notice", () => {
    render(
      <FlowBar
        live={false}
        message="Copied to clipboard"
        action={<button type="button">Paste</button>}
        elapsed="0:00"
      />,
    );
    expect(screen.getByRole("button", { name: "Paste" })).toBeTruthy();
  });

  it("download progress is a progressbar with a percentage", () => {
    render(<FlowBar live={false} status="loading" progress={0.43} elapsed="0:00" />);
    const p = screen.getByRole("progressbar");
    expect(p.getAttribute("aria-valuenow")).toBe("43");
    expect(p.style.getPropertyValue("--p")).toBe("43%");
    expect(screen.getByText("43%")).toBeTruthy();
    expect(screen.getByText("Getting the speech model")).toBeTruthy();
  });

  it("clamps progress", () => {
    const { rerender } = render(
      <FlowBar live={false} status="loading" progress={1.7} elapsed="0:00" />,
    );
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("100");
    rerender(<FlowBar live={false} status="loading" progress={Number.NaN} elapsed="0:00" />);
    const p = screen.getByRole("progressbar");
    expect(p.hasAttribute("aria-valuenow")).toBe(false);
    expect(p.dataset.indeterminate).toBe("true");
  });

  it("starting the engine is an indeterminate rail", () => {
    render(<FlowBar live={false} status="loading" elapsed="0:00" />);
    expect(screen.getByRole("progressbar").dataset.indeterminate).toBe("true");
    expect(screen.getByText("Starting the speech engine")).toBeTruthy();
  });

  it("marks a clean landing on the bar itself", () => {
    const { container } = render(<FlowBar live={false} confirm elapsed="0:00" />);
    expect(bar(container).dataset.confirm).toBe("true");
  });
});

describe("FlowBar progress rail", () => {
  // The body carries the entrance animation, and an element with a transform
  // animation is the containing block for anything absolutely positioned inside
  // it. Inside the body, the rail rode the text baseline instead of the glass's
  // bottom edge.
  it("sits on the bar itself, outside the animated body", () => {
    const { container } = render(<FlowBar live={false} status="loading" progress={0.4} elapsed="0:00" />);
    const rail = container.querySelector(".flowbar-rail")!;
    expect(rail.parentElement?.classList.contains("flowbar")).toBe(true);
  });
});

describe("Wispr recording controls", () => {
  it("keeps ten bars and distinct stop/discard actions in hands-free mode", () => {
    const stop = vi.fn();
    const cancel = vi.fn();
    const bubble = vi.fn();
    const { container } = render(<div onClick={bubble}><FlowBar live latched elapsed="0:04" onToggle={stop} onCancel={cancel} /></div>);
    expect(container.querySelectorAll(".flowbar-wave .wave-bar")).toHaveLength(10);
    fireEvent.click(screen.getByRole("button", { name: "Stop dictation" }));
    expect(stop).toHaveBeenCalledOnce();
    expect(cancel).not.toHaveBeenCalled();
    expect(bubble).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Discard this dictation" }));
    expect(cancel).toHaveBeenCalledOnce();
  });
  it.each(["left", "right"] as const)("makes hands-free recording explicit on %s dock", (edge) => {
    render(<FlowBar live latched edge={edge} elapsed="0:04" onToggle={() => {}} />);
    expect(screen.getByRole("button", { name: "Stop dictation" })).toBeTruthy();
    expect(screen.getByRole("img", { name: "Hands-free" })).toBeTruthy();
  });
});
