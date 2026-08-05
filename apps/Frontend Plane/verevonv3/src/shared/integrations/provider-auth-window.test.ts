import { afterEach, describe, expect, it, vi } from 'vitest'
import { reserveDirectOauthWindow } from './provider-auth-window'

describe('reserveDirectOauthWindow', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('opens a blank popup synchronously so the later OAuth navigation keeps user activation', () => {
    const popup = {} as Window
    const open = vi.spyOn(window, 'open').mockReturnValue(popup)

    expect(reserveDirectOauthWindow()).toBe(popup)
    expect(open).toHaveBeenCalledTimes(1)
    expect(open.mock.calls[0]?.[0]).toBe('about:blank')
    expect(open.mock.calls[0]?.[1]).toBe('_blank')
  })

  it('returns null when the browser blocks a popup', () => {
    vi.spyOn(window, 'open').mockReturnValue(null)

    expect(reserveDirectOauthWindow()).toBeNull()
  })
})
