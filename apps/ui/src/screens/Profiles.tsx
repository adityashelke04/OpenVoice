/** Writing style: per-application formatting rules.
 *
 * Called "Writing style" rather than "Profiles" because the audience is anyone who
 * types. The concept is simple once framed as behaviour: the same sentence should
 * come out differently in a chat window than in a terminal, and this is where that
 * is decided.
 *
 * Markup and classes follow the reference (docs/redesign/reference/reference.html,
 * Writing style section; styles in hub/screens.css). Every rule shows its effect in
 * "What that does" beside it: settings whose effect you cannot see are settings
 * nobody touches. The rules bind to the stored profile exactly as the old screen
 * did; only the presentation changed.
 *
 * The Advanced screen that used to share this file lives in Advanced.tsx until its
 * own redesign, so nothing here reaches for the legacy `../ui` kit.
 */

import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { AppWindow, ChatCircle, Code, Eye, Sliders, TerminalWindow } from "@phosphor-icons/react";
import { Card, Segmented, SettingRow, Switch } from "../hub/ui";
import { previewFormat, type FillerLevel, type Profile, type Settings as S } from "../engine/settings";
import "../hub/screens.css";

type Style = {
  /** The stored profile this tab edits. */
  name: string;
  title: string;
  icon: ReactNode;
  /** The Rules card's caption: the old "where" sentence, shortened as in the reference. */
  where: string;
  /** What the preview says you said. Each one exercises the rules its style exists for. */
  sample: string;
};

const icon = (Icon: typeof ChatCircle, chip: string) => (
  <Icon weight="bold" aria-hidden style={{ color: `var(--chip-${chip})` }} />
);

/** The tabs, in the order people think of them, not the order `ov-format`
 *  happens to store the profiles in (which puts the fallback first). */
const STYLES: Style[] = [
  {
    name: "prose",
    title: "Messages and documents",
    icon: icon(ChatCircle, "prose"),
    where: "Chat, email, notes, browsers",
    sample: "um so basically the deploy is done and you know we should ship it",
  },
  {
    name: "editor",
    title: "Code editors",
    icon: icon(Code, "editor"),
    where: "VS Code, Cursor and other editors",
    sample: "call use effect here comma then return null",
  },
  {
    name: "terminal",
    title: "Terminals",
    icon: icon(TerminalWindow, "terminal"),
    where: "Command prompts",
    sample: "cube control get pods dash dash all dash namespaces",
  },
  {
    // The fallback reuses the Messages sentence: side by side, the difference
    // (light filler removal, no full stop) is the point.
    name: "default",
    title: "Everything else",
    icon: icon(AppWindow, "default"),
    where: "Anything not listed elsewhere",
    sample: "um so basically the deploy is done and you know we should ship it",
  },
];

/** The rows' full original hints. The screen shows the reference's short hints;
 *  the long ones, with the caveats that did not fit, stay as each row's tooltip. */
const LONG = {
  fillers: "“Light” removes um and uh. “Aggressive” also removes like, you know and basically — useful, but those words are occasionally what you meant.",
  punctuation: "Say “comma”, “new line” or “open bracket” and get the symbol. Say “literally comma” to get the word.",
  naming: "Say “camel case user name” and get userName. Mostly useful when writing code.",
};

const FILLERS: { value: FillerLevel; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "light", label: "Light" },
  { value: "aggressive", label: "Aggressive" },
];

