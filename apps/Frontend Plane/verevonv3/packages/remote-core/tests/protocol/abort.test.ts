import { describe, expect, it, vi } from 'vitest';
import { abortError, throwIfAborted, withAbort, withTimeout } from '../../src/protocol/abort.js';
import { RemoteConnectionError } from '../../src/errors/RemoteError.js';

describe('throwIfAborted', () => {
  it('does nothing without a signal, or with an un-aborted one', () => {
    expect(() => throwIfAborted(undefined)).not.toThrow();
    expect(() => throwIfAborted(new AbortController().signal)).not.toThrow();
  });

  it('throws a typed RemoteConnectionError once aborted', () => {
    const controller = new AbortController();
    controller.abort();
    expect(() => throwIfAborted(controller.signal)).toThrow(RemoteConnectionError);
    expect(() => throwIfAborted(controller.signal)).toThrow(/aborted by the caller/i);
  });

  it('reports the abort as a user-initiated reason, not a protocol error', () => {
    expect(abortError().metadata).toMatchObject({ reason: 'user' });
  });
});

describe('withAbort', () => {
  it('passes the value through when nothing aborts', async () => {
    await expect(withAbort(Promise.resolve(7), new AbortController().signal)).resolves.toBe(7);
    await expect(withAbort(Promise.resolve(7), undefined)).resolves.toBe(7);
  });

  it('rejects immediately when the signal is already aborted, without awaiting the work', async () => {
    const controller = new AbortController();
    controller.abort();
    const work = vi.fn(() => new Promise<number>(() => undefined)); // never settles

    await expect(withAbort(work(), controller.signal)).rejects.toThrow(RemoteConnectionError);
  });

  it('rejects as soon as the signal fires, even if the work never settles', async () => {
    const controller = new AbortController();
    const pending = withAbort(new Promise<void>(() => undefined), controller.signal);

    controller.abort();

    await expect(pending).rejects.toThrow(/aborted by the caller/i);
  });

  it('propagates the original rejection rather than masking it as an abort', async () => {
    const controller = new AbortController();
    await expect(withAbort(Promise.reject(new Error('real failure')), controller.signal)).rejects.toThrow(
      'real failure',
    );
  });

  it('removes its abort listener so a long-lived signal does not accumulate them', async () => {
    const controller = new AbortController();
    const remove = vi.spyOn(controller.signal, 'removeEventListener');

    await withAbort(Promise.resolve('done'), controller.signal);

    expect(remove).toHaveBeenCalledTimes(1);
  });
});

describe('withTimeout', () => {
  it('passes the value through when the work finishes first', async () => {
    await expect(
      withTimeout(Promise.resolve('fast'), 1000, () => new Error('too slow')),
    ).resolves.toBe('fast');
  });

  it('rejects with the supplied error when the deadline passes', async () => {
    // Injected scheduler: fires synchronously, so the test needs no real clock.
    const immediate = (handler: () => void): unknown => {
      handler();
      return 1;
    };

    await expect(
      withTimeout(
        new Promise<void>(() => undefined),
        1000,
        () => new Error('timed out waiting'),
        immediate,
        () => undefined,
      ),
    ).rejects.toThrow('timed out waiting');
  });

  it('treats a non-positive or infinite deadline as "no deadline"', async () => {
    const schedule = vi.fn();
    await expect(
      withTimeout(Promise.resolve('x'), 0, () => new Error('nope'), schedule, () => undefined),
    ).resolves.toBe('x');
    await expect(
      withTimeout(Promise.resolve('x'), Number.POSITIVE_INFINITY, () => new Error('nope'), schedule, () => undefined),
    ).resolves.toBe('x');
    expect(schedule).not.toHaveBeenCalled();
  });

  it('clears the pending timer once the work wins, so it cannot fire later', async () => {
    const clear = vi.fn();
    await withTimeout(Promise.resolve('x'), 1000, () => new Error('nope'), () => 42, clear);
    expect(clear).toHaveBeenCalledWith(42);
  });
});
