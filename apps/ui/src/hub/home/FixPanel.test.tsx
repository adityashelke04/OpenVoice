import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { FixPanel } from "./FixPanel";
import type { Row } from "../../engine/stats";
import type { Settings } from "../../engine/settings";

it("retains a selected punctuated correction until persistence succeeds and restores focus", async () => {
  let succeed = false;
  let rule: Settings | undefined;
  const done = vi.fn();
  const patch = async (fn: (s: Settings) => void) => {
    const s = { dictionary: [] } as unknown as Settings;
    fn(s);
    rule = s;
    return succeed;
  };
  render(<div className="last"><button className="fix-toggle">Fix a word</button><FixPanel row={{ raw_text: "zima." } as Row} patch={patch} onDone={done} /></div>);
  fireEvent.click(screen.getByRole("button", { name: "zima." }));
  fireEvent.change(screen.getByLabelText("Write it as"), { target: { value: "Zyma" } });
  fireEvent.click(screen.getByText("Save"));
  await screen.findByRole("alert");
  expect(done).not.toHaveBeenCalled();
  expect((screen.getByLabelText("You said") as HTMLInputElement).value).toBe("zima.");
  expect((screen.getByLabelText("Write it as") as HTMLInputElement).value).toBe("Zyma");
  expect(rule?.dictionary[0]).toMatchObject({ spoken: ["zima."], written: "Zyma" });
  succeed = true;
  fireEvent.click(screen.getByText("Save"));
  await waitFor(() => expect(done).toHaveBeenCalledOnce());
  expect(document.activeElement).toBe(screen.getByText("Fix a word"));
});
