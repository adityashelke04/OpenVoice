/** Settings.
 *
 * Copy is written for someone who has never opened a terminal. There used to be a
 * Models screen beside this one, where the reader had to weigh graphics memory
 * against accuracy against download size before they could dictate well. There is
 * one model now and it ships with the app, so the screen is gone and the choice
 * with it — removing the decision is the feature, not a simplification of it.
 */

import { useEffect, useState } from "react";
import { Badge, Button, Card, MicTestMeter, Notice, Select, Toggle } from "../ui";
import {
  checkForUpdate,
  HOTKEYS,
  installUpdate,
  listMicrophones,
  loadSettings,
  openDataDir,
  restartApp,
  restartReasons,
  saveSettings,
  type Settings as S,
  type UpdateStatus,
} from "../engine/settings";
import "./screens.css";

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
function withCurrent(options: string[], current: string): string[] {
  return options.includes(current) ? options : [current, ...options];
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

/** A labelled row. The only list primitive these screens use. */
function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="srow">
      <div className="srow-label">
        <div className="t-body-strong">{label}</div>
        {hint && <div className="t-caption srow-hint">{hint}</div>}
      </div>
      <div className="srow-control">{children}</div>
    </div>
  );
}

export function useSettings() {
  const [settings, setSettings] = useState<S | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadSettings().then((s) => s && setSettings(s));
  }, []);

  const patch = async (fn: (s: S) => void) => {
    if (!settings) return;
    // Optimistic, then reconciled with whatever the store actually wrote — the
    // Rust side validates and can reject.
    const next = structuredClone(settings);
    fn(next);
    setSettings(next);
    setSaving(true);
    setError(null);
    try {
      const saved = await saveSettings(next);
      if (saved) setSettings(saved);
    } catch (e) {
      setError(String(e));
      setSettings(settings);
    } finally {
      setSaving(false);
    }
  };

  return { settings, patch, saving, error };
}

/** Updates: the one place OpenVoice contacts a server without being asked.
 *
 *  Written to be read by someone deciding whether to trust it, so it says what
 *  the request is and what it carries rather than just offering a switch. The
 *  check is separated from the install on screen for the same reason it is
 *  separated in the Rust: finding out is not the same as agreeing.
 */
function UpdatesCard({
  settings,
  patch,
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void;
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
    <Card title="Updates">
      <div className="srows">
        <Row
          label="Check for updates when OpenVoice starts"
          hint="One request to GitHub for a signed list of releases. It carries no identifier and no usage data — there is nowhere in the code to put one. Turn this off and no request is made at all."
        >
          <Toggle
            on={settings.config.updates.check_on_launch}
            onChange={(v) => patch((s) => (s.config.updates.check_on_launch = v))}
            label="Check for updates on launch"
          />
        </Row>
        <Row
          label="Check now"
          hint={
            result
              ? result.available
                ? `Version ${result.version} is available. You have ${result.currentVersion}.`
                : `You are on the latest version (${result.currentVersion}).`
              : "Updates are verified against a signing key built into this app before anything is installed."
          }
        >
          {result?.available ? (
            <Button variant="primary" onClick={install} disabled={installing}>
              {installing ? "Installing…" : `Install ${result.version}`}
            </Button>
          ) : (
            <Button onClick={check} disabled={checking}>
              {checking ? "Checking…" : "Check now"}
            </Button>
          )}
        </Row>
      </div>
      {error && <Notice tone="danger">{error}</Notice>}
    </Card>
  );
}

