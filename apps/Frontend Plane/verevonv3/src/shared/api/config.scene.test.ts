import { afterEach, describe, expect, it, vi } from 'vitest'
import { gatewayBaseUrl } from './config'

describe('gatewayBaseUrl in scene capture mode', () => {
  afterEach(() => vi.unstubAllEnvs())

  it('forces same-origin URLs even when an absolute gateway is configured', () => {
    vi.stubEnv('VITE_SCENE_MODE', '1')
    vi.stubEnv('VITE_VEREVON_GATEWAY_URL', 'https://gateway.example')
    expect(gatewayBaseUrl()).toBe('')
  })
})
