import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

function fakeOscillator() {
  return { type: "", frequency: { value: 0 }, onended: null as (() => void) | null,
    connect: vi.fn((node: unknown) => node), disconnect: vi.fn(), start: vi.fn(), stop: vi.fn() };
}

function fakeGain() {
  return { gain: { value: 1, setValueAtTime: vi.fn(), exponentialRampToValueAtTime: vi.fn() },
    connect: vi.fn(), disconnect: vi.fn() };
}

function fakeBufferSource() {
  return { buffer: null as AudioBuffer | null, onended: null as (() => void) | null,
    connect: vi.fn(), disconnect: vi.fn(), start: vi.fn(), stop: vi.fn() };
}

class FakeContext {
  static instances: FakeContext[] = [];
  currentTime = 10;
  state: AudioContextState = "running";
  destination = {};
  resume = vi.fn(async () => { this.state = "running"; });
  oscillators: ReturnType<typeof fakeOscillator>[] = [];
  gains: ReturnType<typeof fakeGain>[] = [];
  sources: ReturnType<typeof fakeBufferSource>[] = [];
  decodeAudioData = vi.fn(async () => ({ duration: 0.18 }) as AudioBuffer);
  constructor() { FakeContext.instances.push(this); }
  createOscillator() {
    const oscillator = fakeOscillator();
    this.oscillators.push(oscillator);
    return oscillator;
  }
  createGain() {
    const gain = fakeGain();
    this.gains.push(gain);
    return gain;
  }
  createBufferSource() {
    const source = fakeBufferSource();
    this.sources.push(source);
    return source;
  }
}