export function SettingsScreen({
  settings,
  patch,
  error,
  levelRef,
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void;
  error: string | null;
  levelRef?: { current: number };
}) {
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

  return (
    <div className="screen">
      <header className="screen-head">
        <h1 className="t-title">Settings</h1>
      </header>

      {error && <Notice tone="danger">{error}</Notice>}

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
          effect once OpenVoice restarts. Everything else — your shortcut
          included — is already working.
        </Notice>
      )}

      <Card title="Dictation">
        <div className="srows">
          <Row
            label="Shortcut"
            hint={
              c.activation === "toggle"
                ? "Press this key to start, and press it again to stop. The new key works the moment you pick it."
                : "Hold this key while you speak, then let go. The new key works the moment you pick it."
            }
          >
            <Select
              options={HOTKEYS.map(([, label]) => label)}
              value={HOTKEYS.find(([v]) => v === c.chord.key)?.[1] ?? "Right Ctrl"}
              onChange={(e) =>
                patch((s) => {
                  const found = HOTKEYS.find(([, label]) => label === e.target.value);
                  if (found) s.config.chord.key = found[0];
                })
              }
              style={{ width: 160 }}
            />
          </Row>
          <Row
            label="How it starts"
            hint="Hold to talk keeps the microphone open only while the key is down, so it cannot be left listening by accident. Press to start and stop is easier on your hand for anything long. Applies to your next dictation."
          >
            <Select
              options={["Hold to talk", "Press to start and stop"]}
              value={c.activation === "toggle" ? "Press to start and stop" : "Hold to talk"}
              onChange={(e) =>
                patch((s) => {
                  s.config.activation =
                    e.target.value === "Press to start and stop" ? "toggle" : "push_to_talk";
                })
              }
              style={{ width: 220 }}
            />
          </Row>
          <Row
            label="Microphone"
            hint="Leave on the system default unless the wrong one is being used."
          >
            <Select
              options={["System default", ...mics]}
              value={c.input_device ?? "System default"}
              onChange={(e) =>
                patch((s) => {
                  s.config.input_device =
                    e.target.value === "System default" ? null : e.target.value;
                })
              }
              style={{ width: 220 }}
            />
            <MicTestMeter levelRef={levelRef} />
          </Row>
          <Row
            label="Sound feedback"
            hint="A short tone when you start dictating, and another when it finishes and lands."
          >
            <Toggle
              on={c.sound_enabled}
              onChange={(v) => patch((s) => (s.config.sound_enabled = v))}
              label="Play a sound when dictating starts and finishes"
            />
          </Row>
          <Row
            label="Maximum recording"
            hint="Recording stops on its own after this long, so a stuck key cannot record forever."
          >
            <Select
              options={["1 minute", "2 minutes", "5 minutes"]}
              value={
                c.limits.max_duration_ms <= 60_000
                  ? "1 minute"
                  : c.limits.max_duration_ms <= 120_000
                    ? "2 minutes"
                    : "5 minutes"
              }
              onChange={(e) =>
                patch((s) => {
                  s.config.limits.max_duration_ms =
                    e.target.value === "1 minute"
                      ? 60_000
                      : e.target.value === "2 minutes"
                        ? 120_000
                        : 300_000;
                })
              }
              style={{ width: 150 }}
            />
          </Row>
        </div>
      </Card>

      <UpdatesCard settings={settings} patch={patch} />

      <Card title="Privacy">
        <div className="srows">
          <Row
            label="Keep recordings"
            hint="Off: your voice is held in memory only and discarded the moment it has been written out. Turn this on only to help diagnose a problem — it applies from the next restart, and recordings already saved stay until you delete them."
          >
            <Toggle
              on={c.privacy.retain_audio}
              onChange={(v) => patch((s) => (s.config.privacy.retain_audio = v))}
              label="Keep recordings on disk"
            />
          </Row>
          {c.privacy.retain_audio && (
            <Row
              label="Delete recordings after"
              hint="Recordings are far larger than transcripts — about 2 MB a minute — so they are cleared on this schedule. Your history is separate and is never affected by this."
            >
              <Select
                options={withCurrent(
                  ["1 day", "7 days", "30 days", "Keep them"],
                  audioDaysLabel(c.privacy.audio_days),
                )}
                value={audioDaysLabel(c.privacy.audio_days)}
                onChange={(e) =>
                  patch((s) => {
                    s.config.privacy.audio_days =
                      e.target.value === "Keep them" ? 0 : parseInt(e.target.value, 10);
                  })
                }
                style={{ width: 150 }}
              />
            </Row>
          )}
          <Row
            label="Hide secrets in history"
            hint="API keys and tokens are replaced with [redacted] before a transcript is saved or logged. The text delivered to your app is never altered — only the stored copy. Edit the patterns under privacy.redact_patterns in settings.toml."
          >
            <Badge dot tone={c.privacy.redact_patterns.length > 0 ? "live" : "neutral"}>
              {c.privacy.redact_patterns.length > 0
                ? `${c.privacy.redact_patterns.length} patterns`
                : "Off"}
            </Badge>
          </Row>
          <Row label="Keep history for" hint="Older entries are deleted automatically.">
            <Select
              options={withCurrent(
                ["7 days", "30 days", "90 days", "Forever"],
                historyDaysLabel(c.privacy.history_days),
              )}
              value={historyDaysLabel(c.privacy.history_days)}
              onChange={(e) =>
                patch((s) => {
                  s.config.privacy.history_days =
                    e.target.value === "Forever" ? 0 : parseInt(e.target.value, 10);
                })
              }
              style={{ width: 150 }}
            />
          </Row>
          <Row
            label="Sends nothing anywhere"
            hint="There is no analytics, no crash reporting and no account. This is not a setting because there is nothing to turn off — dictation has no network path at all. The speech model is installed with the app, so nothing is ever fetched."
          >
            <Badge dot tone="live">
              Local only
            </Badge>
          </Row>
          {/* Deliberately has no Download, Delete or "In use" control. There is
              one model, it arrived with the app, and it cannot be changed or
              removed -- so this states a fact rather than offering a choice. No
              accuracy figure appears here on purpose: the measured one came from
              audio that is in-domain for this model, and a number on screen would
              outlive that caveat. */}
          <Row
            label="Speech engine"
            hint="Parakeet TDT 0.6B v2, from NVIDIA. English. It runs entirely on this computer and is installed with the app, so there is nothing to choose and nothing to download."
          >
            <Badge dot tone="live">
              Typically ~0.5 s
            </Badge>
          </Row>
          <Row label="Your data" hint="Transcripts and settings live in a plain folder you can open, copy or delete.">
            <Button onClick={() => openDataDir()}>Open folder</Button>
          </Row>
        </div>
      </Card>
    </div>
  );
}
