import { createElement } from "react";
import { describe, expect, it, vi } from "vitest";
import { flowAmplitude, FlowWave, normalizeFlowRms } from "./FlowWave";
import { render } from "@testing-library/react";
describe("live waveform ballistics", () => {
  it("responds on the first audio frame instead of filling a history buffer", () => {
    expect(flowAmplitude(0, .2, 16.667)).toBeCloseTo(.03, 3);
  });
  it("rejects invalid samples and clamps loud inputs", () => {
    expect(flowAmplitude(0, Number.NaN, 16)).toBe(0);
    expect(flowAmplitude(0, Infinity, 16)).toBe(0);
    expect(flowAmplitude(0, -1, 16)).toBe(0);
    expect(flowAmplitude(1, 50, 16)).toBe(1);
  });
  it("has the same response after equal time at 60 and 120 Hz", () => {
    const full = flowAmplitude(0, .15, 1000 / 60);
    const half = flowAmplitude(flowAmplitude(0, .15, 1000 / 120), .15, 1000 / 120);
    expect(full).toBeCloseTo(half, 8);
  });
});

it("stops sampling while the overlay document is hidden and resumes once", () => {
  const request = vi.spyOn(window, "requestAnimationFrame").mockReturnValue(7);
  const cancel = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
  const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(false);
  const { unmount } = render(createElement(FlowWave, { active: true, level: .5 }));
  expect(request).toHaveBeenCalledTimes(1);
  hidden.mockReturnValue(true);
  document.dispatchEvent(new Event("visibilitychange"));
  expect(cancel).toHaveBeenCalledWith(7);
  hidden.mockReturnValue(false);
  document.dispatchEvent(new Event("visibilitychange"));
  expect(request).toHaveBeenCalledTimes(2);
  unmount();
  request.mockRestore();cancel.mockRestore();hidden.mockRestore();
});

it("makes quiet speech visibly distinct from silence and louder speech", () => {
  const silence = normalizeFlowRms(0);
  const quiet = normalizeFlowRms(.02);
  const speech = normalizeFlowRms(.08);
  expect(silence).toBe(0);
  expect(Math.max(1, quiet * 5)).toBeGreaterThan(2);
  expect(Math.max(1, speech * 5)).toBeGreaterThan(Math.max(1, quiet * 5) + 1);
  expect(normalizeFlowRms(Number.NaN)).toBe(0);
  expect(normalizeFlowRms(Infinity)).toBe(0);
  expect(normalizeFlowRms(-1)).toBe(0);
  expect(normalizeFlowRms(2)).toBe(1);
});
