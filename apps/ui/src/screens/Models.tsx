/** Speech model: which engine the app loads, and what it costs to keep.
 *
 * Copy is written for someone who has never heard of Parakeet: "Multilingual" and
 * "Light" rather than model ids, with the id kept as secondary detail for people
 * who want it. Nothing here quotes an accuracy figure (see MODEL_COPY).
 *
 * Markup and classes follow the reference (docs/redesign/reference/reference.html,
 * Speech model section; styles in hub/screens.css). Every fact on a card comes from
 * the Rust catalogue, so adding a model there is enough to make it appear here.
 */

import { useEffect, useState, type CSSProperties } from "react";
import { DownloadSimple, Feather, GlobeHemisphereWest, Waveform } from "@phosphor-icons/react";
import { Badge, Button, Notice } from "../hub/ui";
import {
  deleteModel,
  downloadModel,
  formatBytes,
  formatSize,
  getDownload,
  listModels,
  MODEL_COPY,
  type ModelSpec,
  restartApp,
  type Settings as S,
} from "../engine/settings";
import "../hub/screens.css";

/** Kept here rather than in MODEL_COPY, and keyed loosely: a model the catalogue
 *  gains before this file knows about it still renders, with the generic glyph. */
const ICONS: Record<string, typeof Waveform> = {
  "parakeet-tdt-0.6b-v2": Waveform,
  "parakeet-tdt-0.6b-v3": GlobeHemisphereWest,
  "whisper-tiny.en": Feather,
};

/** What the Download button says while a transfer is running. */
function downloadLabel(progress: { done: number; total: number } | null): string {
  if (!progress) return "Downloading…";
  if (progress.total > 0) {
    return `${Math.min(100, Math.round((progress.done / progress.total) * 100))}%`;
  }
  // No Content-Length. Counting up real bytes is more honest than a percentage
  // of a total nobody knows.
  return progress.done > 0 ? formatBytes(progress.done) : "Downloading…";
}

export function ModelsScreen({
  settings,
  patch,
}: {
  settings: S;
  patch: (fn: (s: S) => void) => void | Promise<boolean>;
}) {
  const [models, setModels] = useState<ModelSpec[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);

  const refresh = () => {
    listModels().then((m) => m && setModels(m));
  };

  useEffect(refresh, []);

  // The backend owns downloads across screen unmounts. Restore and poll that
  // shared state instead of depending on a click in this particular mount.
  useEffect(() => {
    let live = true;
    let observed: string | null = null;
    const read = () => void getDownload().then(p => {
      if (!live) return;
      if (p) {
        observed = p.model;
        setBusy(p.model);
        setProgress(p);
      } else if (observed) {
        observed = null;
        setBusy(null);
        setProgress(null);
        refresh();
      }
    }, e => { if (live) setError(`Could not check download progress. ${String(e)}`); });
    read();
    const timer = window.setInterval(read, 400);
    return () => { live = false; clearInterval(timer); };
  }, []);

  const download = async (id: string) => {
    setBusy(id);
    setError(null);
    try {
      await downloadModel(id);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
      setProgress(null);
    }
  };

  const remove = async (id: string) => {
    setBusy(id);
    setError(null);
    try {
      await deleteModel(id);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="scroll">
      <p className="lead">
        {/* The reference colours this one word in page ink with an inline style.
            A `.lead strong` rule would reach every other screen's lead instead. */}
        <strong style={{ color: "var(--ink)" }}>Standard</strong> is included and works
        offline from the moment you install. The other two are optional and only
        downloaded if you ask. Everything runs on this machine either way.
      </p>

      {pending && (
        <Notice
          tone="warn"
          action={
            <Button variant="primary" onClick={() => restartApp()}>
              Restart now
            </Button>
          }
        >
          Restart to start using <strong>{pending}</strong>. It takes about ten
          seconds: the weights are already on this computer.
        </Notice>
      )}

      <div className="models">
        {(models ?? []).map((m, i) => {
          // A model with no copy still renders: its id becomes the label and its
          // real size is shown. That is what keeps adding one a single change to
          // the Rust catalogue.
          const copy = MODEL_COPY[m.id];
          const name = copy?.name ?? m.id;
          const detail = copy?.detail ?? (m.englishOnly ? "English only." : "Multilingual.");
          const active = settings.model === m.id;
          const ready = m.installed;
          const Icon = ICONS[m.id] ?? Waveform;

          return (
            <article
              key={m.id}
              className={active ? "model glass active" : "model glass"}
              data-active={active}
              style={{ "--i": i } as CSSProperties}
            >
              <span className="ic"><Icon weight="bold" aria-hidden /></span>

              {/* The card is a container, not one big button: it carries its own
                  Download and Delete controls, and an interactive element cannot
                  legally live inside another one. Selecting is the name/detail
                  column alone. */}
              <button
                className="model-select"
                aria-pressed={active}
                // Selecting a model you do not have would leave the app running
                // something other than what the screen says is chosen.
                disabled={!ready}
                onClick={async () => {
                  if (active || !ready) return;
                  if (await patch((s) => (s.model = m.id)) !== false) setPending(name);
                }}
              >
                <div className="nm">
                  {name}
                  {/* Saved selection can differ from the running engine until restart. */}
                  {active && ready && <Badge tone="ok">Selected</Badge>}
                  {active && !ready && <Badge>Selected, not downloaded</Badge>}
                  {m.bundled && <Badge>Included</Badge>}
                  {!m.bundled && ready && !active && <Badge>On this computer</Badge>}
                  {m.englishOnly ? <Badge>English only</Badge> : <Badge>25 languages</Badge>}
                </div>
                <div className="dt">{detail}</div>
                <div className="id">{m.id}</div>
                {/* NVIDIA model card: https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3 (verified 2026-10-22). */}
                {m.id === "parakeet-tdt-0.6b-v3" && <div className="dt">Supported languages: Bulgarian, Croatian, Czech, Danish, Dutch, English, Estonian, Finnish, French, German, Greek, Hungarian, Italian, Latvian, Lithuanian, Maltese, Polish, Portuguese, Romanian, Slovak, Slovenian, Spanish, Swedish, Russian, Ukrainian. Hindi is not supported.</div>}
              </button>

              <div className="nums">
                <div>
                  <div className="k">{ready ? "On disk" : "Download"}</div>
                  <div className="v">{formatSize(ready ? m.diskMb : m.downloadMb)}</div>
                </div>
                {copy?.speed && (
                  <div>
                    <div className="k">Typical</div>
                    <div className="v">{copy.speed}</div>
                  </div>
                )}
              </div>

              {/* Always present, even empty: it is the card's fourth grid column,
                  and the rows above and below must line up regardless. */}
              <div className="act">
                {!ready && (
                  <Button
                    size="sm"
                    icon={<DownloadSimple aria-hidden />}
                    onClick={() => download(m.id)}
                    disabled={busy !== null}
                  >
                    {busy === m.id ? downloadLabel(progress) : "Download"}
                  </Button>
                )}
                {ready && !m.bundled && !active && (
                  <Button size="sm" onClick={() => remove(m.id)} disabled={busy !== null}>
                    Delete
                  </Button>
                )}
              </div>
            </article>
          );
        })}
        {models === null && <p className="cap">Reading the model list…</p>}
      </div>

      {/* The Hub's Notice has one tone. A failed fetch or delete is recoverable
          (the button is still there), so amber says as much as red would. */}
      {error && <Notice tone="warn">{error}</Notice>}

      <p className="cap">
        Timings are measured on one particular laptop and are there for comparison,
        not as a promise.
      </p>
    </section>
  );
}
