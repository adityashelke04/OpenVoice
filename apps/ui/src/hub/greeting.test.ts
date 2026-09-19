import { expect, it } from "vitest";
import { greeting } from "./greeting";
const at = (h: number, m = 0) => new Date(2026, 8, 18, h, m);
it.each([[4, 59, "Good evening"], [5, 0, "Good morning"], [11, 59, "Good morning"], [12, 0, "Good afternoon"], [14, 43, "Good afternoon"], [16, 59, "Good afternoon"], [17, 0, "Good evening"], [23, 0, "Good evening"]] as const)(
  "%i:%i", (h, m, g) => expect(greeting(at(h, m))).toBe(g));
