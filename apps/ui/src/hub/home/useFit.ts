/** Show as many Earlier rows as fit, and never half a row (spec 6.2).
 *
 *  The list has no scrollbar of its own: History is where scrolling lives. So
 *  after layout every row whose bottom would cross the container's is hidden
 *  outright, and a day label left with no visible row under it goes too, or the
 *  list would end on "YESTERDAY" and nothing. The open row is never hidden: it
 *  is the one the user is looking at, so it clips instead.
 *
 *  Re-runs after every render that changes `deps` and whenever the container
 *  resizes (window resize, the card above growing). Everything is un-hidden
 *  first, so a taller window gets its rows back. */
import { useLayoutEffect, type DependencyList, type RefObject } from "react";

export function fitChildren(box: HTMLElement) {
  const kids = [...box.children] as HTMLElement[];
  for (const k of kids) k.hidden = false;
  const limit = box.getBoundingClientRect().bottom;
  for (const k of kids) {
    if (k.classList.contains("day") || k.getAttribute("aria-expanded") === "true") continue;
    if (k.getBoundingClientRect().bottom > limit + 0.5) k.hidden = true;
  }
  // A day label needs at least one visible row before the next label.
  kids.forEach((k, i) => {
    if (!k.classList.contains("day")) return;
    let shown = false;
    for (let j = i + 1; j < kids.length && !kids[j].classList.contains("day"); j++) {
      if (!kids[j].hidden) { shown = true; break; }
    }
    k.hidden = !shown;
  });
}

export function useFit(ref: RefObject<HTMLElement | null>, deps: DependencyList) {
  useLayoutEffect(() => {
    const box = ref.current;
    if (!box) return;
    fitChildren(box);
    let ro: ResizeObserver | undefined;
    try {
      ro = new ResizeObserver(() => fitChildren(box));
      ro.observe(box);
    } catch { /* no ResizeObserver: the first fit stands */ }
    return () => ro?.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
