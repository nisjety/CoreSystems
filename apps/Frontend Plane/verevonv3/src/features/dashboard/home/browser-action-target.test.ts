import { describe, expect, it } from 'vitest'
import { browserActionTarget } from './browser-action-target'
import type { BrowserTraceEvent } from '@/shared/api/browser-client'

const now = Date.parse('2026-11-04T12:00:00Z')
const event = (rect: unknown, ts = new Date(now).toISOString()): BrowserTraceEvent => ({
  event_id: 'event', type: 'action_started', ts, seq: 1, payload: { rect }, idempotency_key: 'event',
})

describe('browserActionTarget', () => {
  it('renders a valid recent target from the current subscription', () => {
    const rect = { x: 25, y: 12, width: 100, height: 32 }
    expect(browserActionTarget(event(rect), now - 500, now)).toEqual(rect)
  })

  it('ignores missing, invalid, and replayed targets', () => {
    expect(browserActionTarget(event(null), now, now)).toBeNull()
    expect(browserActionTarget(event({ x: 0, y: 0, width: -1, height: 3 }), now, now)).toBeNull()
    expect(browserActionTarget(event({ x: Infinity, y: 0, width: 1, height: 3 }), now, now)).toBeNull()
    expect(browserActionTarget(event({ x: 0, y: 0, width: 1, height: 3 }, new Date(now - 5000).toISOString()), now, now)).toBeNull()
  })
})
