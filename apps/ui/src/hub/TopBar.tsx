/** The Hub top bar (reference lines 442-452, spec 6.1).
 *
 *  Home greets by time of day and, when Windows knows it, by first name (the
 *  name in the handwritten face is the one human touch the reference allows).
 *  Every other screen shows its title with the icon its sidebar item uses. The
 *  search button opens the Ctrl K palette; "Stays on this PC" is a fact about
 *  the app, not a setting, so it is plain text. */
import {
  BookOpenText, ClockCounterClockwise, Flask, LockSimple, MagnifyingGlass, PenNib, SlidersHorizontal, Waveform, type Icon,
} from "@phosphor-icons/react";
import { greeting } from "./greeting";
import type { ScreenId } from "./nav";

const TITLES: Record<Exclude<ScreenId, "home">, { title: string; Icon: Icon }> = {
  history: { title: "History", Icon: ClockCounterClockwise },
  dictionary: { title: "Dictionary", Icon: BookOpenText },
  style: { title: "Writing style", Icon: PenNib },
  models: { title: "Speech model", Icon: Waveform },
  settings: { title: "Settings", Icon: SlidersHorizontal },
  advanced: { title: "Advanced", Icon: Flask },
};

export interface TopBarProps {
  screen: ScreenId;
  userName: string | null;
  now: Date;
  onSearch: () => void;
}

export function TopBar({ screen, userName, now, onSearch }: TopBarProps) {
  let heading;
  if (screen === "home") {
    const name = userName?.trim() || null;
    heading = (
      <h1 className="hi">
        {greeting(now)}{name ? ", " : ""}{name && <span className="name">{name}</span>}
      </h1>
    );
  } else {
    const { title, Icon } = TITLES[screen];
    heading = <h1 className="page-title"><Icon weight="regular" aria-hidden="true" />{title}</h1>;
  }

  return (
    <div className="top">
      {heading}
      <button type="button" className="search glass" aria-keyshortcuts="Control+K" onClick={onSearch}>
        <MagnifyingGlass size="1em" weight="regular" aria-hidden="true" />
        Search what you've said
        <span className="k" aria-hidden="true">Ctrl K</span>
      </button>
      <div className="lock">
        <LockSimple size="1em" weight="fill" aria-hidden="true" />
        Stays on this PC
      </div>
    </div>
  );
}
