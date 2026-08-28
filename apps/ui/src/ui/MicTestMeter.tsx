/**
 * MicTestMeter — Live audio input VU meter.
 *
 * Tracks audio levels with zero React re-renders by reading `levelRef`
 * directly inside an animation frame loop and writing segment states straight
 * to the DOM.
 *
 * Includes built-in interactive test capability using Web Audio API so the user
 * can verify their microphone in real time from the Settings screen.
 */

import { useEffect, useRef, useState } from "react";
import "./mic-test-meter.css";

export interface MicTestMeterProps {
  /** Live level ref (e.g. from `useLiveEngine`). */
  levelRef?: { current: number };
  /** Number of LED segments in the meter track. Defaults to 12. */
  bars?: number;
  /** Whether to show the "Test" button toggle. Defaults to true. */
  showTestButton?: boolean;
  /** Optional className for custom layout. */
  className?: string;
}

export function MicTestMeter({
  levelRef,
  bars = 12,
  showTestButton = true,
  className = "",
}: MicTestMeterProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const trackRef = useRef<HTMLDivElement>(null);
  const localLevelRef = useRef<number>(0);
  const [isTesting, setIsTesting] = useState(false);

  // AudioContext ref for local test mode
  const audioContextRef = useRef<AudioContext | null>(null);
  const mediaStreamRef = useRef<MediaStream | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);

  // Toggle local mic test mode using Web Audio API
  const toggleTest = async () => {
    if (isTesting) {
      // Stop testing
      stopLocalAudio();
      setIsTesting(false);
      localLevelRef.current = 0;
    } else {
      // Start testing
      try {
        if (!navigator.mediaDevices?.getUserMedia) {
          console.warn("getUserMedia is not supported in this environment");
          return;
        }
        const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        mediaStreamRef.current = stream;

        const AudioCtx = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
        const ctx = new AudioCtx();
        audioContextRef.current = ctx;

        const source = ctx.createMediaStreamSource(stream);
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 256;
        source.connect(analyser);
        analyserRef.current = analyser;

        setIsTesting(true);
      } catch (err) {
        console.warn("Unable to access microphone for testing:", err);
        stopLocalAudio();
        setIsTesting(false);
      }
    }
  };

  const stopLocalAudio = () => {
    if (mediaStreamRef.current) {
      mediaStreamRef.current.getTracks().forEach((t) => t.stop());
      mediaStreamRef.current = null;
    }
    if (audioContextRef.current) {
      audioContextRef.current.close().catch(() => {});
      audioContextRef.current = null;
    }
    analyserRef.current = null;
  };

  // Clean up Web Audio resources on unmount
  useEffect(() => {
    return () => {
      stopLocalAudio();
    };
  }, []);

  // Zero-render animation loop updating DOM nodes directly
  useEffect(() => {
    const rootEl = rootRef.current;
    const trackEl = trackRef.current;
    if (!trackEl) return;

    const segmentNodes = Array.from(trackEl.children) as HTMLElement[];
    const numSegments = segmentNodes.length;

    let rafId = 0;
    let meter = 0;
    let peak = 0;
    let peakAt = 0;
    let lastFrame = 0;

    const ATTACK_MS = 45;
    const RELEASE_MS = 300;
    const HOLD_MS = 700;

    const dataArray = new Uint8Array(128);

    const tick = (now: number) => {
      rafId = requestAnimationFrame(tick);

      const dt = lastFrame === 0 ? 16.7 : Math.min(64, now - lastFrame);
      lastFrame = now;

      // Sample either from local test analyser or external levelRef
      let raw = 0;
      if (analyserRef.current) {
        analyserRef.current.getByteTimeDomainData(dataArray);
        let sumSquares = 0;
        for (let i = 0; i < dataArray.length; i++) {
          const norm = (dataArray[i] - 128) / 128;
          sumSquares += norm * norm;
        }
        const rms = Math.sqrt(sumSquares / dataArray.length);
        // Scale for human speech sensitivity
        raw = Math.min(1, rms * 4.5);
        localLevelRef.current = raw;
      } else if (levelRef) {
        raw = Math.min(1, Math.max(0, levelRef.current));
      }

      // VU Ballistics
      const tau = raw >= meter ? ATTACK_MS : RELEASE_MS;
      meter += (raw - meter) * (1 - Math.exp(-dt / tau));

      if (meter >= peak) {
        peak = meter;
        peakAt = now;
      } else if (now - peakAt > HOLD_MS) {
        peak += (meter - peak) * (1 - Math.exp(-dt / RELEASE_MS));
      }

      const active = meter > 0.02;
      if (rootEl) {
        rootEl.dataset.active = active ? "true" : "false";
      }

      // Perceptual scale: sqrt curve
      const scaledMeter = Math.sqrt(meter);
      const scaledPeak = Math.sqrt(peak);

      const litCount = Math.round(scaledMeter * numSegments);
      const peakIndex = Math.min(
        numSegments - 1,
        Math.floor(scaledPeak * numSegments),
      );

      for (let i = 0; i < numSegments; i++) {
        const node = segmentNodes[i];
        const isLit = i < litCount;
        const isPeak = i === peakIndex && peakIndex > 0 && !isLit;

        node.dataset.lit = isLit ? "true" : "false";
        node.dataset.peak = isPeak ? "true" : "false";
      }
    };

    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [bars, levelRef, isTesting]);

  return (
    <div
      className={`mic-test-meter ${className}`}
      ref={rootRef}
      role="region"
      aria-label="Microphone input meter"
    >
      <span className="mic-test-icon-wrap" aria-hidden="true">
        <svg viewBox="0 0 12 16" width="11" height="14" fill="none" focusable="false">
          <rect
            x="4"
            y="1"
            width="4"
            height="7"
            rx="2"
            fill="currentColor"
          />
          <path
            d="M2 7a4 4 0 0 0 8 0M6 11v3"
            stroke="currentColor"
            strokeWidth="1.3"
            strokeLinecap="round"
          />
        </svg>
      </span>

      <div
        className="mic-test-track"
        ref={trackRef}
        role="meter"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Audio input level"
      >
        {Array.from({ length: bars }, (_, i) => {
          const fraction = (i + 1) / bars;
          const zone =
            fraction > 0.85 ? "hot" : fraction > 0.65 ? "warm" : "normal";
          return (
            <span
              key={i}
              className="mic-test-segment"
              data-zone={zone}
              data-lit="false"
              data-peak="false"
            />
          );
        })}
      </div>

      {showTestButton && (
        <button
          type="button"
          className="mic-test-btn"
          data-testing={isTesting}
          onClick={toggleTest}
          aria-pressed={isTesting}
          title={isTesting ? "Stop microphone test" : "Test microphone level"}
        >
          {isTesting ? "Stop" : "Test"}
        </button>
      )}
    </div>
  );
}
