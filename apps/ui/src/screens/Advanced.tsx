/** Advanced: the formatter trace, the data folders and the engine facts.
 *
 * Nothing here is part of everyday use. It exists for the ten seconds after a
 * transcript comes out wrong: the pipeline shows every stage the formatter ran
 * and marks what each one changed, which turns "the dictation is broken" into
 * "the commands rule did it".
 *
 * Markup and classes follow the reference (docs/redesign/reference/reference.html,
 * Advanced section; styles in hub/screens.css). The trace is live rather than a
 * picture: the sentence is editable and every keystroke re-runs `preview_format`
 * against the real engine, so the rail below always describes this machine.
 */

import { useEffect, useState, type CSSProperties } from "react";
import { Cpu, Folder, GitDiff } from "@phosphor-icons/react";
import { Button, Card, Field, SettingRow } from "../hub/ui";
import { markChanges } from "../hub/diff";
import { getLogPath, openDataDir, previewFormat, type Settings as S } from "../engine/settings";
import "../hub/screens.css";

/** The phrase the README quotes and `ov-format` asserts on, so the rail below
 *  starts out showing every stage doing something. */
const SAMPLE = "um so we need to call use effect here comma then return null";

/** `C:\Users\you\AppData\Roaming\...` written as `%APPDATA%\...`.
 *
 *  The row is there to be read and pasted into Explorer, and both work better
 *  short: the full path is wide enough to need a second line, and it carries the
 *  account name into every screenshot of this screen. Any other shape is left
 *  exactly as the engine reported it. */
function shortPath(path: string): string {
  return path.replace(/^[A-Za-z]:\\Users\\[^\\]+\\AppData\\Roaming\\/i, "%APPDATA%\\");
}

/** Advanced: one card for the pipeline, then the folders and the facts. */
export function AdvancedScreen({ settings }: { settings: S }) {
  const [text, setText] = useState(SAMPLE);
  const [trace, setTrace] = useState<[string, string][]>([]);
  const [logPath, setLogPath] = useState("");

  // `settings` is a dependency as well as `text`: a dictionary entry or a rule
  // changed on another screen changes what this trace says.
  useEffect(() => {
    previewFormat(text, "editor").then((t) => t && setTrace(t));
  }, [text, settings]);

  useEffect(() => {
    getLogPath().then((p) => p && setLogPath(p));
  }, []);

  return (
    <section className="scroll">
      <p className="lead">
        Nothing here is needed for everyday use. It exists so that when something
        looks wrong, you can see exactly why.
      </p>

      <Card
        title="How a sentence is rewritten"
        icon={<GitDiff weight="bold" aria-hidden />}
        right="Code editors style"
        style={{ "--i": 0 } as CSSProperties}
      >
        {/* Editable, because the useful question is always about the sentence
            that just came out wrong, not about this one. */}
        <Field
          mono
          aria-label="A sentence to trace"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />

        <div className="pipe">
          {trace.map(([stage, out], i) => {
            const previous = i > 0 ? trace[i - 1][1] : null;
            const changed = previous !== null && previous !== out;
            return (
              <div className={changed ? "step ch" : "step"} key={stage}>
                <span className="n" />
                <span className="sn">{stage}</span>
                <span className="so">
                  {/* Only what this stage added is marked. Deletions have no
                      place to sit, and the row above still shows them. */}
                  {changed
                    ? markChanges(previous, out).map((s, k) => (s.changed ? <mark key={k}>{s.text}</mark> : s.text))
                    : out}
                </span>
                {/* Always the fourth cell, tagged or not: the rows are one grid
                    and have to line up whether or not anything happened. */}
                {changed ? <span className="tg">changed</span> : <span />}
              </div>
            );
          })}
        </div>
      </Card>

      <div className="set-grid">
        <Card title="Files" icon={<Folder weight="bold" aria-hidden />} style={{ "--i": 1 } as CSSProperties}>
          <SettingRow
            label="Log"
            hint={<span style={{ fontFamily: "var(--mono)" }}>{shortPath(logPath)}</span>}
            title="Everything the app recorded about this session. Worth attaching to a bug report."
          >
            <Button size="sm" onClick={() => openDataDir()}>Open folder</Button>
          </SettingRow>

          <SettingRow
            label="Settings and history"
            hint="Plain files you can copy or delete."
            title="Plain text files. Copy them to another machine, or delete them to start over."
          >
            <Button size="sm" onClick={() => openDataDir()}>Open folder</Button>
          </SettingRow>
        </Card>

        <Card title="Engine" icon={<Cpu weight="bold" aria-hidden />} style={{ "--i": 2 } as CSSProperties}>
          <SettingRow
            label="Speech model"
            title="Chosen on the Speech model screen. It runs on this computer, and changing it needs a restart."
          >
            <span className="kv">{settings.model}</span>
          </SettingRow>

          <SettingRow
            label="Paste instead of type above"
            title="Longer text is pasted rather than typed, which is instant but briefly borrows your clipboard."
          >
            <span className="kv">{settings.config.paste_threshold_chars} characters</span>
          </SettingRow>

          <SettingRow label="Corrections" title="How many entries your dictionary holds.">
            <span className="kv">{settings.dictionary.length}</span>
          </SettingRow>
        </Card>
      </div>
    </section>
  );
}
