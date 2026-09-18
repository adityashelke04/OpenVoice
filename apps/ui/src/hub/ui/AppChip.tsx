import { AppWindow, Browser, ChatCircle, Code, EnvelopeSimple, FolderSimple, Note, TerminalWindow, type Icon } from "@phosphor-icons/react";
import { appDisplay, type AppIcon } from "../apps";

const ICONS: Record<AppIcon, Icon> = { Code, TerminalWindow, ChatCircle, Note, Browser, EnvelopeSimple, AppWindow, FolderSimple };

/** Which app a dictation went to. Name and icon come from the exe (spec 6.11); the
 *  colour always comes from the row's profile, never the exe. */
export function AppChip({ exe, profile }: { exe: string; profile: string }) {
  const { name, icon, chip } = appDisplay(exe, profile);
  const I = ICONS[icon];
  return (
    <span className={`app-chip c-${chip}`}>
      <I weight="bold" aria-hidden />
      {name}
    </span>
  );
}
