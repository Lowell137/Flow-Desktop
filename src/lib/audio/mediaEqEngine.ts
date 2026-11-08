import { logToBackend } from "../diagnostics";
import { createAudioContext, keepRunning, resumeWithin } from "./audioContext";
import { EQ_GRAPH_OFF, type EqGraphSpec } from "./eq/graphPlan";
import { EqChain, RAMP_TIME_CONSTANT } from "./eqChain";
import { MAX_BOOST } from "./musicAudioEngine";
import { probeMediaElementRouting } from "./webAudioProbe";

/** Where the limiter starts pulling peaks down while a boost is on. */
const BOOST_LIMIT_DB = -3;

// One context for every video player in this window; each player keeps its own chain.
let sharedContext: AudioContext | null = null;
let routingWorks: Promise<boolean | null> | null = null;
// WebAudio failed or delivered silence in this window: video plays from its elements alone.
let unavailable = false;
const routedElements = new Set<HTMLMediaElement>();

function videoContext(): AudioContext | null {
  if (!sharedContext) {
    sharedContext = createAudioContext();
    if (sharedContext) keepRunning(sharedContext, () => [...routedElements].some((el) => !el.paused));
  }
  return sharedContext;
}

/** Runs the routing self-test once per window; a test that could not run is tried again later. */
function checkRouting(ctx: AudioContext): Promise<boolean | null> {
  routingWorks ??= probeMediaElementRouting(ctx).then((works) => {
    if (works === null) routingWorks = null;
    if (works === false) {
      unavailable = true;
      void logToBackend("warn", "video equalizer off: WebAudio returned silence from a media element");
    }
    return works;
  });
  return routingWorks;
}

/**
 * The equalizer for one video player: its <video> element and the separate <audio> element
 * adaptive playback uses. Elements are routed through WebAudio only once the equalizer changes
 * the sound, and each gets its source node exactly once in its life, as WebAudio requires.
 */
export class MediaEqEngine {
  private chain: EqChain | null = null;
  private elements: HTMLMediaElement[] = [];
  private sources = new WeakMap<HTMLMediaElement, MediaElementAudioSourceNode>();
  private watched = new WeakSet<HTMLMediaElement>();
  private spec: EqGraphSpec = EQ_GRAPH_OFF;
  private eqEngaged = false;
  private boost = 1;
  private boostGain: GainNode | null = null;
  private limiter: DynamicsCompressorNode | null = null;
  private routing = false;

  /** Routing is needed once the equalizer changes the sound or the volume is amplified. */
  private get engaged(): boolean {
    return this.eqEngaged || this.boost > 1;
  }

  /** `factor` of 1 is off; above 1 the video's sound is amplified (and peaks limited). */
  setBoost(factor: number): void {
    this.boost = Number.isFinite(factor) ? Math.max(1, Math.min(MAX_BOOST, factor)) : 1;
    if (this.engaged) void this.route();
    this.applyBoost();
  }

  private applyBoost(): void {
    if (!sharedContext || !this.boostGain || !this.limiter) return;
    const t = sharedContext.currentTime;
    this.boostGain.gain.setTargetAtTime(this.boost, t, RAMP_TIME_CONSTANT);
    this.limiter.threshold.setTargetAtTime(this.boost > 1 ? BOOST_LIMIT_DB : 0, t, RAMP_TIME_CONSTANT);
  }

  /** The player's current media elements; an element that left is let go. */
  setElements(elements: (HTMLMediaElement | null)[]): void {
    const next = elements.filter((el): el is HTMLMediaElement => el !== null);
    for (const el of this.elements) {
      if (next.includes(el)) continue;
      this.sources.get(el)?.disconnect();
      routedElements.delete(el);
    }
    this.elements = next;
    for (const el of next) {
      if (this.watched.has(el)) continue;
      this.watched.add(el);
      // A routed element needs its context running; one not routed yet gets another chance,
      // since the self-test needs a context the webview lets run.
      el.addEventListener("play", () => {
        if (routedElements.has(el) && sharedContext) void resumeWithin(sharedContext);
        else if (this.engaged) void this.route();
      });
    }
    if (this.engaged) void this.route();
  }

  setEq(spec: EqGraphSpec, engaged: boolean): void {
    this.spec = spec;
    this.eqEngaged = engaged;
    if (this.engaged) void this.route();
    this.chain?.apply(spec);
  }

  dispose(): void {
    this.setElements([]);
    this.chain?.output.disconnect();
    this.boostGain?.disconnect();
    this.limiter?.disconnect();
    this.chain = null;
    this.boostGain = null;
    this.limiter = null;
  }

  private async route(): Promise<void> {
    if (unavailable || this.routing) return;
    const ctx = videoContext();
    if (!ctx) {
      unavailable = true;
      return;
    }
    this.routing = true;
    try {
      if ((await checkRouting(ctx)) !== true || !this.engaged) return;
      if (!this.chain) {
        this.chain = new EqChain(ctx);
        this.boostGain = ctx.createGain();
        this.limiter = ctx.createDynamicsCompressor();
        this.limiter.knee.value = 0;
        this.limiter.ratio.value = 20;
        this.limiter.attack.value = 0.003;
        this.limiter.release.value = 0.1;
        this.chain.output.connect(this.boostGain);
        this.boostGain.connect(this.limiter);
        this.limiter.connect(ctx.destination);
        this.chain.apply(this.spec);
        this.applyBoost();
      }
      for (const el of this.elements) this.connect(ctx, this.chain, el);
    } catch (error) {
      unavailable = true;
      console.warn("[mediaEqEngine] WebAudio unavailable, video plays without the equalizer", error);
    } finally {
      this.routing = false;
    }
  }

  private connect(ctx: AudioContext, chain: EqChain, el: HTMLMediaElement): void {
    if (this.sources.has(el) && routedElements.has(el)) return;
    // Without CORS from the start the samples reach WebAudio as silence.
    if (el.crossOrigin !== "anonymous") return;
    let source = this.sources.get(el);
    if (!source) {
      source = ctx.createMediaElementSource(el);
      this.sources.set(el, source);
      void logToBackend("info", "video equalizer routed", {
        element: el.tagName.toLowerCase(),
        baseLatencyMs: Math.round(ctx.baseLatency * 1_000),
        outputLatencyMs: Math.round((ctx.outputLatency ?? 0) * 1_000),
      });
    }
    source.connect(chain.input);
    routedElements.add(el);
    if (!el.paused) void resumeWithin(ctx);
  }
}

/** False once WebAudio failed or proved silent for video in this window. */
export const videoEqAvailable = (): boolean => !unavailable;
