import type { CSSProperties } from "react";

/**
 * The logo's seven bars, as proportions of the tallest.
 *
 * Read off `BARS` in `hub/Logo.tsx` (height / 800), because the resting Flow Bar
 * is the logo: the same object that becomes the waveform the moment the
 * microphone opens. Rounded to two places; at 16px tall the third place is less
 * than a tenth of a pixel.
 */
const MARK_HEIGHTS = [0.22, 0.46, 0.88, 0.62, 1, 0.38, 0.18] as const;

/**
 * The Flow Bar at rest: the seven-bar mark, monochrome and still.
 *
 * It replaces "Hold [Right Ctrl]". Instruction text is useful exactly once, and
 * this is on screen all day; the shortcut lives in the Flow Menu's header and in
 * the idle button's tooltip and accessible name instead.
 */
export function FlowMark() {
  return (
    <span className="flowbar-mark" aria-hidden>
      {MARK_HEIGHTS.map((h, k) => (
        <i key={k} style={{ "--h": h, "--k": k } as CSSProperties} />
      ))}
    </span>
  );
}
