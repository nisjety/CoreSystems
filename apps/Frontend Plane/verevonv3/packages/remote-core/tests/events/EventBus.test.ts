import { describe, expect, it, vi } from 'vitest';
import { EventBus } from '../../src/events/EventBus.js';

interface Events extends Record<string, unknown> {
  ping: { value: number };
}

describe('EventBus', () => {
  it('delivers emitted payloads to subscribed handlers', () => {
    const bus = new EventBus<Events>();
    const handler = vi.fn();
    bus.on('ping', handler);

    bus.emit('ping', { value: 42 });

    expect(handler).toHaveBeenCalledWith({ value: 42 });
  });

  it('stops delivering after unsubscribe', () => {
    const bus = new EventBus<Events>();
    const handler = vi.fn();
    const unsubscribe = bus.on('ping', handler);
    unsubscribe();

    bus.emit('ping', { value: 1 });

    expect(handler).not.toHaveBeenCalled();
  });

  it('tolerates a handler unsubscribing itself mid-emit', () => {
    const bus = new EventBus<Events>();
    const second = vi.fn();
    const unsubscribeFirst = bus.on('ping', () => unsubscribeFirst());
    bus.on('ping', second);

    expect(() => bus.emit('ping', { value: 1 })).not.toThrow();
    expect(second).toHaveBeenCalledTimes(1);
  });

  it('clear() removes every listener', () => {
    const bus = new EventBus<Events>();
    const handler = vi.fn();
    bus.on('ping', handler);
    bus.clear();

    bus.emit('ping', { value: 1 });

    expect(handler).not.toHaveBeenCalled();
  });
});
