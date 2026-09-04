/** The Speech model screen.
 *
 * Its own file, unlike the version deleted in 0.5.0, which lived inside a
 * 700-line Settings.tsx holding two unrelated screens — which is precisely why
 * removing one of them was fiddly enough to get wrong.
 *
 * Copy is written for someone who has never heard of Parakeet: "Multilingual"
 * and "Light" rather than model ids, with the id shown as secondary detail for
 * people who want it. Nothing here quotes an accuracy figure — see MODEL_COPY.
 */

import { useEffect, useState } from "react";
import { Badge, Button, Notice } from "../ui";
import {
  deleteModel,
  downloadModel,
  formatBytes,
  formatSize,
  getDownload,
  listModels,
  MODEL_COPY,
  type ModelSpec,
  modelsOnDisk,
  restartApp,
  type Settings as S,
} from "../engine/settings";
import "./screens.css";

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
  patch: (fn: (s: S) => void) => void;
}) {
  const [models, setModels] = useState<ModelSpec[] | null>(null);
  const [onDisk, setOnDisk] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);

  const refresh = () => {
    listModels().then((m) => m && setModels(m));
    modelsOnDisk().then((n) => n !== null && setOnDisk(n));
  };

  useEffect(refresh, []);

  // Poll while a transfer is in flight. A 465 MB download can take minutes on a
  // slow link, and a button that says only "Downloading…" for that long is
  // indistinguishable from one that has hung — which is the state in which
  // people kill the app and leave a part-fetched model behind.
  useEffect(() => {
    if (!busy) return;
    const id = setInterval(() => {
      getDownload().then((p) => setProgress(p && p.model === busy ? p : null));
    }, 400);
    return () => clearInterval(id);
  }, [busy]);

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
    <div className="screen">
      <header className="screen-head">
        <h1 className="t-title">Speech model</h1>
        <p className="t-body screen-lead">
          <strong>Standard</strong> is included and works offline from the moment you
          install. The other two are optional and only downloaded if you ask.
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
          Restart to start using <strong>{pending}</strong>. It takes about ten
          seconds — the weights are already on this computer.
        </Notice>
      )}

      <div className="model-list">
        {(models ?? []).map((m) => {
          // A model with no copy still renders: its id becomes the label and its
          // real size is shown. That is what keeps adding one a single change to
          // the Rust catalogue.
          const copy = MODEL_COPY[m.id];
          const name = copy?.name ?? m.id;
          const detail = copy?.detail ?? (m.englishOnly ? "English only." : "Multilingual.");
          const active = settings.model === m.id;
          const ready = m.installed;

          return (
            // A container rather than one big button: the row needs its own
            // Download and Delete controls, and an interactive element cannot
            // legally live inside another one.
            <div key={m.id} className="model" data-active={active}>
              <button
                className="model-select"
                aria-pressed={active}
                // Selecting a model you do not have would leave the app running
                // something other than what the screen says is chosen.
                disabled={!ready}
                onClick={() => {
                  if (active || !ready) return;
                  patch((s) => (s.model = m.id));
                  setPending(name);
                }}
              >
                <div className="model-main">
                  <div className="hstack">
                    <span className="t-subheading">{name}</span>
                    {/* "In use" only when it is both chosen *and* present.
                        Claiming otherwise beside a Download button was simply
                        contradictory. */}
                    {active && ready && (
                      <Badge dot tone="live">
                        In use
                      </Badge>
                    )}
                    {active && !ready && <Badge>Selected — not downloaded</Badge>}
                    {m.bundled && <Badge>Included</Badge>}
                    {!m.bundled && ready && !active && <Badge>On this computer</Badge>}
                    {m.englishOnly ? <Badge>English only</Badge> : <Badge>25 languages</Badge>}
                  </div>
                  <div className="t-caption model-detail">{detail}</div>
                  <div className="t-mono model-id">{m.id}</div>
                </div>
                <div className="model-numbers">
                  <div>
                    <div className="t-label">{ready ? "On disk" : "Download"}</div>
                    <div className="t-mono">
                      {formatSize(ready ? m.diskMb : m.downloadMb)}
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

              {!ready && (
                <Button size="sm" onClick={() => download(m.id)} disabled={busy !== null}>
                  {busy === m.id ? downloadLabel(progress) : "Download"}
                </Button>
              )}
              {ready && !m.bundled && !active && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => remove(m.id)}
                  disabled={busy !== null}
                >
                  Delete
                </Button>
              )}
            </div>
          );
        })}
        {models === null && <p className="t-caption">Reading the model list…</p>}
      </div>

      {error && <Notice tone="danger">{error}</Notice>}

      <p className="t-caption screen-foot">
        Downloaded models are using {formatBytes(onDisk)} on this computer. Deleting
        one frees that space and you can fetch it again later; <strong>Standard</strong>
        {" "}came with the app and stays. Timings are measured on one particular laptop
        and are there for comparison, not as a promise.
      </p>
    </div>
  );
}
