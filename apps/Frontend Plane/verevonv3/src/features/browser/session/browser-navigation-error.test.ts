import { describe, expect, it } from 'vitest'
import { ApiError } from '@/shared/api/http'
import { navigationFailureFromError, retryUrlOf } from './browser-navigation-error'

const tr = (_no: string, en: string) => en

describe('navigationFailureFromError (WS9.B)', () => {
  it('is null for actions that are not navigations, so the inline form error stays', () => {
    expect(navigationFailureFromError({ type: 'back' }, new ApiError('x', 500, 'driver_failed'), tr)).toBeNull()
    expect(navigationFailureFromError({ type: 'reload', ignore_cache: true }, new Error('x'), tr)).toBeNull()
  })

  it('keeps the URL the person asked for, and retry loads that URL', () => {
    const failure = navigationFailureFromError(
      { type: 'navigate', url: 'https://example.com/a' },
      new ApiError('boom', 502, 'driver_failed'),
      tr,
    )
    expect(failure?.url).toBe('https://example.com/a')
    expect(retryUrlOf(failure)).toBe('https://example.com/a')
    expect(failure?.reason).toBe('unreachable')
  })

  it('classifies an invalid URL and a blocked page', () => {
    const invalid = navigationFailureFromError(
      { type: 'navigate', url: 'https://example.com' },
      new ApiError('bad', 400, 'invalid_url'),
      tr,
    )
    expect(invalid?.reason).toBe('invalid_url')
    const blocked = navigationFailureFromError(
      { type: 'navigate', url: 'https://example.com' },
      new ApiError('bad', 409, 'challenge_detected', { blockReason: 'waf_interstitial' }),
      tr,
    )
    expect(blocked?.reason).toBe('blocked')
  })

  it('classifies a navigation outside the run domains by its typed code, not its message', () => {
    const refused = navigationFailureFromError(
      { type: 'navigate', url: 'https://other.example/a' },
      new ApiError('any wording at all', 403, 'browser_navigation_outside_scope'),
      tr,
    )
    expect(refused?.reason).toBe('outside_scope')
    expect(refused?.url).toBe('https://other.example/a')
    const reworded = navigationFailureFromError(
      { type: 'navigate', url: 'https://other.example/a' },
      new ApiError('outside the run\'s allowed domains', 403, 'browser_session_failed'),
      tr,
    )
    expect(reworded?.reason).toBe('failed')
  })

  it('never shows the raw server message or an unknown code', () => {
    const failure = navigationFailureFromError(
      { type: 'navigate', url: 'https://example.com' },
      new ApiError('internal stack detail 10.0.0.5', 500, 'unknown_code_xyz'),
      tr,
    )
    expect(failure?.sentence).not.toContain('internal stack detail')
    expect(failure?.sentence).not.toContain('unknown_code_xyz')
    expect(failure?.sentence.length).toBeGreaterThan(0)
  })

  it('returns no retry URL when there is no failure', () => {
    expect(retryUrlOf(null)).toBeNull()
    expect(retryUrlOf(undefined)).toBeNull()
  })
})
