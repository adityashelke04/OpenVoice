/** Show as many Earlier rows as fit, and never half a row (spec 6.2).
 *
 *  The list has no scrollbar of its own: History is where scrolling lives. So
 *  after layout every row whose bottom would cross the container's is hidden
 *  outright, and a day label left with no visible row under it goes too, or the
 *  list would end on "YESTERDAY" and nothing.
 *
 *  Except while a row is open. An open row (and its Fix a word panel) can be
 *  taller than the space below it, and clipping the actions the user just asked
 *  for is worse than a scrollbar. So while any row is open nothing is hidden,
 *  the list scrolls (Earlier sets `.rows.scrolling`), and the open row scrolls
 *  itself into view (`revealInList`). Closing it returns to the fitted list,
 *  scrolled back to the top.
 *
 *  Re-runs after every render that changes `deps` and whenever the container
 *  resizes (window resize, the card above growing). Everything is un-hidden
 *  first, so a taller window gets its rows back. */
import { useLayoutEffect, type DependencyList, type RefObject } from "react";

const anyOpen = (box: HTMLElement) => box.querySelector('[aria-expanded="true"]') !== null;

export function fitChildren(box: HTMLElement) {
  const kids = [...box.children] as HTMLElement[];
  for (const k of kids) k.hidden = false;
  if (anyOpen(box)) return;
  box.scrollTop = 0;
  const limit = box.getBoundingClientRect().bottom;
  for (const k of kids) {
    if (k.classList.contains("day")) continue;
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

/** Scroll the list holding `el` just enough to show it ("nearest"), or its top
 *  if it is taller than the list. Only the list moves: `scrollIntoView` would
 *  also scroll the page's overflow-hidden ancestors when the row is taller than
 *  they are, shifting the whole screen. */
export function revealInList(el: HTMLElement) {
  const list = el.closest<HTMLElement>(".rows");
  if (!list) return;
  const b = list.getBoundingClientRect(), r = el.getBoundingClientRect();
  if (r.top < b.top) list.scrollTop -= b.top - r.top;
  else if (r.bottom > b.bottom) list.scrollTop += Math.min(r.bottom - b.bottom, r.top - b.top);
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
