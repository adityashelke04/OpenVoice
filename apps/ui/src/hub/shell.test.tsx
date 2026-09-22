import { describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { NAV, Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import { useHubKeys } from "./useHubKeys";
import { initialScreen } from "../windows/Hub";
import { pushToast, useToasts } from "./toast";
import { Toasts } from "./Toasts";
import { setMedia } from "../test/setup";
import { renderHook } from "@testing-library/react";

const base = { levelRef: { current: 0 }, listening: false, shortcut: "Right Ctrl" };

describe("hub shell", () => {
  it("sidebar lists the six sections in order with shortcuts", () => {
    render(<Sidebar screen="home" onNavigate={() => {}} engine="ready" {...base} />);
    const nav = screen.getByRole("navigation", { name: "Sections" });
    expect(within(nav).getAllByRole("button").map((b) => b.textContent)).toEqual(["Home1", "Dictionary2", "Writing style3", "Speech model4", "Settings5", "Advanced6"]);
    expect(NAV.map((n) => n.id)).toEqual(["home", "dictionary", "style", "models", "settings", "advanced"]);
  });
  it("history keeps Home current", () => {
    render(<Sidebar screen="history" onNavigate={() => {}} engine="ready" {...base} />);
    expect(screen.getByRole("button", { name: /Home/ }).getAttribute("aria-current")).toBe("page");
  });
  it("engine card states", () => {
    const { rerender } = render(<Sidebar screen="home" onNavigate={() => {}} engine="ready" {...base} />);
    expect(screen.getByText("Ready, on this PC")).toBeTruthy();
    rerender(<Sidebar screen="home" onNavigate={() => {}} engine="starting" {...base} />);
    expect(screen.getByText("Starting…")).toBeTruthy();
    rerender(<Sidebar screen="home" onNavigate={() => {}} engine="error" {...base} />);
    expect(screen.getByText("Not running")).toBeTruthy();
  });
  it("Ctrl+1..6, Ctrl+K and Escape", () => {
    const onScreen = vi.fn(), onPalette = vi.fn(), onEscape = vi.fn();
    renderHook(() => useHubKeys({ onScreen, onPalette, onEscape }));
    fireEvent.keyDown(window, { key: "3", code: "Digit3", ctrlKey: true });
    fireEvent.keyDown(window, { key: "k", code: "KeyK", ctrlKey: true });
    fireEvent.keyDown(window, { key: "Escape", code: "Escape" });
    expect(onScreen).toHaveBeenCalledWith("style");
    expect(onPalette).toHaveBeenCalled();
    expect(onEscape).toHaveBeenCalled();
  });
  it("matches keys by position, so non-US layouts work", () => {
    // AZERTY: the 1 key types "&" without Shift; French Ctrl+K is still KeyK.
    const onScreen = vi.fn(), onPalette = vi.fn();
    renderHook(() => useHubKeys({ onScreen, onPalette, onEscape: () => {} }));
    fireEvent.keyDown(window, { key: "&", code: "Digit1", ctrlKey: true });
    fireEvent.keyDown(window, { key: "3", code: "Numpad3", ctrlKey: true });
    expect(onScreen.mock.calls).toEqual([["home"]]);
  });
  it("ignores Shift, key repeat and IME composition", () => {
    const onScreen = vi.fn(), onPalette = vi.fn();
    renderHook(() => useHubKeys({ onScreen, onPalette, onEscape: () => {} }));
    fireEvent.keyDown(window, { key: "2", code: "Digit2", ctrlKey: true, isComposing: true });
    fireEvent.keyDown(window, { key: "@", code: "Digit2", ctrlKey: true, shiftKey: true });
    fireEvent.keyDown(window, { key: "K", code: "KeyK", ctrlKey: true, shiftKey: true });
    fireEvent.keyDown(window, { key: "2", code: "Digit2", ctrlKey: true, repeat: true });
    fireEvent.keyDown(window, { key: "k", code: "KeyK", ctrlKey: true, repeat: true });
    expect(onScreen).not.toHaveBeenCalled();
    expect(onPalette).not.toHaveBeenCalled();
  });
  it("nav titles only in the icon rail", () => {
    render(<Sidebar screen="home" onNavigate={() => {}} engine="ready" {...base} />);
    expect(screen.getByRole("button", { name: /Home/ }).getAttribute("title")).toBeNull();
    act(() => setMedia("(max-width: 959px)", true));
    expect(screen.getByRole("button", { name: /Home/ }).getAttribute("title")).toBe("Home");
    act(() => setMedia("(max-width: 959px)", false));
  });
  it("toasts land in one persistent status region", () => {
    render(<Toasts />);
    const region = screen.getByRole("status");
    expect(region.textContent).toBe("");
    act(() => { pushToast({ tone: "warn", message: "Copied instead" }); });
    expect(screen.getAllByRole("status")).toEqual([region]);
    expect(region.textContent).toContain("Copied instead");
    act(() => { screen.getByRole("button", { name: "Dismiss" }).click(); });
    expect(screen.getByRole("status")).toBe(region);
  });
  it("greeting with and without a name", () => {
    const now = new Date(2026, 8, 18, 14, 43);
    const { rerender } = render(<TopBar screen="home" userName="Aditya" now={now} onSearch={() => {}} />);
    expect(screen.getByRole("heading").textContent).toBe("Good afternoon, Aditya");
    rerender(<TopBar screen="home" userName={null} now={now} onSearch={() => {}} />);
    expect(screen.getByRole("heading").textContent).toBe("Good afternoon");
    rerender(<TopBar screen="history" userName="Aditya" now={now} onSearch={() => {}} />);
    expect(screen.getByRole("heading").textContent).toBe("History");
  });
  it("initialScreen reads ?screen=", () => {
    expect(initialScreen("?window=hub&screen=models")).toBe("models");
    expect(initialScreen("?screen=bogus")).toBe("home");
  });
  it("toasts auto-dismiss after 5 s", () => {
    vi.useFakeTimers();
    const h = renderHook(() => useToasts());
    act(() => { pushToast({ tone: "info", message: "Nothing to paste yet" }); });
    expect(h.result.current).toHaveLength(1);
    act(() => { vi.advanceTimersByTime(5000); });
    expect(h.result.current).toHaveLength(0);
    vi.useRealTimers();
  });
});
