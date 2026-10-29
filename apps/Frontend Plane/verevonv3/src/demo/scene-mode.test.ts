import { describe, expect, it } from 'vitest'
import { shouldEnableSceneMode } from './scene-mode'

describe('scene mode gate', () => {
  it('enables fixtures only for a scene route in development', () => {
    expect(shouldEnableSceneMode('1', true, '/scene/02-plan')).toBe(true)
    expect(shouldEnableSceneMode('1', true, '/scene/03-undersok')).toBe(true)
  })

  it('never installs fictional fixtures on ordinary routes or production builds', () => {
    expect(shouldEnableSceneMode('1', true, '/dashboard')).toBe(false)
    expect(shouldEnableSceneMode('1', true, '/chat')).toBe(false)
    expect(shouldEnableSceneMode('1', false, '/scene/02-plan')).toBe(false)
    expect(shouldEnableSceneMode(undefined, true, '/scene/02-plan')).toBe(false)
  })
})
