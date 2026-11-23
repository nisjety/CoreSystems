import { describe, expect, it } from 'vitest'

import { RUN_STOP_TIMEOUT_MS, runStopCanBePressed, runStopStateForCode } from './space-run-stop'

// WS09-S8: the Stop control's states on a live run card. Pure rules, so they are read without a DOM.

describe('runStopCanBePressed (WS09-S8)', () => {
  it('is open before any attempt, and after a timeout or a failure, so a retry can reuse the key', () => {
    expect(runStopCanBePressed('idle')).toBe(true)
    expect(runStopCanBePressed('timeout')).toBe(true)
    expect(runStopCanBePressed('failed')).toBe(true)
  })

  it('is closed while an attempt is in flight or has been sent', () => {
    expect(runStopCanBePressed('pending')).toBe(false)
    expect(runStopCanBePressed('sent')).toBe(false)
  })

  it('stays closed after a refusal that a retry cannot change', () => {
    expect(runStopCanBePressed('forbidden')).toBe(false)
    expect(runStopCanBePressed('zdr')).toBe(false)
  })
})

describe('runStopStateForCode (WS09-S8)', () => {
  it('maps the two refusals a retry cannot fix to their own states', () => {
    expect(runStopStateForCode('space_run_control_not_authorized')).toBe('forbidden')
    expect(runStopStateForCode('zdr_durable_mutation_forbidden')).toBe('zdr')
  })

  it('reads any other answer as a failure that can be retried', () => {
    expect(runStopStateForCode('run_control_unavailable')).toBe('failed')
    expect(runStopStateForCode('')).toBe('failed')
  })
})

describe('RUN_STOP_TIMEOUT_MS (WS09-S8)', () => {
  it('waits long enough for a slow answer, and no longer than half a minute', () => {
    expect(RUN_STOP_TIMEOUT_MS).toBeGreaterThanOrEqual(10_000)
    expect(RUN_STOP_TIMEOUT_MS).toBeLessThanOrEqual(30_000)
  })
})
