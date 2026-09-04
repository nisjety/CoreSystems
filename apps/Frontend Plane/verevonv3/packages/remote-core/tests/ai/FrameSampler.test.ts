import { describe, expect, it, vi } from 'vitest';
import { FrameSampler } from '../../src/ai/FrameSampler.js';
import type { RemoteVideoFrame } from '../../src/types/public.js';

function frame(overrides: Partial<RemoteVideoFrame> = {}): RemoteVideoFrame {
  return { width: 10, height: 10, timestamp: 0, ...overrides };
}

describe('FrameSampler', () => {
  it('never forwards frames faster than maxFramesPerSecond', () => {
    let now = 0;
    const sampler = new FrameSampler({ maxFramesPerSecond: 1, now: () => now });
    const received: RemoteVideoFrame[] = [];
    sampler.onFrame((f) => received.push(f));

    sampler.submit(frame());
    now += 500;
    sampler.submit(frame());
    now += 600;
    sampler.submit(frame());

    expect(received).toHaveLength(2);
  });

  it('drops frames while paused', () => {
    const now = 0;
    const sampler = new FrameSampler({ maxFramesPerSecond: 100, now: () => now });
    const received: RemoteVideoFrame[] = [];
    sampler.onFrame((f) => received.push(f));

    sampler.pause();
    sampler.submit(frame());
    expect(received).toHaveLength(0);

    sampler.resume();
    sampler.submit(frame());
    expect(received).toHaveLength(1);
  });

  it('filters by displayId when configured', () => {
    let now = 0;
    const sampler = new FrameSampler({ maxFramesPerSecond: 100, displayId: 'primary', now: () => now });
    const received: RemoteVideoFrame[] = [];
    sampler.onFrame((f) => received.push(f));

    sampler.submit(frame({ displayId: 'secondary' }));
    now += 10;
    sampler.submit(frame({ displayId: 'primary' }));

    expect(received).toHaveLength(1);
    expect(received[0]?.displayId).toBe('primary');
  });

  it('close() removes all handlers', () => {
    const now = 0;
    const sampler = new FrameSampler({ maxFramesPerSecond: 100, now: () => now });
    const handler = vi.fn();
    sampler.onFrame(handler);
    sampler.close();

    sampler.submit(frame());

    expect(handler).not.toHaveBeenCalled();
  });
});
