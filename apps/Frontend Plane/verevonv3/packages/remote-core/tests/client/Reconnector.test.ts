import { describe, expect, it, vi } from 'vitest';
import { Reconnector, type ReconnectPolicy } from '../../src/client/Reconnector.js';
import { AuthenticationError, PermissionDeniedError, TransportError } from '../../src/errors/RemoteError.js';
import { noopLogger } from '../../src/logging/Logger.js';

const POLICY: ReconnectPolicy = { maxAttempts: 4, baseDelayMs: 100, maxDelayMs: 350, attemptTimeoutMs: 0 };

function recordingSleep(): { sleep: (ms: number) => Promise<void>; delays: number[] } {
  const delays: number[] = [];
  return {
    delays,
    sleep: async (ms) => {
      delays.push(ms);
    },
  };
}

describe('Reconnector', () => {
  it('backs off exponentially, caps at maxDelayMs, and stops on the first success', async () => {
    const { sleep, delays } = recordingSleep();
    let calls = 0;
    const attempt = vi.fn(async () => {
      calls += 1;
      if (calls < 3) throw new TransportError(`attempt ${calls} failed`);
    });

    const reconnector = new Reconnector({ policy: POLICY, attempt, logger: noopLogger, sleep });
    await expect(reconnector.run()).resolves.toEqual({ kind: 'succeeded', attempts: 3 });

    expect(attempt).toHaveBeenCalledTimes(3);
    // 100, 200, then 400 capped to 350 — and no fourth wait after success.
    expect(delays).toEqual([100, 200, 350]);
  });

  it('gives up honestly once the policy is exhausted, and reports the last error', async () => {
    const { sleep, delays } = recordingSleep();
    const attempt = vi.fn(async () => {
      throw new TransportError('still down');
    });

    const reconnector = new Reconnector({ policy: POLICY, attempt, logger: noopLogger, sleep });
    const outcome = await reconnector.run();

    expect(outcome).toMatchObject({ kind: 'exhausted', attempts: POLICY.maxAttempts });
    if (outcome.kind !== 'exhausted') throw new Error('expected exhausted');
    expect(outcome.lastError?.message).toBe('still down');
    expect(attempt).toHaveBeenCalledTimes(POLICY.maxAttempts);
    expect(delays).toEqual([100, 200, 350, 350]);
  });

  it('maxAttempts: 0 never attempts anything', async () => {
    const attempt = vi.fn(async () => undefined);
    const reconnector = new Reconnector({
      policy: { ...POLICY, maxAttempts: 0 },
      attempt,
      logger: noopLogger,
      sleep: async () => undefined,
    });

    await expect(reconnector.run()).resolves.toMatchObject({ kind: 'exhausted', attempts: 0 });
    expect(attempt).not.toHaveBeenCalled();
  });

  it('cancel() during the backoff wait prevents the next attempt', async () => {
    const attempt = vi.fn(async () => {
      throw new TransportError('down');
    });
    let release: (() => void) | undefined;
    const sleep = (): Promise<void> =>
      new Promise((resolve) => {
        release = resolve;
      });

    const reconnector = new Reconnector({ policy: POLICY, attempt, logger: noopLogger, sleep });
    const run = reconnector.run();
    expect(reconnector.isRunning).toBe(true);

    // Cancel while the first backoff is pending, then let the wait finish.
    reconnector.cancel();
    release?.();

    await expect(run).resolves.toEqual({ kind: 'cancelled' });
    expect(attempt).not.toHaveBeenCalled();
    expect(reconnector.isRunning).toBe(false);
  });

  it('cancel() after a failed attempt stops the loop without another wait', async () => {
    const { sleep, delays } = recordingSleep();
    // The attempt needs a handle to the reconnector that owns it.
    const holder: { current?: Reconnector } = {};
    const attempt = vi.fn(async () => {
      holder.current?.cancel();
      throw new TransportError('down');
    });

    holder.current = new Reconnector({ policy: POLICY, attempt, logger: noopLogger, sleep });
    await expect(holder.current.run()).resolves.toEqual({ kind: 'cancelled' });

    expect(attempt).toHaveBeenCalledTimes(1);
    expect(delays).toEqual([100]);
  });

  it('refuses to run twice concurrently', async () => {
    let release: (() => void) | undefined;
    const attempt = (): Promise<void> =>
      new Promise((resolve) => {
        release = resolve;
      });
    const reconnector = new Reconnector({
      policy: POLICY,
      attempt,
      logger: noopLogger,
      sleep: async () => undefined,
    });

    const first = reconnector.run();
    await Promise.resolve();
    await expect(reconnector.run()).resolves.toEqual({ kind: 'cancelled' });

    release?.();
    await expect(first).resolves.toMatchObject({ kind: 'succeeded' });
  });

  // A wrong password or a revoked permission cannot be fixed by waiting, and on
  // a 2FA host each retry would mean another code prompt the operator never asked for.
  it.each([
    ['an authentication failure', new AuthenticationError('Wrong Password')],
    ['a revoked permission', new PermissionDeniedError('input.pointer')],
  ])('stops immediately on %s instead of burning every attempt', async (_label, error) => {
    const { sleep, delays } = recordingSleep();
    const attempt = vi.fn(async () => {
      throw error;
    });

    const reconnector = new Reconnector({ policy: POLICY, attempt, logger: noopLogger, sleep });
    const outcome = await reconnector.run();

    expect(outcome).toMatchObject({ kind: 'refused' });
    expect(attempt).toHaveBeenCalledTimes(1);
    expect(delays).toEqual([100]);
  });

  it('a hung attempt is bounded by attemptTimeoutMs so the loop can still finish', async () => {
    const { sleep, delays } = recordingSleep();
    const deadlines: number[] = [];
    // Injected scheduler fires synchronously, so no real clock is involved.
    const scheduleTimeout = (handler: () => void, ms: number): unknown => {
      deadlines.push(ms);
      handler();
      return deadlines.length;
    };
    // Never settles — exactly the "operator walked away from the 2FA prompt" case.
    const attempt = vi.fn(() => new Promise<void>(() => undefined));

    const reconnector = new Reconnector({
      policy: { maxAttempts: 2, baseDelayMs: 10, maxDelayMs: 10, attemptTimeoutMs: 5_000 },
      attempt,
      logger: noopLogger,
      sleep,
      scheduleTimeout,
      clearScheduled: () => undefined,
    });

    const outcome = await reconnector.run();

    expect(outcome).toMatchObject({ kind: 'exhausted', attempts: 2 });
    if (outcome.kind !== 'exhausted') throw new Error('expected exhausted');
    expect(outcome.lastError?.message).toMatch(/exceeded 5000 ms/);
    expect(attempt).toHaveBeenCalledTimes(2);
    expect(delays).toEqual([10, 10]);
    expect(deadlines).toEqual([5_000, 5_000]);
  });

  it('clears the deadline once an attempt settles, leaving no timer behind', async () => {
    const clearScheduled = vi.fn();
    const reconnector = new Reconnector({
      policy: { maxAttempts: 1, baseDelayMs: 1, maxDelayMs: 1, attemptTimeoutMs: 5_000 },
      attempt: async () => undefined,
      logger: noopLogger,
      sleep: async () => undefined,
      scheduleTimeout: () => 'deadline-handle',
      clearScheduled,
    });

    await expect(reconnector.run()).resolves.toMatchObject({ kind: 'succeeded' });
    expect(clearScheduled).toHaveBeenCalledWith('deadline-handle');
  });
});
