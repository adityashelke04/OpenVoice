/** Dictionary: teach OpenVoice the words it keeps getting wrong.
 *
 * This is the product's core promise made touchable. The live preview matters more
 * than the list: you type what you heard it write, add a correction, and watch the
 * result change without dictating anything. A dictionary you cannot test is a
 * dictionary nobody trusts.
 *
 * Markup and classes follow the reference (docs/redesign/reference/reference.html,
 * Dictionary section; styles in hub/screens.css). The try-out's result box marks
 * what the formatter changed, the same word diff Home's Fix panel uses, so the
 * mechanism ("you said this, it wrote that") is visible rather than described.
 */

import { useCallback, useEffect, useId, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { ArrowRight, Flask, ListChecks, MagnifyingGlass, Microphone, Plus, TextT, X } from "@phosphor-icons/react";
import { Button, Card, Field } from "../hub/ui";
import { markChanges } from "../hub/diff";
import { addDictionaryTerm, previewFormat, type Settings as S } from "../engine/settings";
import "../hub/screens.css";

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
  const heardRef = useRef<HTMLInputElement>(null);
  const writtenRef = useRef<HTMLInputElement>(null);
  const trialId = useId();

  const terms = settings.dictionary;
  const q = query.trim().toLowerCase();
  const shown = q
    ? terms.filter((t) => t.written.toLowerCase().includes(q) || t.spoken.some((s) => s.toLowerCase().includes(q)))
    : terms;

  // Re-run the preview whenever the phrase or the dictionary changes, so adding a
  // term visibly updates the result.
  //
  // Each run carries a sequence number and a slower earlier request is discarded
  // when it lands. Without that, typing quickly enough to have two previews in
  // flight let the first one to *finish* win rather than the last one sent, and
  // the box would settle showing the result for a phrase you had already edited.
  const seq = useRef(0);
  const run = useCallback(async (text: string) => {
    const mine = ++seq.current;
    if (!text.trim()) {
      setResult("");
      return;
    }
    const trace = await previewFormat(text, "prose");
    if (mine !== seq.current) return; // superseded while this was in flight
    if (trace?.length) setResult(trace[trace.length - 1][1]);
  }, []);

  useEffect(() => {
    run(trial);
  }, [trial, terms, run]);

  // Owner decision 5: the full stop the prose style adds at the end is not
  // marked, as in the reference; every other added word or mark is.
  const segs = useMemo(
    () => (result ? markChanges(trial, result, { ignoreCase: true, ignoreFinalPeriod: true }) : []),
    [trial, result],
  );

  const add = () => {
    // Add is never disabled (a half-transparent primary reads as broken); an
    // incomplete pair sends focus to the half that is missing instead.
    if (!heard.trim()) return heardRef.current?.focus();
    if (!written.trim()) return writtenRef.current?.focus();
    patch((s) => { addDictionaryTerm(s, heard, written); });
    setHeard("");
    setWritten("");
    heardRef.current?.focus();
  };
  const onEnter = (e: KeyboardEvent) => {
    if (e.key === "Enter") { e.preventDefault(); add(); }
  };

  const remove = (w: string) => patch((s) => {
    s.dictionary = s.dictionary.filter((t) => t.written !== w);
  });

  return (
    <section className="scroll dict">
      <p className="lead">Names, jargon and technical words often come out wrong. Tell OpenVoice what you meant once, and it will get it right from then on.</p>

      <Card title="Try a phrase" icon={<Flask weight="bold" aria-hidden />} right="Uses the Messages style" style={{ "--i": 0 } as CSSProperties}>
        <div className="tryrow">
          <div className="trybox">
            <label className="lbl" htmlFor={trialId}><Microphone aria-hidden />What OpenVoice heard</label>
            <textarea
              id={trialId}
              className="txt"
              rows={2}
              spellCheck={false}
              placeholder="Type what OpenVoice wrote, for example: call use effect here"
              value={trial}
              onChange={(e) => setTrial(e.target.value)}
            />
          </div>
          <div className="trybox out">
            <div className="lbl"><TextT aria-hidden />What lands at your cursor</div>
            <div className="txt" aria-live="polite">
              {result
                ? segs.map((s, i) => (s.changed ? <mark key={i}>{s.text}</mark> : s.text))
                : <span className="txt-empty">The corrected version appears here.</span>}
            </div>
          </div>
        </div>
      </Card>

      <Card
        title="Your corrections"
        icon={<ListChecks weight="bold" aria-hidden />}
        meta={terms.length}
        className="corrections"
        style={{ "--i": 1 } as CSSProperties}
        actions={
          <Field
            className="term-search"
            icon={<MagnifyingGlass aria-hidden />}
            placeholder="Search"
            aria-label="Search corrections"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        }
      >
        {shown.length === 0 ? (
          <div className="terms-empty">
            <div className="terms-empty-title">{q ? `Nothing matches "${query.trim()}"` : "No corrections yet"}</div>
            {!q && <p className="cap">When OpenVoice mishears a word, add it here. A name, a piece of jargon, a product: anything it writes wrong more than once.</p>}
          </div>
        ) : (
          <div className="terms">
            {shown.map((t) => (
              <div className="term" key={t.written}>
                <span className="sp" title={t.spoken.join(" · ")}>{t.spoken.join(" · ")}</span>
                <ArrowRight className="ar" aria-hidden />
                <span className="wr">{t.written}</span>
                <button type="button" className="x" aria-label={`Remove ${t.written}`} onClick={() => remove(t.written)}>
                  <X aria-hidden />
                </button>
              </div>
            ))}
          </div>
        )}

        <div className="addrow">
          <Field ref={heardRef} placeholder="You said, e.g. use effect" aria-label="You said" value={heard} onChange={(e) => setHeard(e.target.value)} onKeyDown={onEnter} />
          <Field ref={writtenRef} placeholder="Write it as, e.g. useEffect" aria-label="Write it as" value={written} onChange={(e) => setWritten(e.target.value)} onKeyDown={onEnter} />
          <Button variant="primary" icon={<Plus weight="bold" aria-hidden />} onClick={add}>Add</Button>
        </div>
      </Card>

      <p className="cap">OpenVoice also knows 30 built-in terms like useEffect and kubectl. Yours always win. Changes apply to your next dictation.</p>
    </section>
  );
}
