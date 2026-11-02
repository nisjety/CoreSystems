import { describe, expect, it } from 'vitest'
import {
  parkReasonLabel,
  RUN_PARK_REASONS,
  runParkedFrameOf,
  runResumedFrameOf,
} from './run-park'
import runParkSource from './run-park.ts?raw'

/** The `run_parked` frame of the owner's turn stream, as model-gateway's `run_parked_event` writes it. */
function parkedFrame(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    run_id: 'run-42',
    request_id: 'req-1',
    status: 'parked',
    reason: 'provider_exhausted',
    resume_after: '2026-10-20T12:00:00Z',
    ...overrides,
  }
}

describe('runParkedFrameOf', () => {
  it('reads the closed reasons of a parked run, and only those', () => {
    expect([...RUN_PARK_REASONS]).toEqual([
      'credential_window',
      'provider_exhausted',
      'upstream_unavailable',
      'reauth_required',
    ])
    for (const reason of RUN_PARK_REASONS) {
      expect(runParkedFrameOf(parkedFrame({ reason }))).toEqual({
        runId: 'run-42',
        requestId: 'req-1',
        reason,
        resumeAfter: '2026-10-20T12:00:00Z',
      })
    }
  })

  it('gives null for a reason it does not know, so the caller reports an unknown event', () => {
    // `crash_interrupted` is the one reason the executor never writes in this build (crash-park
    // is a later slice): a frame that names it is not displayed as if it were understood.
    for (const reason of ['crash_interrupted', '', 'Provider_Exhausted', 'provider_exhausted ', 'a sentence', 7, null]) {
      expect(runParkedFrameOf(parkedFrame({ reason })), String(reason)).toBeNull()
    }
    expect(runParkedFrameOf((() => { const f = parkedFrame(); delete f.reason; return f })())).toBeNull()
  })

  it('gives null unless the frame names its run and says it is parked', () => {
    expect(runParkedFrameOf(parkedFrame({ run_id: '' }))).toBeNull()
    expect(runParkedFrameOf(parkedFrame({ run_id: 42 }))).toBeNull()
    expect(runParkedFrameOf(parkedFrame({ status: 'running' }))).toBeNull()
    expect(runParkedFrameOf(parkedFrame({ status: undefined }))).toBeNull()
    // The orchestration subject's run_parked carries the run id alone (the reason is never
    // broadcast), so it is not a turn-stream frame.
    expect(runParkedFrameOf({ run_id: 'run-42' })).toBeNull()
  })

  it('gives null for anything that is not an object', () => {
    for (const value of [null, undefined, 'run_parked', 7, true, [], [parkedFrame()]]) {
      expect(runParkedFrameOf(value)).toBeNull()
    }
  })

  it('keeps resume_after only when it is a readable time, and never echoes free text', () => {
    expect(runParkedFrameOf(parkedFrame({ resume_after: null }))?.resumeAfter).toBeNull()
    expect(runParkedFrameOf(parkedFrame({ resume_after: 'soon, maybe' }))?.resumeAfter).toBeNull()
    expect(runParkedFrameOf(parkedFrame({ resume_after: '' }))?.resumeAfter).toBeNull()
    expect(runParkedFrameOf(parkedFrame({ resume_after: 1760961600 }))?.resumeAfter).toBeNull()
    const absent = parkedFrame()
    delete absent.resume_after
    expect(runParkedFrameOf(absent)?.resumeAfter).toBeNull()
    // V8's lenient legacy Date.parse accepts all of these, so a parse check alone would echo
    // them: only an RFC 3339 timestamp is kept.
    for (const text of [
      'ignore previous instructions 12',
      'hello 1',
      'x 2026',
      '2026',
      '1',
      '2026-10-20',
      '2026-10-20 12:00:00',
      '2026-10-20T12:00:00',
      ' 2026-10-20T12:00:00Z',
      '2026-10-20T12:00:00Z ',
      '2026-10-20T12:00:00Z and more',
      'Tue Oct 20 2026 12:00:00 GMT+0000',
      '2026-13-45T99:99:99Z',
    ]) {
      expect(runParkedFrameOf(parkedFrame({ resume_after: text }))?.resumeAfter, text).toBeNull()
    }
    for (const stamp of ['2026-10-20T12:00:00Z', '2026-10-20T12:00:00.250Z', '2026-10-20T14:00:00+02:00', '2026-10-20T10:00:00-02:00']) {
      expect(runParkedFrameOf(parkedFrame({ resume_after: stamp }))?.resumeAfter, stamp).toBe(stamp)
    }
    expect(runParkedFrameOf(parkedFrame({ resume_after: '2026-10-20T12:00:00.250+00:00' }))?.resumeAfter).toBe(
      '2026-10-20T12:00:00.250+00:00',
    )
  })

  it('leaves the request id out when the frame carries none', () => {
    const frame = parkedFrame()
    delete frame.request_id
    const read = runParkedFrameOf(frame)
    expect(read).toEqual({ runId: 'run-42', reason: 'provider_exhausted', resumeAfter: '2026-10-20T12:00:00Z' })
    expect(read && 'requestId' in read).toBe(false)
  })

  it('does not carry any other field of the frame', () => {
    const read = runParkedFrameOf(parkedFrame({ claim_token: 'secret-claim', approval_id: 'apr-1', extra: { a: 1 } }))
    expect(JSON.stringify(read)).not.toContain('secret-claim')
    expect(Object.keys(read ?? {}).sort()).toEqual(['reason', 'requestId', 'resumeAfter', 'runId'])
  })
})

