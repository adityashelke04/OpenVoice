/** Writing style — per-application formatting rules.
 *
 * Called "Writing style" rather than "Profiles" because the audience is anyone who
 * types. The concept is simple once framed as behaviour: the same sentence should
 * come out differently in a chat window than in a terminal, and this is where that
 * is decided.
 *
 * Every toggle shows its effect live in the preview beneath. Settings whose effect
 * you cannot see are settings nobody touches.
 */

import { useEffect, useState } from "react";
import { Button, Card, Select, Toggle } from "../ui";
import { previewFormat, type Settings as S } from "../engine/settings";
import "./screens.css";

/** Human framing for each built-in profile. */
const MEANING: Record<string, { title: string; where: string; sample: string }> = {
  prose: {
    title: "Messages and documents",
    where: "Chat, email, notes, browsers — anywhere you write in sentences.",
    sample: "um so basically the deploy is done and you know we should ship it",
  },
  editor: {
    title: "Code editors",
    where: "VS Code, Cursor and other editors.",
    sample: "call use effect here comma then return null",
  },
  terminal: {
    title: "Terminals",
    where: "Command prompts, where a stray capital letter breaks the command.",
    sample: "cube control get pods dash dash all dash namespaces",
  },
};

export function ProfilesScreen({
  settings,
  patch,
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void;
}) {
  const [open, setOpen] = useState(settings.profiles[0]?.name ?? "prose");
  const profile = settings.profiles.find((p) => p.name === open) ?? settings.profiles[0];
  const meaning = MEANING[profile?.name] ?? {
    title: profile?.name ?? "",
    where: "",
    sample: "hello there",
  };

  const [preview, setPreview] = useState("");
  useEffect(() => {
    if (!profile) return;
    previewFormat(meaning.sample, profile.name).then((t) => {
      if (t?.length) setPreview(t[t.length - 1][1]);
    });
  }, [settings, profile, meaning.sample]);

  if (!profile) return null;

  const edit = (fn: (p: NonNullable<typeof profile>) => void) =>
    patch((s) => {
      const target = s.profiles.find((p) => p.name === profile.name);
      if (target) fn(target);
    });

  return (
    <div className="screen">
      <header className="screen-head">
        <h1 className="t-title">Writing style</h1>
        <p className="t-body screen-lead">
          The same words should look different depending on where they land. A chat
          message wants a capital letter and a full stop; a terminal command must have
          neither. OpenVoice picks the right style from whichever app you are in.
        </p>
      </header>

      <div className="style-tabs">
        {settings.profiles.map((p) => (
          <button
            key={p.name}
            className="tab"
            aria-selected={p.name === open}
            onClick={() => setOpen(p.name)}
          >
            {MEANING[p.name]?.title ?? p.name}
          </button>
        ))}
      </div>

      <Card title={meaning.title}>
        <p className="t-caption" style={{ marginBottom: 16 }}>
          {meaning.where}
        </p>

        <div className="srows">
          <Row
            label="Capitalise sentences"
            hint="Start each sentence with a capital letter."
          >
            <Toggle
              on={profile.capitalize === "sentence"}
              onChange={(v) => edit((p) => (p.capitalize = v ? "sentence" : "force_lower"))}
              label="Capitalise sentences"
            />
          </Row>
          <Row label="End with a full stop" hint="Add one if you did not say it.">
            <Toggle
              on={profile.end_period}
              onChange={(v) => edit((p) => (p.end_period = v))}
              label="End with a full stop"
            />
          </Row>
          <Row
            label="Remove filler words"
            hint="“Light” removes um and uh. “Aggressive” also removes like, you know and basically — useful, but those words are occasionally what you meant."
          >
            <Select
              options={["Off", "Light", "Aggressive"]}
              value={
                profile.fillers === "off"
                  ? "Off"
                  : profile.fillers === "aggressive"
                    ? "Aggressive"
                    : "Light"
              }
              onChange={(e) =>
                edit((p) => (p.fillers = e.target.value.toLowerCase() as typeof p.fillers))
              }
              style={{ width: 140 }}
            />
          </Row>
          <Row
            label="Spoken punctuation"
            hint="Say “comma”, “new line” or “open bracket” and get the symbol. Say “literally comma” to get the word."
          >
            <Toggle
              on={profile.voice_commands}
              onChange={(v) => edit((p) => (p.voice_commands = v))}
              label="Spoken punctuation"
            />
          </Row>
          <Row
            label="Spoken naming styles"
            hint="Say “camel case user name” and get userName. Mostly useful when writing code."
          >
            <Toggle
              on={profile.case_transforms}
              onChange={(v) => edit((p) => (p.case_transforms = v))}
              label="Spoken naming styles"
            />
          </Row>
        </div>
      </Card>

      <Card title="What that does">
        <div className="tryout">
          <div className="t-label">You say</div>
          <div className="tryout-result" style={{ color: "var(--mute)" }}>
            {meaning.sample}
          </div>
          <div className="t-label">OpenVoice writes</div>
          <div className="tryout-result" data-empty={!preview}>
            {preview || "—"}
          </div>
        </div>
      </Card>

      <Card title="Which apps use this">
        <div className="app-list">
          {profile.matches.length === 0 ? (
            <p className="t-caption">
              Everything that is not matched by another style falls back to this one.
            </p>
          ) : (
            profile.matches.map((m) => (
              <span className="app-chip" key={m}>
                {m}
              </span>
            ))
          )}
        </div>
      </Card>
    </div>
  );
}

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

