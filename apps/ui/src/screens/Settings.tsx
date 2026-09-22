/** Settings: the shortcut, the microphone, how the app looks, and what it keeps.
 *
 * Copy is written for someone who has never opened a terminal. Each row carries
 * the reference's short hint, and the longer explanation it replaced stays as the
 * row's `title`: the screen has to be skimmable, but the reasoning behind a
 * privacy setting is worth keeping somewhere a curious reader can reach.
 *
 * Markup and classes follow the reference (docs/redesign/reference/reference.html,
 * Settings section; styles in hub/screens.css). Appearance is new here: themes
 * and the light/dark choice used to be reachable only from the sidebar corner and
 * Ctrl+K, which is not where anybody looks for them.
 */

import { useEffect, useState, type CSSProperties } from "react";
import { ArrowsClockwise, Desktop, Microphone, Moon, Palette, ShieldCheck, Sun } from "@phosphor-icons/react";
import { Badge, Button, Card, Keycap, MicMeter, Notice, Segmented, SelectField, SettingRow, Slider, Switch } from "../hub/ui";
import { ThemeCard } from "../hub/ThemeCard";
import { THEMES, useTheme, type ModeChoice, type ThemeName } from "../hub/theme";
import {
  APP_VERSION,
  checkForUpdate,
  DEFAULT_REDACT_PATTERNS,
  HOTKEYS,
  installUpdate,
  listMicrophones,
  openDataDir,
  restartApp,
  restartReasons,
  type Settings as S,
  type UpdateStatus,
} from "../engine/settings";
import "../hub/screens.css";

/** Join reasons the way a person would say them: "a, b and c".
 *
 *  The Rust side returns them as separate phrases rather than a pre-joined string
 *  so this stays the only place the app decides what a list sounds like. */