beforeEach(() => { vi.resetModules(); FakeContext.instances = []; vi.stubGlobal("AudioContext", FakeContext); });
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("sound playback", () => {
  it("cannot break dictation when the output device cannot initialize", async () => {
    vi.stubGlobal("AudioContext", class { constructor() { throw new Error("Output unavailable"); } });
    const { playFlowSound } = await import("./sound");
    await expect(playFlowSound("start")).resolves.toBeUndefined();
  });

  it("disconnects oscillator and gain nodes when a cue finishes", async () => {
    const { playFlowSound } = await import("./sound");
    await playFlowSound("start");
    const context = FakeContext.instances[0];
    expect(context.oscillators.length).toBeGreaterThan(0);
    for (const oscillator of context.oscillators) {
      expect(oscillator.onended).toBeTypeOf("function");
      oscillator.onended?.();
      expect(oscillator.disconnect).toHaveBeenCalledOnce();
    }
    for (const gain of context.gains) expect(gain.disconnect).toHaveBeenCalledOnce();
  });

  it("silently handles a rejected audio resume", async () => {
    class Unavailable extends FakeContext {
      state: AudioContextState = "suspended";
      resume = vi.fn(async () => { throw new Error("Autoplay denied"); });
    }
    vi.stubGlobal("AudioContext", Unavailable);
    const { playFlowSound } = await import("./sound");
    await expect(playFlowSound("start")).resolves.toBeUndefined();
    expect(FakeContext.instances[0].oscillators).toHaveLength(0);
  });

  it("drops an obsolete start cue when resuming audio takes longer than the recording", async () => {
    let resume!: () => void;
    class Suspended extends FakeContext {
      state: AudioContextState = "suspended";
      resume = vi.fn(() => new Promise<void>(resolve => {
        resume = () => { this.state = "running"; resolve(); };
      }));
    }
    vi.stubGlobal("AudioContext", Suspended);
    const { playFlowSound } = await import("./sound");
    const start = playFlowSound("start");
    const firstResume = resume;
    const stop = playFlowSound("stop");
    firstResume();
    resume();
    await Promise.all([start, stop]);
    const context = FakeContext.instances[0];
    expect(context.oscillators).toHaveLength(2);
    expect(context.oscillators[0].frequency.value).toBeLessThan(400);
  });

  it("muting cancels a pending resume and disconnects an audible cue immediately", async () => {
    const { playFlowSound, silenceFlowSounds } = await import("./sound");
    await playFlowSound("success");
    const context = FakeContext.instances[0];
    silenceFlowSounds();
    for (const oscillator of context.oscillators) expect(oscillator.disconnect).toHaveBeenCalledOnce();
    const count = context.oscillators.length;
    context.state = "suspended";
    let resume!: () => void;
    context.resume = vi.fn(() => new Promise<void>(resolve => { resume = () => { context.state = "running"; resolve(); }; }));
    const pending = playFlowSound("start");
    silenceFlowSounds();
    resume();
    await pending;
    expect(context.oscillators).toHaveLength(count);
  });

  it("does not replay a stale cue after an autoplay permission delay", async () => {
    let resume!: () => void;
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    class Suspended extends FakeContext {
      state: AudioContextState = "suspended";
      resume = vi.fn(() => new Promise<void>(resolve => {
        resume = () => { this.state = "running"; resolve(); };
      }));
    }
    vi.stubGlobal("AudioContext", Suspended);
    const { playFlowSound } = await import("./sound");
    const pending = playFlowSound("start");
    now = 1000;
    resume();
    await pending;
    expect(FakeContext.instances[0].oscillators).toHaveLength(0);
  });

  it("preloads only local WAVs and plays the decoded reference without synthesizing over it", async () => {
    const urls: string[] = [];
    vi.stubGlobal("fetch", async (url: string) => {
      urls.push(url);
      return { ok: true, arrayBuffer: async () => new ArrayBuffer(44) };
    });
    const { preloadFlowSounds, playFlowSound, silenceFlowSounds } = await import("./sound");
    await preloadFlowSounds();
    expect(urls.sort()).toEqual(["/flow-sounds/error.wav", "/flow-sounds/lock.wav", "/flow-sounds/start.wav", "/flow-sounds/stop.wav", "/flow-sounds/success.wav"]);
    const context = FakeContext.instances[0];
    expect(context.sources).toHaveLength(0);
    await playFlowSound("start");
    expect(context.oscillators).toHaveLength(0);
    expect(context.sources).toHaveLength(1);
    expect(context.sources[0].buffer).toEqual({ duration: 0.18 });
    expect(context.sources[0].start).toHaveBeenCalledOnce();
    silenceFlowSounds();
    expect(context.sources[0].stop).toHaveBeenCalledOnce();
    expect(context.sources[0].disconnect).toHaveBeenCalledOnce();
    context.sources[0].onended?.();
    expect(context.sources[0].disconnect).toHaveBeenCalledOnce();
    await preloadFlowSounds();
    expect(urls).toHaveLength(5);
  });

  it("falls back immediately when the optional local pack is missing or still loading", async () => {
    let finishLoading!: () => void;
    const response = new Promise(resolve => {
      finishLoading = () => resolve({ ok: false });
    });
    vi.stubGlobal("fetch", () => response);
    const { preloadFlowSounds, playFlowSound } = await import("./sound");
    const loading = preloadFlowSounds();
    await playFlowSound("stop");
    expect(FakeContext.instances[0].oscillators).toHaveLength(2);
    finishLoading();
    await loading;
    expect(FakeContext.instances[0].sources).toHaveLength(0);
  });

  it("a corrupt local recording cannot disable synthesized feedback", async () => {
    vi.stubGlobal("fetch", async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(0) }));
    class Corrupt extends FakeContext {
      decodeAudioData = vi.fn(async (): Promise<AudioBuffer> => { throw new Error("Invalid WAV"); });
    }
    vi.stubGlobal("AudioContext", Corrupt);
    const { preloadFlowSounds, playFlowSound } = await import("./sound");
    await preloadFlowSounds();
    await playFlowSound("start");
    expect(FakeContext.instances[0].oscillators).toHaveLength(2);
  });

  it("plays the reference error at half volume and disposes its gain with the source", async () => {
    vi.stubGlobal("fetch", async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(44) }));
    const { preloadFlowSounds, playFlowSound, silenceFlowSounds } = await import("./sound");
    await preloadFlowSounds();
    await playFlowSound("error");
    const context = FakeContext.instances[0];
    expect(context.gains).toHaveLength(1);
    expect(context.gains[0].gain.value).toBe(0.5);
    expect(context.sources[0].connect).toHaveBeenCalledWith(context.gains[0]);
    expect(context.gains[0].connect).toHaveBeenCalledWith(context.destination);
    context.sources[0].onended?.();
    expect(context.sources[0].disconnect).toHaveBeenCalledOnce();
    expect(context.gains[0].disconnect).toHaveBeenCalledOnce();
    silenceFlowSounds();
    expect(context.gains[0].disconnect).toHaveBeenCalledOnce();
    await playFlowSound("error");
    silenceFlowSounds();
    expect(context.sources[1].stop).toHaveBeenCalledOnce();
    expect(context.gains[1].disconnect).toHaveBeenCalledOnce();
  });
});
