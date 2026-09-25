import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { Waveform, waveEnvelope } from "../Waveform";

describe("waveEnvelope", () => {
  it("is symmetric, peaks in the middle, and stays in (0.35, 1]", () => {
    const n = 26;
    const v = Array.from({ length: n }, (_, i) => waveEnvelope(i, n));
    for (let i = 0; i < n; i++) expect(v[i]).toBeCloseTo(v[n - 1 - i], 10);
    const peak = Math.max(...v);
    expect(v[12]).toBe(peak);
    expect(v[13]).toBe(peak);
    expect(Math.min(...v)).toBeGreaterThan(0.35);
    expect(peak).toBeLessThanOrEqual(1);
  });
});

describe("Waveform", () => {
  it("indexes every bar so the stylesheet can place it on the spectrum", () => {
    const { container } = render(<Waveform bars={5} level={0.5} />);
    const ks = [...container.querySelectorAll<HTMLElement>(".wave-bar")].map((b) =>
      b.style.getPropertyValue("--k"),
    );
    expect(ks).toEqual(["0", "1", "2", "3", "4"]);
  });
});
