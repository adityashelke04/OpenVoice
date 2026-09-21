import { CheckCircle } from "@phosphor-icons/react";
import type { ThemeName } from "./theme";

/** One theme in Appearance (reference `.tcard`): a miniature of the Hub drawn in
 *  that theme's own colours, with its name underneath.
 *
 *  The card carries `data-theme` and `data-mode` itself, so every token inside it
 *  resolves to the theme it is offering rather than the one currently on. That is
 *  the whole point of the preview: themes.css keys on `[data-theme][data-mode]`,
 *  not on `:root`, which is what lets a subtree be a different theme.
 *
 *  `.themes` sets `--page-ink` for the name, which must stay readable against the
 *  page instead of turning near-invisible inside a light card on a dark page. */
export function ThemeCard({ theme, label, mode, selected, onSelect }: {
  theme: ThemeName;
  label: string;
  mode: "light" | "dark";
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      className={selected ? "tcard on" : "tcard"}
      aria-pressed={selected}
      data-theme={theme}
      data-mode={mode}
      onClick={onSelect}
    >
      {/* Spans, not the reference's divs: a <button> may only contain phrasing
          content, and these are laid out by class either way. */}
      <span className="mini" aria-hidden="true">
        <i /><i /><i />
        <span className="sb" /><span className="gl" /><span className="bt" /><span className="bt2" />
      </span>
      <span className="tn">
        {label}
        {selected && <CheckCircle weight="fill" aria-hidden />}
      </span>
    </button>
  );
}
