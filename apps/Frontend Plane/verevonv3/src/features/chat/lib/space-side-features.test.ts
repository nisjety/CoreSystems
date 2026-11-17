import { afterEach, describe, expect, it, vi } from 'vitest'
import { byoGatewaySwitchOn, sideFeaturesWithheldOnSpace } from './space-side-features'

afterEach(() => {
  vi.unstubAllEnvs()
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