/** Advanced — the surfaces a curious or stuck user needs, kept off the first screen. */
export function AdvancedScreen({ settings }: { settings: S }) {
  const [text, setText] = useState("um so we need to call use effect here comma then return null");
  const [trace, setTrace] = useState<[string, string][]>([]);
  const [logPath, setLogPath] = useState("");

  useEffect(() => {
    previewFormat(text, "editor").then((t) => t && setTrace(t));
  }, [text, settings]);

  useEffect(() => {
    import("../engine/settings").then(({ getLogPath }) =>
      getLogPath().then((p) => p && setLogPath(p)),
    );
  }, []);

  return (
    <div className="screen">
      <header className="screen-head">
        <h1 className="t-title">Advanced</h1>
        <p className="t-body screen-lead">
          Nothing here is needed for everyday use. It exists so that when something
          looks wrong, you can see exactly why.
        </p>
      </header>

      <Card title="How a sentence is rewritten">
        <input
          className="input"
          value={text}
          onChange={(e) => setText(e.target.value)}
          style={{ width: "100%", marginBottom: 12 }}
        />
        <div className="trace">
          {trace.map(([stage, out], i) => {
            const changed = i > 0 && trace[i - 1][1] !== out;
            return (
              <div className="trace-row" key={stage} data-changed={changed}>
                <span className="t-label trace-stage">{stage}</span>
                <span className={changed ? "trace-out changed" : "trace-out"}>{out}</span>
              </div>
            );
          })}
        </div>
        <p className="t-caption" style={{ marginTop: 12, maxWidth: "68ch" }}>
          Each row is one rule. Highlighted rows changed the text. When a transcript
          comes out wrong, this shows which rule did it — which turns a vague
          complaint into a ten-second diagnosis.
        </p>
      </Card>

      <Card title="Files">
        <div className="srows">
          <Row label="Log" hint={logPath || "…"}>
            <Button onClick={() => import("../engine/settings").then((m) => m.openDataDir())}>
              Open folder
            </Button>
          </Row>
          <Row
            label="Settings and history"
            hint="Plain text files. Copy them to another machine, or delete them to start over."
          >
            <Button onClick={() => import("../engine/settings").then((m) => m.openDataDir())}>
              Open folder
            </Button>
          </Row>
        </div>
      </Card>

      <Card title="Engine">
        <div className="srows">
          {/* Reads the engine's own id rather than settings.model. Those two
              drifted apart when the model stopped being selectable, and this row
              spent that time confidently naming a model the app was not running,
              under a hint pointing at a screen that no longer exists. */}
          <Row label="Speech model" hint="Included with OpenVoice. English, and runs on this computer.">
            <span className="t-mono">parakeet-tdt-0.6b-v2</span>
          </Row>
          <Row
            label="Switch to typing above"
            hint="Longer text is pasted instead of typed, which is instant but briefly borrows your clipboard."
          >
            <span className="t-mono">{settings.config.paste_threshold_chars} characters</span>
          </Row>
          <Row label="Corrections" hint="Your dictionary entries.">
            <span className="t-mono">{settings.dictionary.length}</span>
          </Row>
        </div>
      </Card>
    </div>
  );
}
