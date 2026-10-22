import { useEffect, useState } from "react";
import { discardRecovery, getRecovery, retryAudio, type RecoveryAudio } from "../../engine/settings";
import { Button } from "../ui";

export function Recovery({ sessions, notice }: { sessions: number; notice: unknown }) {
  const [audio, setAudio] = useState<RecoveryAudio | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let live = true;
    const read = () => void getRecovery().then(a => { if (live) setAudio(a); }, e => { if (live) setError(`Could not check retained audio. ${String(e)}`); });
    read();
    const timer = window.setInterval(read, 5000);
    return () => { live = false; clearInterval(timer); };
  }, [sessions, notice]);
  const act = async (discard: boolean) => {
    if (!audio || busy) return;
    setBusy(true);
    setError("");
    try {
      if (discard) await discardRecovery(); else await retryAudio(audio.session);
      setAudio(null);
    } catch (e) { setError(`Could not ${discard ? "discard audio" : "retry transcription"}. ${String(e)}`); }
    finally { setBusy(false); }
  };
  if (!audio) return error ? <p role="alert">{error}</p> : null;
  return <article className="card glass" aria-label="Recover dictation">
    <h2>Recover dictation</h2>
    <p>{audio.partial ? "The captured portion" : "The audio"} ({Math.ceil(audio.durationMs / 1000)} seconds) is temporarily available for retry. It expires after 10 minutes. Recovered text is kept for manual copy and paste.</p>
    <Button disabled={busy} onClick={() => void act(false)}>Retry transcription</Button>
    <Button disabled={busy} onClick={() => void act(true)}>Discard audio</Button>
    {error && <p role="alert">{error}</p>}
  </article>;
}