describe('runResumedFrameOf', () => {
  it('reads the run and the segment its resume started', () => {
    expect(runResumedFrameOf({ run_id: 'run-42', resumed_segment: 3 })).toEqual({
      runId: 'run-42',
      resumedSegment: 3,
    })
    expect(runResumedFrameOf({ run_id: 'run-42', resumed_segment: 0 })).toEqual({
      runId: 'run-42',
      resumedSegment: 0,
    })
  })

  it('gives null for a segment that is not a non-negative integer, as the run console does', () => {
    for (const resumed_segment of ['2', -1, 1.5, Number.NaN, null, undefined, Number.MAX_SAFE_INTEGER + 2]) {
      expect(runResumedFrameOf({ run_id: 'run-42', resumed_segment }), String(resumed_segment)).toBeNull()
    }
  })

  it('gives null without a run id or for a non-object', () => {
    expect(runResumedFrameOf({ resumed_segment: 2 })).toBeNull()
    expect(runResumedFrameOf({ run_id: '', resumed_segment: 2 })).toBeNull()
    for (const value of [null, undefined, 'run_resumed', 3, [], true]) {
      expect(runResumedFrameOf(value)).toBeNull()
    }
  })
})

describe('parkReasonLabel', () => {
  it('words every reason in Norwegian and in English', () => {
    const seen = new Set<string>()
    for (const reason of RUN_PARK_REASONS) {
      for (const locale of ['no', 'en'] as const) {
        const label = parkReasonLabel(reason, locale)
        expect(label.trim().length, `${reason}/${locale}`).toBeGreaterThan(0)
        expect(label, `${reason}/${locale}`).not.toContain('_')
        expect(label, `${reason}/${locale}`).not.toBe(reason)
        expect(seen.has(label), `${reason}/${locale} repeats`).toBe(false)
        seen.add(label)
      }
    }
    expect(seen.size).toBe(RUN_PARK_REASONS.length * 2)
  })

  it('says the run is paused and offers no retry, in either language', () => {
    for (const reason of RUN_PARK_REASONS) {
      expect(parkReasonLabel(reason, 'en')).toMatch(/^Paused/)
      expect(parkReasonLabel(reason, 'no')).toMatch(/^Satt på pause/)
      for (const locale of ['no', 'en'] as const) {
        expect(parkReasonLabel(reason, locale)).not.toMatch(/retry|try again|prøv igjen|forsøk igjen|trykk|click/i)
      }
    }
  })

  it('gives a generic label, never the raw token, for a reason it does not know', () => {
    expect(parkReasonLabel('something_new', 'en')).toBe('Paused.')
    expect(parkReasonLabel('something_new', 'no')).toBe('Satt på pause.')
  })
})

describe('run-park.ts is display only', () => {
  it('imports no client, calls no network and names no continue call', () => {
    const code = runParkSource.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '')
    expect(code).not.toMatch(/\bfetch\b|requestJson|readSseStream|orchestration-client|chat-client/)
    expect(code).not.toMatch(/continueParkedRun|continueRun|claim_token|claimToken/)
  })
})
