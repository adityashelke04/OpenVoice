/** Settings and Models.
 *
 * Copy is written for someone who has never opened a terminal — "Accurate" and
 * "Light" rather than `large-v3-turbo` and `small.en`, with the model id shown as
 * secondary detail for people who want it. The audience is anyone who types.
 */

import { useEffect, useState } from "react";
import { Badge, Button, Card, Notice, Select, Toggle } from "../ui";
import {
  checkForUpdate,
  deleteModel,
  downloadModel,
  formatBytes,
  formatSize,
  HOTKEYS,
  installUpdate,
  listMicrophones,
  listModels,
  loadSettings,
  MODEL_COPY,
  type ModelSpec,
  openDataDir,
  restartApp,
  saveSettings,
  type Settings as S,
  type UpdateStatus,
} from "../engine/settings";
import "./screens.css";

/** Whisper's most commonly dictated languages, curated rather than exhaustive —
 *  faster-whisper actually accepts ~99 ISO 639-1 codes, but a 99-item dropdown
 *  is not a feature, it's a search problem. "Auto-detect" maps to `null`
 *  (`Config.language: None`), which trades a little accuracy for not asking
 *  the user to pick anything at all. */
const LANGUAGES: readonly [code: string, name: string][] = [
  ["en", "English"],
  ["es", "Spanish"],
  ["fr", "French"],
  ["de", "German"],
  ["it", "Italian"],
  ["pt", "Portuguese"],
  ["nl", "Dutch"],
  ["ru", "Russian"],
  ["zh", "Chinese"],
  ["ja", "Japanese"],
  ["ko", "Korean"],
  ["hi", "Hindi"],
  ["ar", "Arabic"],
  ["tr", "Turkish"],
  ["pl", "Polish"],
  ["sv", "Swedish"],
];

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
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void;
  error: string | null;
}) {
  const [mics, setMics] = useState<string[]>([]);
  useEffect(() => {
    listMicrophones().then((m) => m && setMics(m));
  }, []);

  const c = settings.config;

  return (
    <div className="screen">
      <header className="screen-head">
        <h1 className="t-title">Settings</h1>
      </header>

      {error && <Notice tone="danger">{error}</Notice>}

      <Card title="Dictation">
        <div className="srows">
          <Row
            label="Shortcut"
            hint={
              c.activation === "toggle"
                ? "Press this key to start, and press it again to stop. Takes effect after a restart."
                : "Hold this key while you speak, then let go. Takes effect after a restart."
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
            hint="Hold to talk keeps the microphone open only while the key is down, so it cannot be left listening by accident. Press to start and stop is easier on your hand for anything long. Takes effect after a restart."
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
              style={{ width: 260 }}
            />
          </Row>
          <Row
            label="Language"
            hint="Telling OpenVoice the language beats auto-detect for a single short utterance — that's the whole reason this isn't 'Auto-detect' by default. Change it if you dictate in something other than English."
          >
            <Select
              options={["Auto-detect", ...LANGUAGES.map(([, name]) => name)]}
              value={LANGUAGES.find(([code]) => code === c.language)?.[1] ?? "Auto-detect"}
              onChange={(e) =>
                patch((s) => {
                  const found = LANGUAGES.find(([, name]) => name === e.target.value);
                  s.config.language = found ? found[0] : null;
                })
              }
              style={{ width: 200 }}
            />
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
                options={["1 day", "7 days", "30 days", "Keep them"]}
                value={
                  c.privacy.audio_days === 0
                    ? "Keep them"
                    : c.privacy.audio_days === 1
                      ? "1 day"
                      : `${c.privacy.audio_days} days`
                }
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
              options={["7 days", "30 days", "90 days", "Forever"]}
              value={
                c.privacy.history_days === 0
                  ? "Forever"
                  : `${c.privacy.history_days} days`
              }
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
            hint="There is no analytics, no crash reporting and no account. This is not a setting because there is nothing to turn off — the code has no way to reach the internet except to download a speech model you ask for."
          >
            <Badge dot tone="live">
              Local only
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

export function ModelsScreen({
  settings,
  patch,
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void;
}) {
  const [pending, setPending] = useState<string | null>(null);
  const [models, setModels] = useState<ModelSpec[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => {
    listModels().then((m) => m && setModels(m));
  };
  useEffect(refresh, []);

  const [busy, setBusy] = useState<string | null>(null);

  const remove = async (id: string) => {
    setError(null);
    try {
      await deleteModel(id);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const download = async (id: string) => {
    setError(null);
    setBusy(id);
    try {
      await downloadModel(id);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const onDisk = (models ?? []).reduce((n, m) => n + m.installedBytes, 0);

  return (
    <div className="screen">
      <header className="screen-head">
        <h1 className="t-title">Speech model</h1>
        <p className="t-body screen-lead">
          Bigger models understand more, but need more of your graphics card.
          Everything runs on this machine either way.
        </p>
      </header>

      {pending && (
        <Notice
          tone="warn"
          action={
            <Button variant="primary" onClick={() => restartApp()}>
              Restart now
            </Button>
          }
        >
          Restart to start using <strong>{pending}</strong>. If it's already on
          this machine, that takes about ten seconds. If not, you'll see it
          download first — OpenVoice never fetches a model you haven't chosen.
        </Notice>
      )}

      <div className="model-list">
        {(models ?? []).map((m) => {
          const active = settings.model === m.id;
          // A model with no copy still renders: its id is the label and its real
          // size is shown. That is what keeps adding one a single-file change.
          const copy = MODEL_COPY[m.id];
          const name = copy?.name ?? m.id;
          const detail =
            copy?.detail ??
            (m.vramMb > 0
              ? `Needs about ${formatSize(m.vramMb)} of graphics memory.`
              : "Works without a graphics card.");
          return (
            // A container rather than one big button: the row needs its own
            // Delete control, and an interactive element cannot legally live
            // inside another one.
            <div key={m.id} className="model" data-active={active}>
              <button
                className="model-select"
                aria-pressed={active}
                onClick={() => {
                  if (active) return;
                  patch((s) => (s.model = m.id));
                  setPending(name);
                }}
              >
                <div className="model-main">
                <div className="hstack">
                  <span className="t-subheading">{name}</span>
                  {active && (
                    <Badge dot tone="live">
                      In use
                    </Badge>
                  )}
                  {m.englishOnly && <Badge>English only</Badge>}
                  {m.installed && !active && <Badge>On this computer</Badge>}
                </div>
                <div className="t-caption model-detail">{detail}</div>
                <div className="t-mono model-id">{m.id}</div>
              </div>
              <div className="model-numbers">
                <div>
                  <div className="t-label">{m.installed ? "On disk" : "Download"}</div>
                  <div className="t-mono">
                    {m.installed ? formatBytes(m.installedBytes) : formatSize(m.sizeMb)}
                  </div>
                </div>
                {copy?.speed && (
                  <div>
                    <div className="t-label">Typical</div>
                    <div className="t-mono">{copy.speed}</div>
                  </div>
                )}
                </div>
              </button>
              {!m.installed && (
                <Button
                  size="sm"
                  onClick={() => download(m.id)}
                  disabled={busy !== null}
                >
                  {busy === m.id ? "Downloading…" : "Download"}
                </Button>
              )}
              {m.installed && !m.inUse && (
                <Button size="sm" variant="ghost" onClick={() => remove(m.id)}>
                  Delete
                </Button>
              )}
            </div>
          );
        })}
        {models === null && (
          <p className="t-caption">Reading the model list…</p>
        )}
      </div>

      {error && <Notice tone="danger">{error}</Notice>}

      <p className="t-caption screen-foot">
        Speech models are using {formatBytes(onDisk)} on this computer. Deleting one
        frees that space; you can download it again later. Measured timings are from
        an RTX 3050 with 4 GB — if a game or video call is using the graphics card,
        choose <strong>Light</strong>, because the accurate model needs the card
        mostly to itself.
      </p>
    </div>
  );
}
