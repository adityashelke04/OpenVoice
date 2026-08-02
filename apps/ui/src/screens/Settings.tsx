/** Settings and Models.
 *
 * Copy is written for someone who has never opened a terminal — "Accurate" and
 * "Light" rather than `large-v3-turbo` and `small.en`, with the model id shown as
 * secondary detail for people who want it. The audience is anyone who types.
 */

import { useEffect, useState } from "react";
import { Badge, Button, Card, Kbd, Notice, Select, Toggle } from "../ui";
import {
  listMicrophones,
  loadSettings,
  MODELS,
  openDataDir,
  restartApp,
  saveSettings,
  type Settings as S,
} from "../engine/settings";
import "./screens.css";

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
          <Row label="Shortcut" hint="Hold this key while you speak, then let go.">
            <Kbd>Right Ctrl</Kbd>
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

      <Card title="Privacy">
        <div className="srows">
          <Row
            label="Keep recordings"
            hint="Off: your voice is held in memory only and discarded the moment it has been written out. Turn this on only to help diagnose a problem."
          >
            <Toggle
              on={c.privacy.retain_audio}
              onChange={(v) => patch((s) => (s.config.privacy.retain_audio = v))}
              label="Keep recordings on disk"
            />
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
        {MODELS.map((m) => {
          const active = settings.model === m.id;
          return (
            <button
              key={m.id}
              className="model"
              data-active={active}
              aria-pressed={active}
              onClick={() => {
                if (active) return;
                patch((s) => (s.model = m.id));
                setPending(m.name);
              }}
            >
              <div className="model-main">
                <div className="hstack">
                  <span className="t-subheading">{m.name}</span>
                  {active && (
                    <Badge dot tone="live">
                      In use
                    </Badge>
                  )}
                </div>
                <div className="t-caption model-detail">{m.detail}</div>
                <div className="t-mono model-id">{m.id}</div>
              </div>
              <div className="model-numbers">
                <div>
                  <div className="t-label">Download</div>
                  <div className="t-mono">{m.size}</div>
                </div>
                <div>
                  <div className="t-label">Typical</div>
                  <div className="t-mono">{m.speed}</div>
                </div>
              </div>
            </button>
          );
        })}
      </div>

      <p className="t-caption screen-foot">
        Measured on this computer: an RTX 3050 with 4 GB. If a game or video call is
        using the graphics card, choose <strong>Light</strong> — the accurate model
        needs the card mostly to itself.
      </p>
    </div>
  );
}
