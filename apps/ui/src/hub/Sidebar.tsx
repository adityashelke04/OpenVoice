/** The Hub sidebar (reference lines 409-438, spec 6.1).
 *
 *  Brand, the six sections, and a footer with the engine's state and the
 *  Light/System/Dark switch. Nav items are buttons, not links: they change a
 *  screen inside one window and have no URL to open elsewhere.
 *
 *  The active item's pill is one `motion` element shared by layoutId, so moving
 *  between sections slides it rather than blinking it off and on (spec 4.8). It
 *  is the only thing here that moves, and only on a click. */
import { Desktop, Moon, Sun } from "@phosphor-icons/react";
import { motion } from "motion/react";
import { Keycap } from "./ui";
import { Logo } from "./Logo";
import { NAV, type ScreenId } from "./nav";
import { useTheme, type ModeChoice } from "./theme";

export { NAV };

export type EngineState = "ready" | "starting" | "error";

const ENGINE_LABEL: Record<EngineState, string> = {
  ready: "Ready, on this PC",
  starting: "Starting…",
  error: "Not running",
};

const MODES: { id: ModeChoice; label: string; Icon: typeof Sun }[] = [
  { id: "light", label: "Light", Icon: Sun },
  { id: "system", label: "System", Icon: Desktop },
  { id: "dark", label: "Dark", Icon: Moon },
];

export interface SidebarProps {
  screen: ScreenId;
  onNavigate: (id: ScreenId) => void;
  engine: EngineState;
  shortcut: string;
  levelRef: { readonly current: number };
  listening: boolean;
}

export function Sidebar({ screen, onNavigate, engine, shortcut, levelRef, listening }: SidebarProps) {
  const { prefs, setMode } = useTheme();
  // The full history is Home's list, opened wider; Home stays the current section.
  const current = screen === "history" ? "home" : screen;

  return (
    <aside className="side">
      <div className="brand">
        <Logo levelRef={levelRef} listening={listening} />
        <span className="wordmark">OpenVoice</span>
      </div>

      <nav className="nav" aria-label="Sections">
        {NAV.map(({ id, label, Icon, key }) => {
          const on = current === id;
          return (
            <button
              key={id}
              type="button"
              className="nav-item"
              aria-current={on ? "page" : undefined}
              aria-keyshortcuts={`Control+${key}`}
              title={label}
              onClick={() => onNavigate(id)}
            >
              {on && <motion.span className="nav-pill" layoutId="nav-pill" transition={{ type: "spring", stiffness: 500, damping: 40 }} aria-hidden="true" />}
              <Icon size="1em" weight="regular" aria-hidden="true" />
              <span className="label">{label}</span>
              <span className="kbd" aria-hidden="true">{key}</span>
            </button>
          );
        })}
      </nav>

      <div className="side-foot">
        <div className="mic glass" title={ENGINE_LABEL[engine]}>
          <div className="row1" role="status">
            <span className={`dot${engine === "error" ? " err" : engine === "starting" ? " idle" : ""}`} aria-hidden="true" />
            <span>{ENGINE_LABEL[engine]}</span>
          </div>
          <div className="row2">Hold <Keycap>{shortcut}</Keycap> to talk</div>
        </div>
        <div className="modes glass" role="group" aria-label="Light or dark">
          {MODES.map(({ id, label, Icon }) => (
            <button
              key={id}
              type="button"
              className={prefs.mode === id ? "on" : undefined}
              aria-label={label}
              aria-pressed={prefs.mode === id}
              title={label}
              onClick={() => setMode(id)}
            >
              <Icon size="1em" weight="regular" aria-hidden="true" />
            </button>
          ))}
        </div>
      </div>
    </aside>
  );
}
