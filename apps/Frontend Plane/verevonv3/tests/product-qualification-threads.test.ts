import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { qualifyingThreads, latestQualifyingThreadPerScenario, QUALIFYING_STREAK_LENGTH } from './e2e/product-qualification-threads'

/**
 * `qualifyingThreads()` reads a real private capture folder via
 * `Q16_CAPTURE_ROOT`, so these tests point it at a synthetic, fully
 * throwaway fixture tree under the OS temp dir — never the real private
 * recordings folder, and nothing here is copied into the repo.
 */
let root: string | undefined
let previousEnv: string | undefined

afterEach(() => {
  if (previousEnv === undefined) delete process.env.Q16_CAPTURE_ROOT
  else process.env.Q16_CAPTURE_ROOT = previousEnv
  if (root) rmSync(root, { recursive: true, force: true })
  root = undefined
})

type Capture = { scenario: string; threadId: string; startedAt: string; build?: string; accepted?: boolean }

function writeCapture(captureRoot: string, take: string, capture: Capture) {
  const dir = join(captureRoot, take, 'run')
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, 'capture.json'), JSON.stringify({ accepted: true, ...capture }))
}

function useFixtureRoot(): string {
  previousEnv = process.env.Q16_CAPTURE_ROOT
  root = mkdtempSync(join(tmpdir(), 'q16-fixture-'))
  process.env.Q16_CAPTURE_ROOT = root
  return root
}

describe('qualifyingThreads: same-build streak enforcement', () => {
  it('rejects a streak that spans more than one build (a partial rebuild mid freeze)', () => {
    const captureRoot = useFixtureRoot()
    // 4 accepted takes on build A, then 1 more (the most recent) on build B:
    // the last 5 by startedAt are NOT a same-build streak.
    for (let i = 1; i <= 4; i++) {
      writeCapture(captureRoot, `take-0${i}`, {
        scenario: '02-salgsrapport', threadId: `THREAD-A-${i}`,
        startedAt: `2026-09-26T06:0${i}:00.000Z`, build: 'sha256:aaaa',
      })
    }
    writeCapture(captureRoot, 'take-05', {
      scenario: '02-salgsrapport', threadId: 'THREAD-B-5',
      startedAt: '2026-09-26T06:05:00.000Z', build: 'sha256:bbbb',
    })
    expect(() => qualifyingThreads()).toThrow(/02-salgsrapport/)
    expect(() => qualifyingThreads()).toThrow(/different builds/)
  })

  it('accepts a streak where every take shares one build', () => {
    const captureRoot = useFixtureRoot()
    for (let i = 1; i <= QUALIFYING_STREAK_LENGTH; i++) {
      writeCapture(captureRoot, `take-0${i}`, {
        scenario: '01-kundesvar', threadId: `THREAD-${i}`,
        startedAt: `2026-09-26T06:0${i}:00.000Z`, build: 'sha256:cccc',
      })
    }
    const threads = qualifyingThreads()
    expect(threads).toHaveLength(QUALIFYING_STREAK_LENGTH)
    expect(threads.every((t) => t.build === 'sha256:cccc')).toBe(true)
  })

  it('only checks the LAST N accepted takes: an old build outside the streak window does not fail it', () => {
    const captureRoot = useFixtureRoot()
    // An old take on a stale build, well before the qualifying window.
    writeCapture(captureRoot, 'take-00', {
      scenario: '03-kampanje', threadId: 'THREAD-OLD', startedAt: '2026-09-20T00:00:00.000Z', build: 'sha256:stale',
    })
    for (let i = 1; i <= QUALIFYING_STREAK_LENGTH; i++) {
      writeCapture(captureRoot, `take-0${i}`, {
        scenario: '03-kampanje', threadId: `THREAD-${i}`,
        startedAt: `2026-09-26T06:0${i}:00.000Z`, build: 'sha256:dddd',
      })
    }
    const threads = qualifyingThreads()
    expect(threads).toHaveLength(QUALIFYING_STREAK_LENGTH)
    expect(threads.some((t) => t.threadId === 'THREAD-OLD')).toBe(false)
  })

  it('rejected (accepted: false) takes never count toward or break the streak', () => {
    const captureRoot = useFixtureRoot()
    writeCapture(captureRoot, 'take-00', {
      scenario: '04-prosjektplan', threadId: 'THREAD-REJECTED', startedAt: '2026-09-26T06:00:00.000Z',
      build: 'sha256:eeee', accepted: false,
    })
    for (let i = 1; i <= QUALIFYING_STREAK_LENGTH; i++) {
      writeCapture(captureRoot, `take-0${i}`, {
        scenario: '04-prosjektplan', threadId: `THREAD-${i}`,
        startedAt: `2026-09-26T06:0${i}:00.000Z`, build: 'sha256:ffff',
      })
    }
    const threads = qualifyingThreads()
    expect(threads).toHaveLength(QUALIFYING_STREAK_LENGTH)
    expect(threads.some((t) => t.threadId === 'THREAD-REJECTED')).toBe(false)
  })

  it('propagates the same-build check to latestQualifyingThreadPerScenario (it builds on qualifyingThreads)', () => {
    const captureRoot = useFixtureRoot()
    for (let i = 1; i <= 4; i++) {
      writeCapture(captureRoot, `take-0${i}`, {
        scenario: '02-salgsrapport', threadId: `THREAD-A-${i}`,
        startedAt: `2026-09-26T06:0${i}:00.000Z`, build: 'sha256:aaaa',
      })
    }
    writeCapture(captureRoot, 'take-05', {
      scenario: '02-salgsrapport', threadId: 'THREAD-B-5',
      startedAt: '2026-09-26T06:05:00.000Z', build: 'sha256:bbbb',
    })
    expect(() => latestQualifyingThreadPerScenario()).toThrow(/different builds/)
  })

  it('returns [] (never throws) when the capture root itself is unreadable', () => {
    previousEnv = process.env.Q16_CAPTURE_ROOT
    process.env.Q16_CAPTURE_ROOT = join(tmpdir(), 'q16-fixture-does-not-exist-' + Math.random().toString(36).slice(2))
    expect(qualifyingThreads()).toEqual([])
  })
})
