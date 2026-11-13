import { describe, expect, it } from 'vitest'
import { browserTerminalFailureCopy } from './browser-terminal-copy'
import { applyDurableRunEvent, emptyChatRunWatch } from './chat-run-watch'

// GAP-W3-EP-1-S5-timeout-copy: the one place the SPA words a browser run's terminal failure code. Every known
// code has Norwegian and English copy; an unknown or non-slug code is never shown. The chat run watch keeps
// only the watched run's closed code, never text from the server.

const no = (noText: string) => noText
const en = (_noText: string, enText: string) => enText

const KNOWN = [
  'browser_continue_tab_changed',
  'browser_continue_origin_changed',
  'browser_permission_denied',
  'browser_agent_permission_denied',
  'browser_timed_out',
  'browser_agent_timed_out',
  'browser_resource_exhausted',
  'browser_agent_resource_exhausted',
  'browser_help_timed_out',
] as const

describe('browserTerminalFailureCopy', () => {
  it.each(KNOWN)('words %s in both languages, differently', (code) => {
    const norwegian = browserTerminalFailureCopy(code, no)
    const english = browserTerminalFailureCopy(code, en)
    expect(norwegian).toBeTruthy()
    expect(english).toBeTruthy()
    expect(norwegian).not.toBe(english)
    expect(norwegian).not.toContain(code)
    expect(english).not.toContain(code)
  })

  it('tells the person the takeover was not taken in time and how to start again', () => {
    expect(browserTerminalFailureCopy('browser_help_timed_out', en)).toBe(
      'Nobody took over the browser in time, so the run ended. Start the task again when you are ready to help.',
    )
    expect(browserTerminalFailureCopy('browser_help_timed_out', no)).toMatch(/^Ingen tok over nettleseren i tide/)
  })

  it.each([
    ['an unknown slug', 'browser_quota_exceeded'],
    ['an upper-case spelling', 'BROWSER_HELP_TIMED_OUT'],
    ['server text', 'Upstream said: secret internal text'],
    ['an empty string', ''],
    ['a number', 7],
    ['null', null],
    ['undefined', undefined],
    ['an object', { code: 'browser_help_timed_out' }],
  ])('returns null for %s', (_label, code) => {
    expect(browserTerminalFailureCopy(code, en)).toBeNull()
    expect(browserTerminalFailureCopy(code, no)).toBeNull()
  })
})

describe('applyDurableRunEvent on a run failure', () => {
  const failed = (payload: Record<string, unknown>) => ({
    eventId: 'evt_9',
    eventType: 'EVENT_TYPE_RUN_FAILED',
    at: '2026-11-12T10:00:00.000Z',
    payload,
  })

  it('keeps the closed code of the watched run and ends the watch', () => {
    const state = applyDurableRunEvent(
      emptyChatRunWatch('run_abc123', false),
      failed({ run_id: 'run_abc123', terminal: true, status: 'failed', failure_code: 'browser_help_timed_out' }),
    )
    expect(state.live).toBe(false)
    expect(state.terminalFailure).toBe('browser_help_timed_out')
  })

  it('leaves the state unchanged for a failure of another run', () => {
    const before = emptyChatRunWatch('run_abc123', false)
    const after = applyDurableRunEvent(
      before,
      failed({ run_id: 'run_other', terminal: true, status: 'failed', failure_code: 'browser_help_timed_out' }),
    )
    expect(after).toBe(before)
  })

  it('leaves the state unchanged for a failure that names no run, or is not terminal', () => {
    const before = emptyChatRunWatch('run_abc123', false)
    expect(applyDurableRunEvent(before, failed({ terminal: true, failure_code: 'browser_timed_out' }))).toBe(before)
    expect(
      applyDurableRunEvent(before, failed({ run_id: 'run_abc123', terminal: false, failure_code: 'browser_timed_out' })),
    ).toBe(before)
  })

  it('keeps no failure text that is not a closed slug', () => {
    const state = applyDurableRunEvent(
      emptyChatRunWatch('run_abc123', false),
      failed({ run_id: 'run_abc123', terminal: true, failure_code: 'Secret upstream text', error: 'details' }),
    )
    expect(state.live).toBe(false)
    expect(state.terminalFailure).toBeNull()
    expect(JSON.stringify(state)).not.toContain('Secret upstream text')
    expect(JSON.stringify(state)).not.toContain('details')
  })
})
