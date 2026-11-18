import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  byoGatewaySwitchOn,
  isThreadRefusalCode,
  sideFeatureThreadOf,
  sideFeaturesWithheldOnSpace,
  subscriptionRouteWithheldOnSpace,
} from './space-side-features'

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('isThreadRefusalCode', () => {
  it('recognises the typed refusals of a named thread, and no other code', () => {
    for (const code of [
      'scope_not_available_in_space',
      'side_chat_thread_retention_refused',
      'connection_ambiguous',
      'side_chat_model_required',
    ]) {
      expect(isThreadRefusalCode(code), code).toBe(true)
    }
    expect(isThreadRefusalCode('review_unavailable')).toBe(false)
    expect(isThreadRefusalCode('delegated_auth_unavailable')).toBe(false)
    expect(isThreadRefusalCode(null)).toBe(false)
    expect(isThreadRefusalCode(undefined)).toBe(false)
  })
})

describe('sideFeatureThreadOf', () => {
  it('names the open thread, a Space thread included, so Model Gateway decides it', () => {
    // A Space thread is named whatever the switch says: the gateway refuses it typed, and nothing falls to the platform.
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(sideFeatureThreadOf('thread-space-1', false)).toBe('thread-space-1')
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    expect(sideFeatureThreadOf('thread-space-1', false)).toBe('thread-space-1')
  })

  it('names no thread for a temporary chat or for a page with no thread', () => {
    expect(sideFeatureThreadOf('thread-1', true)).toBeUndefined()
    expect(sideFeatureThreadOf('', false)).toBeUndefined()
    expect(sideFeatureThreadOf(null, false)).toBeUndefined()
    expect(sideFeatureThreadOf(undefined, false)).toBeUndefined()
  })
})

describe('sideFeaturesWithheldOnSpace', () => {
  it('withholds the side features on a Space thread while the BYO gateway switch is the literal true', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(byoGatewaySwitchOn()).toBe(true)
    expect(sideFeaturesWithheldOnSpace(true)).toBe(true)
  })

  it('never withholds them on a thread that is not a Space thread', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(sideFeaturesWithheldOnSpace(false)).toBe(false)
    expect(sideFeaturesWithheldOnSpace(undefined)).toBe(false)
  })

  it('changes nothing while the switch is off, on a Space thread or not', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    expect(byoGatewaySwitchOn()).toBe(false)
    expect(sideFeaturesWithheldOnSpace(true)).toBe(false)
    expect(sideFeaturesWithheldOnSpace(false)).toBe(false)
  })

  it('reads the switch as the strict literal: every other spelling is off', () => {
    for (const value of ['True', 'TRUE', '1', 'yes', ' true']) {
      vi.stubEnv('VITE_BYO_GATEWAY', value)
      expect(sideFeaturesWithheldOnSpace(true), value).toBe(false)
    }
  })
})

describe('subscriptionRouteWithheldOnSpace', () => {
  it('withholds the ChatGPT subscription route on a Space thread while the BYO gateway switch is the literal true', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(subscriptionRouteWithheldOnSpace(true)).toBe(true)
  })

  it('never withholds it on a thread that is not a Space thread', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(subscriptionRouteWithheldOnSpace(false)).toBe(false)
    expect(subscriptionRouteWithheldOnSpace(undefined)).toBe(false)
  })

  it('changes nothing while the switch is off, on a Space thread or not', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    expect(subscriptionRouteWithheldOnSpace(true)).toBe(false)
    expect(subscriptionRouteWithheldOnSpace(false)).toBe(false)
  })

  it('reads the switch as the strict literal: every other spelling is off', () => {
    for (const value of ['True', 'TRUE', '1', 'yes', ' true']) {
      vi.stubEnv('VITE_BYO_GATEWAY', value)
      expect(subscriptionRouteWithheldOnSpace(true), value).toBe(false)
    }
  })
})
