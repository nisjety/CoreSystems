import type { RemoteVideoFrame } from '../types/public.js';

export interface FrameSamplerOptions {
  readonly maxFramesPerSecond: number;
  readonly displayId?: string;
  readonly now?: () => number;
}

export type FrameHandler = (frame: RemoteVideoFrame) => void;

/**
 * Begrenser hvor mange frames AI-laget faktisk får se. Skjermbilder sendes
 * ALDRI automatisk for hver frame — se "Do NOT automatically send every
 * screen frame to AI." `now` kan injiseres for deterministiske tester.
 */
export class FrameSampler {
  private lastEmittedAt = -Infinity;
  private readonly minIntervalMs: number;
  private readonly handlers = new Set<FrameHandler>();
  private readonly now: () => number;
  private paused = false;

  constructor(private readonly options: FrameSamplerOptions) {
    this.minIntervalMs = options.maxFramesPerSecond > 0 ? 1000 / options.maxFramesPerSecond : Infinity;
    this.now = options.now ?? (() => Date.now());
  }

  get isPaused(): boolean {
    return this.paused;
  }

  pause(): void {
    this.paused = true;
  }

  resume(): void {
    this.paused = false;
  }

  onFrame(handler: FrameHandler): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  /** Kalles for hver innkommende frame; slipper den videre kun hvis den består filtrene. */
  submit(frame: RemoteVideoFrame): void {
    if (this.paused) return;
    if (this.options.displayId && frame.displayId !== this.options.displayId) return;

    const timestamp = this.now();
    if (timestamp - this.lastEmittedAt < this.minIntervalMs) return;

    this.lastEmittedAt = timestamp;
    for (const handler of [...this.handlers]) handler(frame);
  }

  close(): void {
    this.handlers.clear();
  }
}
