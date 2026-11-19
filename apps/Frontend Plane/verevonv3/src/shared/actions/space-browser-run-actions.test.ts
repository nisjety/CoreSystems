import { describe, expect, it } from 'vitest'

import { getActionDescriptor } from '@/shared/actions/action-registry'

// WS20-S5 and D4 (W3-DEC-BROWSER): the room browser run start. The gateway accepts a start with no thread and creates
// one (browser_runs.rs parse_start), so the contract must too: it used to require a thread, which kept a room-level
// "Browse with the agent" from being an action a person's client could name.

const ID = 'spaces.browser_run.start'

const descriptor = () => {
  const found = getActionDescriptor(ID)
  if (!found) throw new Error(`${ID} is not registered`)
  return found
}

const valid = {
  spaceRef: 'space-1',
  goal: 'Find the price of the lamp',
  allowedDomains: ['shop.example.test'],
  clientSendId: 'start-0000000000000001',
}

describe('spaces.browser_run.start (WS20-S5, D4)', () => {
  it('starts a run on a new thread when no thread is named', () => {
    const input = descriptor().inputSchema
    expect(input.safeParse(valid).success).toBe(true)
    expect(input.safeParse({ ...valid, threadId: undefined }).success).toBe(true)
  })

  it('still takes a named thread, and refuses a blank or oversized one', () => {
    const input = descriptor().inputSchema
    expect(input.safeParse({ ...valid, threadId: 'thr_1' }).success).toBe(true)
    expect(input.safeParse({ ...valid, threadId: '   ' }).success).toBe(false)
    expect(input.safeParse({ ...valid, threadId: 'x'.repeat(129) }).success).toBe(false)
  })

  it('keeps every other bound: the goal, one to eight domains, a send id, and no shared live view', () => {
    const input = descriptor().inputSchema
    expect(input.safeParse({ ...valid, goal: '  ' }).success).toBe(false)
    expect(input.safeParse({ ...valid, allowedDomains: [] }).success).toBe(false)
    expect(input.safeParse({ ...valid, allowedDomains: Array.from({ length: 9 }, (_, i) => `d${i}.example.test`) }).success).toBe(false)
    expect(input.safeParse({ ...valid, clientSendId: 'short' }).success).toBe(false)
    expect(input.safeParse({ ...valid, shareView: true }).success).toBe(false)
  })

  it('answers the thread it ran in, whether the caller named one or not', () => {
    const output = descriptor().outputSchema
    expect(
      output.safeParse({ run_id: 'run_1', thread_id: 'thr_new', plan_id: 'plan_1', view: { scope: 'owner' } }).success,
    ).toBe(true)
  })
})
