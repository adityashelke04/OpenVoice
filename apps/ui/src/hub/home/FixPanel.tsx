/** Teach the dictionary from a dictation that came out wrong.
 *
 * The whole point is the first line: it shows what the model actually *heard*,
 * which is information the user has never had access to. Knowing that "kubectl"
 * arrived as "cube control" is most of the work; once you can see the
 * mishearing, correcting it is obvious. Guessing at it from the formatted output
 * is not.
 *
 * The heard words are buttons because the spoken form is almost always a couple
 * of adjacent words lifted straight out of that line, and retyping them by hand
 * to teach a tool that a word was misheard is exactly the kind of friction that
 * stops people bothering.
 *
 * Logic ported from the old Hub's FixRow; only the styling (and an em dash in
 * the caption) changed.
 */
import { useId, useState } from "react";
import { Button, Field } from "../ui";
import { addDictionaryTerm, type Settings } from "../../engine/settings";
import type { Row } from "../../engine/stats";

export function FixPanel({ row, patch, onDone }: {
  row: Row;
  patch: (fn: (s: Settings) => void) => void;
  onDone: () => void;
}) {
  const id = useId();
  const [heard, setHeard] = useState("");
  const [written, setWritten] = useState("");

  const words = row.raw_text.split(/\s+/).filter(Boolean);
  const canSave = heard.trim().length > 0 && written.trim().length > 0;

  const save = () => {
    if (!canSave) return;
    patch((s) => { addDictionaryTerm(s, heard, written); });
    onDone();
  };

  return (
    <div className="fix">
      <div className="cap">OpenVoice heard this. Click the words it got wrong.</div>
      <div className="chips">
        {words.map((w, i) => (
          <button key={`${w}-${i}`} type="button" className="exe" onClick={() => setHeard((h) => (h ? `${h} ${w}` : w))}>
            {w}
          </button>
        ))}
      </div>
      <div className="fix-form">
        <div className="fix-field">
          <span className="cap" id={`${id}-said`}>You said</span>
          <Field aria-labelledby={`${id}-said`} mono placeholder="cube control" value={heard} onChange={(e) => setHeard(e.target.value)} />
        </div>
        <div className="fix-field">
          <span className="cap" id={`${id}-write`}>Write it as</span>
          <Field
            aria-labelledby={`${id}-write`}
            mono
            placeholder="kubectl"
            value={written}
            onChange={(e) => setWritten(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") save(); }}
          />
        </div>
        <Button variant="primary" size="sm" onClick={save} disabled={!canSave}>Save</Button>
        <Button size="sm" onClick={onDone}>Cancel</Button>
      </div>
      <p className="cap">Applies to the next thing you dictate. Nothing already written changes.</p>
    </div>
  );
}