export function ProfilesScreen({
  settings,
  patch,
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void;
}) {
  // Only the styles whose profile exists get a tab; Messages opens first when present.
  const styles = STYLES.filter((st) => settings.profiles.some((p) => p.name === st.name));
  const [open, setOpen] = useState(styles[0]?.name ?? "prose");
  const style = styles.find((st) => st.name === open) ?? styles[0];
  const profile = settings.profiles.find((p) => p.name === style?.name);

  // The preview re-runs when the style or any rule changes. A slower, older
  // request is dropped when it lands (the Dictionary try-out's guard), so
  // switching tabs quickly never leaves one style's output under another's sample.
  const [preview, setPreview] = useState("");
  const seq = useRef(0);
  const sample = style?.sample ?? "";
  const name = profile?.name;
  useEffect(() => {
    if (!name) return;
    const mine = ++seq.current;
    setPreview("");
    previewFormat(sample, name).then((t) => {
      if (mine === seq.current && t?.length) setPreview(t[t.length - 1][1]);
    });
  }, [settings, name, sample]);

  if (!style || !profile) return null;

  const edit = (fn: (p: Profile) => void) =>
    patch((s) => {
      const target = s.profiles.find((p) => p.name === profile.name);
      if (target) fn(target);
    });

  return (
    <section className="scroll writing">
      <p className="lead">The same words should look different depending on where they land. A chat message wants a capital letter and a full stop; a terminal command must have neither.</p>

      <Segmented
        size="lg"
        label="Writing style"
        value={style.name}
        onChange={setOpen}
        options={styles.map((st) => ({ value: st.name, label: st.title, icon: st.icon }))}
      />

      <div className="style-grid">
        <Card title="Rules" icon={<Sliders weight="bold" aria-hidden />} right={style.where} style={{ "--i": 0 } as CSSProperties}>
          <SettingRow label="Capitalise sentences" hint="Start each sentence with a capital letter." title="Start each sentence with a capital letter.">
            <Switch
              label="Capitalise sentences"
              checked={profile.capitalize === "sentence"}
              onChange={(v) => edit((p) => { p.capitalize = v ? "sentence" : "force_lower"; })}
            />
          </SettingRow>
          <SettingRow label="End with a full stop" hint="Add one if you did not say it." title="Add one if you did not say it.">
            <Switch
              label="End with a full stop"
              checked={profile.end_period}
              onChange={(v) => edit((p) => { p.end_period = v; })}
            />
          </SettingRow>
          <SettingRow label="Remove filler words" hint="Light removes um and uh. Aggressive also removes like, you know and basically." title={LONG.fillers}>
            <Segmented
              label="Remove filler words"
              value={profile.fillers}
              onChange={(v) => edit((p) => { p.fillers = v; })}
              options={FILLERS}
            />
          </SettingRow>
          <SettingRow label="Spoken punctuation" hint={'Say "comma" or "new line" and get the symbol.'} title={LONG.punctuation}>
            <Switch
              label="Spoken punctuation"
              checked={profile.voice_commands}
              onChange={(v) => edit((p) => { p.voice_commands = v; })}
            />
          </SettingRow>
          <SettingRow label="Spoken naming styles" hint={'Say "camel case user name" and get userName.'} title={LONG.naming}>
            <Switch
              label="Spoken naming styles"
              checked={profile.case_transforms}
              onChange={(v) => edit((p) => { p.case_transforms = v; })}
            />
          </SettingRow>
        </Card>

        <div className="style-side">
          <Card title="What that does" icon={<Eye weight="bold" aria-hidden />} style={{ "--i": 1 } as CSSProperties}>
            <div className="trybox">
              <div className="lbl">You say</div>
              <div className="txt">{style.sample}</div>
            </div>
            <div className="trybox out">
              <div className="lbl">OpenVoice writes</div>
              {/* A no-break space holds the line while the preview is on its way. */}
              <div className="txt" aria-live="polite">{preview || " "}</div>
            </div>
          </Card>

          <Card title="Used in these apps" icon={<AppWindow weight="bold" aria-hidden />} style={{ "--i": 2 } as CSSProperties}>
            {profile.matches.length === 0 ? (
              <p className="cap">Anything not matched by another style uses this one.</p>
            ) : (
              <div className="chips">
                {profile.matches.map((m) => <span className="exe" key={m}>{m}</span>)}
              </div>
            )}
          </Card>
        </div>
      </div>
    </section>
  );
}
