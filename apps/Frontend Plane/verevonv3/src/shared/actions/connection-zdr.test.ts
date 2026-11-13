import { describe, expect, it } from 'vitest'
import { connectionZdrAcknowledgeOutput } from './connection-zdr'

// ZDR Z-19d: Integration Core names an active row whose text is below the minimum `superseded`. The browser's output
// contract accepts that state with the other two, and refuses any state the ledger does not name.

function summary(state: string) {
  return {
    id: 'zdrack_1',
    state,
    digest: 'a'.repeat(64),
    textVersion: 'v2',
    locale: 'no',
    acknowledgedVia: 'connect',
    acknowledgedAt: '2026-11-01T10:00:00Z',
  }
}

describe('connection_zdr acknowledge output: the acknowledgement state', () => {
  it.each(['active', 'superseded', 'revoked'])('accepts the %s state', (state) => {
    expect(connectionZdrAcknowledgeOutput.safeParse({ created: true, acknowledgement: summary(state) }).success).toBe(true)
  })

  it('refuses a state the ledger does not name', () => {
    expect(connectionZdrAcknowledgeOutput.safeParse({ created: true, acknowledgement: summary('expired') }).success).toBe(false)
  })
})
