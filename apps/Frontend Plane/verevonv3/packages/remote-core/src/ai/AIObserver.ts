import type { AIObserver, AIObserverEventMap, RemoteVideoFrame, Unsubscribe } from '../types/public.js';
import type { FrameSampler } from './FrameSampler.js';

/**
 * Tynn adapter fra den offentlige AIObserver-typen til FrameSampler.
 * AIObserverEventMap har i dag kun ett medlem ('frame'); casten i `on`
 * bygger bro mellom den generiske signaturen og FrameSamplers konkrete
 * håndterertype.
 */
export class AIObserverImpl implements AIObserver {
  constructor(
    private readonly sampler: FrameSampler,
    private readonly onDispose: () => void,
  ) {}

  on<T extends keyof AIObserverEventMap>(
    _event: T,
    handler: (payload: AIObserverEventMap[T]) => void,
  ): Unsubscribe {
    return this.sampler.onFrame(handler as unknown as (frame: RemoteVideoFrame) => void);
  }

  pause(): void {
    this.sampler.pause();
  }

  resume(): void {
    this.sampler.resume();
  }

  get isPaused(): boolean {
    return this.sampler.isPaused;
  }

  close(): void {
    this.sampler.close();
    this.onDispose();
  }
}
