import { createSignal as createPendingSignal, latest, type SignalOptions } from 'solid-js'

/**
 * The recovered feature modules read a signal immediately after writing it in
 * the same command. Solid 2 otherwise exposes the last committed value until
 * its queue flushes. Keep that command contract through latest(), while using
 * the same Solid 2 signals and dependency tracking as the rest of the app.
 */
export const createSignal = ((value: unknown, options?: SignalOptions<unknown>) => {
  const [read, write] = createPendingSignal(value, options)
  return [Object.assign(() => latest(read), read), write]
}) as typeof createPendingSignal
