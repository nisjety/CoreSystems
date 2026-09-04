import type { SessionStats } from '../types/public.js';

/**
 * All statistikk starter på ærlige nulltall. `bitrateKbps`, `renderedFrames`
 * og `droppedFrames` har ingen reell datakilde før dekode-/render-pipelinen
 * (fase 3/6 i planen) faktisk mater dem — se docs/architecture.md for status.
 * Å late som disse tallene betyr noe før den koblingen finnes ville brutt
 * "do not fake functionality".
 */
export class SessionStatsTracker {
  private decodedFrames = 0;
  private renderedFrames = 0;
  private droppedFrames = 0;
  private latencyMs = 0;
  private bitrateKbps = 0;
  private readonly frameTimestamps: number[] = [];
  private readonly now: () => number;

  constructor(now: () => number = () => Date.now()) {
    this.now = now;
  }

  recordDecodedFrame(): void {
    this.decodedFrames += 1;
    const timestamp = this.now();
    this.frameTimestamps.push(timestamp);
    const windowStart = timestamp - 1000;
    while (true) {
      const oldest = this.frameTimestamps[0];
      if (oldest === undefined || oldest >= windowStart) break;
      this.frameTimestamps.shift();
    }
  }

  recordRenderedFrame(): void {
    this.renderedFrames += 1;
  }

  recordDroppedFrame(): void {
    this.droppedFrames += 1;
  }

  recordLatency(ms: number): void {
    this.latencyMs = ms;
  }

  getSnapshot(): SessionStats {
    return {
      latencyMs: this.latencyMs,
      fps: this.frameTimestamps.length,
      bitrateKbps: this.bitrateKbps,
      droppedFrames: this.droppedFrames,
      decodedFrames: this.decodedFrames,
      renderedFrames: this.renderedFrames,
    };
  }
}
