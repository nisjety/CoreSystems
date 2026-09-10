import type { SessionStats } from '../types/public.js';

/**
 * Alle tallene har nå en reell kilde:
 *  - `decodedFrames`/`fps` fra dekoderens utgang.
 *  - `bitrateKbps` fra faktiske mottatte bytes over et rullende sekund — vår
 *    egen måling, ikke vertens måltall (det rapporteres separat i
 *    'quality'-hendelsen).
 *  - `droppedFrames` fra dekoderen (frames forkastet før første keyframe).
 *  - `renderedFrames` fra rendereren, som melder inn hver maling.
 * Er ingen renderer koblet på, blir `renderedFrames` stående på 0 — det er
 * korrekt, ikke et hull: da males ingenting.
 */
export class SessionStatsTracker {
  private decodedFrames = 0;
  private renderedFrames = 0;
  private droppedFrames = 0;
  private latencyMs = 0;
  private readonly frameTimestamps: number[] = [];
  /** [tidspunkt, bytes] over det siste sekundet. */
  private readonly byteSamples: Array<readonly [number, number]> = [];
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

  /** Faktiske bytes tatt imot for en videomelding. */
  recordEncodedBytes(bytes: number): void {
    if (!Number.isFinite(bytes) || bytes <= 0) return;
    const timestamp = this.now();
    this.byteSamples.push([timestamp, bytes]);
    this.pruneByteSamples(timestamp);
  }

  /** Dekoderen teller kumulativt, så dette er et absolutt tall — ikke et inkrement. */
  setDroppedFrames(total: number): void {
    if (!Number.isFinite(total) || total < 0) return;
    this.droppedFrames = total;
  }

  private pruneByteSamples(nowMs: number): void {
    const windowStart = nowMs - 1000;
    while (true) {
      const oldest = this.byteSamples[0];
      if (oldest === undefined || oldest[0] >= windowStart) break;
      this.byteSamples.shift();
    }
  }

  getSnapshot(): SessionStats {
    this.pruneByteSamples(this.now());
    const bytesInWindow = this.byteSamples.reduce((sum, [, bytes]) => sum + bytes, 0);
    return {
      latencyMs: this.latencyMs,
      fps: this.frameTimestamps.length,
      // bytes/s -> kilobit/s
      bitrateKbps: Math.round((bytesInWindow * 8) / 1000),
      droppedFrames: this.droppedFrames,
      decodedFrames: this.decodedFrames,
      renderedFrames: this.renderedFrames,
    };
  }
}
