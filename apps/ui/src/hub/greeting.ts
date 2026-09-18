export type Greeting = "Good morning" | "Good afternoon" | "Good evening";
export function greeting(d: Date): Greeting {
  const h = d.getHours();
  if (h >= 5 && h < 12) return "Good morning";
  if (h >= 12 && h < 17) return "Good afternoon";
  return "Good evening";
}
