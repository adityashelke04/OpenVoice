/** Dictionary — teach OpenVoice the words it keeps getting wrong.
 *
 * This is the product's core promise made touchable. The live preview matters more
 * than the list: you type what you heard it write, add a correction, and watch the
 * result change without dictating anything. A dictionary you cannot test is a
 * dictionary nobody trusts.
 */

import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Card, Empty, Input, Notice } from "../ui";
import { previewFormat, type Settings as S } from "../engine/settings";
import "./screens.css";

export function DictionaryScreen({
  settings,
  patch,
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void;
}) {
  const [heard, setHeard] = useState("");
  const [written, setWritten] = useState("");
  const [query, setQuery] = useState("");
  const [trial, setTrial] = useState("");
  const [result, setResult] = useState("");

  const terms = settings.dictionary;
  const shown = terms.filter(
    (t) =>
      !query ||
      t.written.toLowerCase().includes(query.toLowerCase()) ||
      t.spoken.some((s) => s.toLowerCase().includes(query.toLowerCase())),
  );

  // Re-run the preview whenever the phrase or the dictionary changes, so adding a
  // term visibly updates the result.
  const run = useCallback(async (text: string) => {
    if (!text.trim()) {
      setResult("");
      return;
    }
    const trace = await previewFormat(text, "prose");
    if (trace?.length) setResult(trace[trace.length - 1][1]);
  }, []);

  useEffect(() => {
    run(trial);
  }, [trial, terms, run]);

  const add = () => {
    const w = written.trim();
    const h = heard.trim().toLowerCase();
    if (!w || !h) return;
    patch((s) => {
      const existing = s.dictionary.find((t) => t.written === w);
      if (existing) {
        if (!existing.spoken.includes(h)) existing.spoken.push(h);
      } else {
        s.dictionary.unshift({ written: w, spoken: [h], group: "code" });
      }
    });
    setHeard("");
    setWritten("");
  };

  const remove = (w: string) => patch((s) => {
    s.dictionary = s.dictionary.filter((t) => t.written !== w);
  });

  return (
    <div className="screen">
      <header className="screen-head">
        <h1 className="t-title">Dictionary</h1>
        <p className="t-body screen-lead">
          Names, jargon and technical words often come out wrong. Tell OpenVoice what
          you meant once, and it will get it right from then on.
        </p>
      </header>

      <Card title="Try a phrase">
        <div className="tryout">
          <Input
            placeholder="Type what OpenVoice wrote, for example: call use effect here"
            value={trial}
            onChange={(e) => setTrial(e.target.value)}
          />
          <div className="tryout-result" data-empty={!result}>
            {result || "The corrected version appears here."}
          </div>
          <p className="t-caption">
            This is exactly what would land at your cursor. Add a correction below and
            watch it change.
          </p>
        </div>
      </Card>

      <Card
        title="Your corrections"
        action={
          <Input
            placeholder="Search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            style={{ width: 200 }}
          />
        }
      >
        <div style={{ margin: "0 -16px -16px" }}>
          {terms.length > 0 && (
            <div className="dict-head">
              <span className="t-label">You said</span>
              <span className="t-label">Write it as</span>
              <span />
            </div>
          )}

          {shown.length === 0 ? (
            <Empty
              title={query ? `Nothing matches “${query}”` : "No corrections yet"}
              hint={
                query
                  ? undefined
                  : "When OpenVoice mishears a word, add it here. A name, a piece of jargon, a product — anything it writes wrong more than once."
              }
            />
          ) : (
            shown.map((t) => (
              <div className="dict-row" key={t.written}>
                <span className="dict-spoken">{t.spoken.join(" · ")}</span>
                <span className="dict-written">{t.written}</span>
                <Button size="sm" variant="ghost" onClick={() => remove(t.written)}>
                  Remove
                </Button>
              </div>
            ))
          )}

          <div className="dict-add">
            <Input
              label="You said"
              placeholder="use effect"
              value={heard}
              onChange={(e) => setHeard(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && add()}
            />
            <Input
              label="Write it as"
              placeholder="useEffect"
              value={written}
              onChange={(e) => setWritten(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && add()}
            />
            <Button variant="primary" onClick={add} disabled={!heard.trim() || !written.trim()}>
              Add
            </Button>
          </div>
        </div>
      </Card>

      <Notice>
        <span>
          OpenVoice already knows about {" "}
          <Badge>30 built-in terms</Badge>{" "}
          like useEffect and kubectl. Yours always win over those.
        </span>
      </Notice>

      <p className="t-caption screen-foot">
        Changes apply to the next thing you dictate — no restart needed.
      </p>
    </div>
  );
}