function sentence(parts: readonly string[]): string {
  if (parts.length <= 1) return parts[0] ?? "";
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

/** Options for a preset picker, guaranteeing the current value is among them.
 *
 * `settings.toml` is a plain file people edit. A value we do not offer — 14 days
 * of audio retention, say — leaves a `<select>` with a value matching no option,
 * and a browser then displays the *first* option instead. So the screen calmly
 * reported "1 day" for a config that said 14, and the only way to find out was
 * to reopen the file. Showing the real value keeps the screen honest, and the
 * user can still pick a preset over it.
 */
function withCurrent(options: string[], current: string): { value: string; label: string }[] {
  return (options.includes(current) ? options : [current, ...options]).map((o) => ({ value: o, label: o }));
}

/** How a recordings-retention value reads. Zero means "keep them". */
function audioDaysLabel(days: number): string {
  if (days === 0) return "Keep them";
  return days === 1 ? "1 day" : `${days} days`;
}

/** How a history-retention value reads. Zero means "forever". */
function historyDaysLabel(days: number): string {
  if (days === 0) return "Forever";
  return days === 1 ? "1 day" : `${days} days`;
}

/** Stops for the recording limit, on a track that runs from 30 s to 5 min. The
 *  track ends are only geometry; these three are the values you can land on. */
const MAX_STOPS = [60_000, 120_000, 300_000];
const minutes = (ms: number) => `${Math.round(ms / 60_000)} min`;

const THEME_LABELS: Record<ThemeName, string> = {
  glacier: "Glacier",
  graphite: "Graphite",
  lagoon: "Lagoon",
};

const MODES: { value: ModeChoice; label: string; icon: React.ReactNode }[] = [
  { value: "light", label: "Light", icon: <Sun aria-hidden /> },
  { value: "system", label: "System", icon: <Desktop aria-hidden /> },
  { value: "dark", label: "Dark", icon: <Moon aria-hidden /> },
];

// Moved to hub/useSettings.ts (the Hub shell owns settings now); re-exported
// so the Flow Bar and older imports keep working.
export { useSettings } from "../hub/useSettings";

/** Updates: the one place OpenVoice contacts a server without being asked.
 *
 *  The check is separated from the install on screen for the same reason it is
 *  separated in the Rust: finding out is not the same as agreeing. Nothing is
 *  said under "Check now" until a check has actually run, so the card at rest
 *  makes no claim about a version it has not looked up.
 */
function UpdatesCard({
  settings,
  patch,
  index,
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void;
  index: number;
}) {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<UpdateStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);

  const check = async () => {
    setChecking(true);
    setError(null);
    setResult(null);
    try {
      setResult(await checkForUpdate());
    } catch (e) {
      setError(String(e));
    } finally {
      setChecking(false);
    }
  };

  const install = async () => {
    setInstalling(true);
    setError(null);
    try {
      await installUpdate();
    } catch (e) {
      setError(String(e));
      setInstalling(false);
    }
  };

  return (
    <Card title="Updates" icon={<ArrowsClockwise weight="bold" aria-hidden />} style={{ "--i": index } as CSSProperties}>
      <SettingRow
        label="Check on launch"
        title="One request to GitHub for a signed list of releases. It carries no identifier and no usage data; there is nowhere in the code to put one. Turn this off and no request is made at all."
      >
        <Switch
          checked={settings.config.updates.check_on_launch}
          onChange={(v) => patch((s) => (s.config.updates.check_on_launch = v))}
          label="Check for updates on launch"
        />
      </SettingRow>
      <SettingRow
        label="Check now"
        hint={
          result
            ? result.available
              ? `Version ${result.version} is available. You have ${result.currentVersion}.`
              : `You are on the latest version (${result.currentVersion}).`
            : undefined
        }
        title="Updates are verified against a signing key built into this app before anything is installed."
      >
        {result?.available ? (
          <Button size="sm" variant="primary" onClick={install} disabled={installing}>
            {installing ? "Installing…" : `Install ${result.version}`}
          </Button>
        ) : (
          <Button size="sm" onClick={check} disabled={checking}>
            {checking ? "Checking…" : "Check now"}
          </Button>
        )}
      </SettingRow>
      <SettingRow label="Version">
        <span className="kv">{APP_VERSION}</span>
      </SettingRow>
      {error && <Notice tone="warn">{error}</Notice>}
    </Card>
  );
}

