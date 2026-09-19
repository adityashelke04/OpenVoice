/** Advanced: the formatter trace, the data folders and the engine facts.
 *
 * Moved out of Profiles.tsx unchanged when Writing style was rewritten on the
 * Hub primitives (plan Task 12), so that file no longer pulls in the legacy
 * `../ui` kit. This screen gets its own redesign in Task 15.
 */

import { useEffect, useState } from "react";
import { Button, Card } from "../ui";
import { previewFormat, type Settings as S } from "../engine/settings";
import "./screens.css";

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
