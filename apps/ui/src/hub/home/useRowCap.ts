/** How many children the Earlier list could possibly show, so it renders that
 *  many instead of rendering all two hundred and hiding the rest.
 *
 *  Measured in the real window before this existed: 208 children in the DOM,
 *  193 of them `hidden`, 15 visible — 2,403 DOM nodes and 398 icon SVGs so that
 *  fifteen rows could be read. Every re-render paid React, regex and layout
 *  costs for rows nobody could see.
 *
 *  Rows never wrap (`home.css` `.row .t` is `nowrap` with an ellipsis), so a
 *  child has a fixed height and the shortest one is a day label. Dividing the
 *  box by that can only ever over-count, which is the safe direction:
 *  `fitChildren` still trims whatever does not fit, it just no longer has two
 *  hundred children to walk. */
import { useLayoutEffect, useState, type RefObject } from "react";

/** The shortest a child can be: a day label (`home.css` `.day`). */
export const MIN_CHILD_PX = 25;

/** Before the box has been measured. Comfortably more than a 740 px window shows. */
export const DEFAULT_CAP = 40;

export function capFor(boxHeight: number): number {
  if (!boxHeight) return DEFAULT_CAP;
  return Math.ceil(boxHeight / MIN_CHILD_PX) + 2;
}

export function useRowCap(ref: RefObject<HTMLElement | null>): number {
  const [cap, setCap] = useState(DEFAULT_CAP);
  useLayoutEffect(() => {
    const box = ref.current;
    if (!box) return;
    const measure = () => setCap(capFor(box.getBoundingClientRect().height));
    measure();
    let ro: ResizeObserver | undefined;
    try {
      ro = new ResizeObserver(measure);
      ro.observe(box);
    } catch { /* no ResizeObserver: the first measure stands */ }
    return () => ro?.disconnect();
  }, [ref]);
  return cap;
}