export function SettingsScreen({
  settings,
  patch,
  error,
  levelRef,
  listening,
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void;
  error: string | null;
  levelRef?: { readonly current: number };
  listening?: boolean;
}) {
  const { prefs, mode, setTheme, setMode, setSolid } = useTheme();

  const [mics, setMics] = useState<string[]>([]);
  useEffect(() => {
    listMicrophones().then((m) => m && setMics(m));
  }, []);

  // What is saved but cannot reach the running engine. Re-asked after every save,
  // because that is the only moment it can change -- and asked of the Rust side
  // rather than worked out here, so a setting that reloads in place never shows up
  // as a reason to restart.
  const [pending, setPending] = useState<string[]>([]);
  useEffect(() => {
    restartReasons().then((r) => r && setPending(r));
  }, [settings]);

  const c = settings.config;
  const toggle = c.activation === "toggle";

  return (
    <section className="scroll">
      {error && <Notice tone="warn">{error}</Notice>}

      {pending.length > 0 && (
        <Notice
          tone="warn"
          action={
            <Button variant="primary" onClick={() => restartApp()}>
              Restart now
            </Button>
          }
        >
          {pending.length === 1 ? "Your change to " : "Your changes to "}
          <strong>{sentence(pending)}</strong>
          {pending.length === 1
            ? " is saved, but it only takes"
            : " are saved, but they only take"}{" "}
          effect once OpenVoice restarts. Everything else, your shortcut included,
          is already working.
        </Notice>
      )}

      <div className="set-grid">
        <div className="set-col">
          <Card title="Dictation" icon={<Microphone weight="bold" aria-hidden />} style={{ "--i": 0 } as CSSProperties}>
            <SettingRow
              label="Shortcut"
              hint="Hold it anywhere to talk."
              title={
                toggle
                  ? "Press this key to start, and press it again to stop. The new key works the moment you pick it."
                  : "Hold this key while you speak, then let go. The new key works the moment you pick it."
              }
            >
              <SelectField
                label="Shortcut"
                width={150}
                value={c.chord.key}
                display={<Keycap>{HOTKEYS.find(([v]) => v === c.chord.key)?.[1] ?? "Right Ctrl"}</Keycap>}
                options={HOTKEYS.map(([value, label]) => ({ value, label }))}
                onChange={(v) => patch((s) => (s.config.chord.key = v))}
              />
            </SettingRow>

            <SettingRow
              label="How it starts"
              hint="Hold to talk cannot be left listening by accident."
              title="Hold to talk keeps the microphone open only while the key is down, so it cannot be left listening by accident. Press to start and stop is easier on your hand for anything long. Applies to your next dictation."
            >
              <Segmented<"push_to_talk" | "toggle">
                label="How it starts"
                value={toggle ? "toggle" : "push_to_talk"}
                options={[
                  { value: "push_to_talk", label: "Hold" },
                  { value: "toggle", label: "Press to toggle" },
                ]}
                onChange={(v) => patch((s) => (s.config.activation = v))}
              />
            </SettingRow>

            <SettingRow
              label="Microphone"
              hint="System default unless the wrong one is used."
              title="Leave on the system default unless the wrong one is being used."
            >
              {/* The meter sits beside the picker, not under it: it is there to
                  confirm the device you just chose is the one hearing you. */}
              <div className="mic-pick">
                <MicMeter levelRef={levelRef} listening={listening} />
                <SelectField
                  label="Microphone"
                  width={150}
                  value={c.input_device ?? "System default"}
                  options={["System default", ...mics].map((m) => ({ value: m, label: m }))}
                  onChange={(v) => patch((s) => (s.config.input_device = v === "System default" ? null : v))}
                />
              </div>
            </SettingRow>

            <SettingRow
              label="Sound feedback"
              hint="A short tone when you start and when it lands."
              title="A short tone when you start dictating, and another when it finishes and lands."
            >
              <Switch
                checked={c.sound_enabled}
                onChange={(v) => patch((s) => (s.config.sound_enabled = v))}
                label="Play a sound when dictating starts and finishes"
              />
            </SettingRow>

            <SettingRow
              label="Maximum recording"
              hint="Stops on its own, so a stuck key cannot record forever."
              title="Recording stops on its own after this long, so a stuck key cannot record forever."
            >
              <Slider
                label="Maximum recording"
                value={c.limits.max_duration_ms}
                stops={MAX_STOPS}
                min={30_000}
                max={300_000}
                format={minutes}
                onChange={(v) => patch((s) => (s.config.limits.max_duration_ms = v))}
              />
            </SettingRow>
          </Card>

          <Card title="Privacy" icon={<ShieldCheck weight="bold" aria-hidden />} style={{ "--i": 2 } as CSSProperties}>
            <SettingRow
              label="Keep recordings"
              hint="Off: your voice is discarded once it is written out."
              title="Off: your voice is held in memory only and discarded the moment it has been written out. Turn this on only to help diagnose a problem; it applies from the next restart, and recordings already saved stay until you delete them."
            >
              <Switch
                checked={c.privacy.retain_audio}
                onChange={(v) => patch((s) => (s.config.privacy.retain_audio = v))}
                label="Keep recordings on disk"
              />
            </SettingRow>

            {c.privacy.retain_audio && (
              <SettingRow
                label="Delete recordings after"
                hint="About 2 MB a minute, so they are cleared on a schedule."
                title="Recordings are far larger than transcripts, about 2 MB a minute, so they are cleared on this schedule. Your history is separate and is never affected by this."
              >
                <SelectField
                  label="Delete recordings after"
                  width={150}
                  value={audioDaysLabel(c.privacy.audio_days)}
                  options={withCurrent(["1 day", "7 days", "30 days", "Keep them"], audioDaysLabel(c.privacy.audio_days))}
                  onChange={(v) => patch((s) => (s.config.privacy.audio_days = v === "Keep them" ? 0 : parseInt(v, 10)))}
                />
              </SettingRow>
            )}

            <SettingRow
              label="Hide secrets in history"
              hint="API keys and tokens are stored as [redacted]."
              title="API keys and tokens are replaced with [redacted] before a transcript is saved or logged. The text delivered to your app is never altered, only the stored copy. The patterns live under privacy.redact_patterns in settings.toml."
            >
              {/* On and off are the shipped patterns and none. Someone who wrote
                  their own patterns keeps them until they touch this switch. */}
              <Switch
                checked={c.privacy.redact_patterns.length > 0}
                onChange={(v) =>
                  patch((s) => {
                    s.config.privacy.redact_patterns = v ? [...DEFAULT_REDACT_PATTERNS] : [];
                  })
                }
                label="Hide secrets in history"
              />
            </SettingRow>

            <SettingRow
              label="Keep history for"
              hint="Older entries are deleted automatically."
              title="Older entries are deleted automatically. Your recordings are separate and have their own schedule."
            >
              <SelectField
                label="Keep history for"
                width={150}
                value={historyDaysLabel(c.privacy.history_days)}
                options={withCurrent(["7 days", "30 days", "90 days", "Forever"], historyDaysLabel(c.privacy.history_days))}
                onChange={(v) => patch((s) => (s.config.privacy.history_days = v === "Forever" ? 0 : parseInt(v, 10)))}
              />
            </SettingRow>

            <SettingRow
              label="Sends nothing anywhere"
              hint="No analytics, no crash reports, no account."
              title="There is no analytics, no crash reporting and no account. This is not a setting because there is nothing to turn off. OpenVoice uses the network for exactly two things, both of which you start: a speech model you choose to download, and the update check you can switch off."
            >
              <Badge tone="ok">Local only</Badge>
            </SettingRow>

            <SettingRow
              label="Your data"
              hint="A plain folder you can open, copy or delete."
              title="Transcripts and settings live in a plain folder you can open, copy or delete."
            >
              <Button size="sm" onClick={() => openDataDir()}>Open folder</Button>
            </SettingRow>
          </Card>
        </div>

        <div className="set-col">
          <Card title="Appearance" icon={<Palette weight="bold" aria-hidden />} style={{ "--i": 1 } as CSSProperties}>
            {/* Each card is its own theme inside, so the name would follow that
                theme's ink. It has to read against this page instead. */}
            <div className="themes" style={{ "--page-ink": "var(--ink)" } as CSSProperties}>
              {THEMES.map((t) => (
                <ThemeCard
                  key={t}
                  theme={t}
                  label={THEME_LABELS[t]}
                  mode={mode}
                  selected={prefs.theme === t}
                  onSelect={() => setTheme(t)}
                />
              ))}
            </div>

            <SettingRow label="Light or dark" hint="System follows Windows.">
              {/* The stored choice, not the resolved mode: someone on System
                  should see System, whichever one Windows is handing them. */}
              <Segmented<ModeChoice>
                label="Light or dark"
                value={prefs.mode}
                options={MODES}
                onChange={setMode}
              />
            </SettingRow>

            <SettingRow
              label="Reduce transparency"
              hint="Solid panels instead of glass. On when Windows transparency is off."
              title="Solid panels instead of glass. It is already on whenever Windows transparency effects are off, and this switch turns it on regardless."
            >
              <Switch checked={prefs.solid} onChange={setSolid} label="Reduce transparency" />
            </SettingRow>
          </Card>

          <UpdatesCard settings={settings} patch={patch} index={3} />
        </div>
      </div>
    </section>
  );
}
