import { describe, expect, it, vi } from 'vitest'

import { notifySpacesListChanged, onSpacesListChanged } from './space-list-changed'

describe('space list change signal (DEPTH-WS04-S2-1)', () => {
  it('tells every subscribed listing once per change', () => {
    const sidebar = vi.fn()
    const index = vi.fn()
    const stopSidebar = onSpacesListChanged(sidebar)
    const stopIndex = onSpacesListChanged(index)
    notifySpacesListChanged()
    expect(sidebar).toHaveBeenCalledTimes(1)
    expect(index).toHaveBeenCalledTimes(1)
    stopSidebar()
    stopIndex()
  })

  it('stops telling a listing once it has unsubscribed', () => {
    const listener = vi.fn()
    const stop = onSpacesListChanged(listener)
    stop()
    notifySpacesListChanged()
    expect(listener).not.toHaveBeenCalled()
  })

  it('keeps the other listings refreshing when one of them throws', () => {
    const broken = vi.fn(() => {
      throw new Error('listing unmounted')
    })
    const healthy = vi.fn()
    const stopBroken = onSpacesListChanged(broken)
    const stopHealthy = onSpacesListChanged(healthy)
    expect(() => notifySpacesListChanged()).not.toThrow()
    expect(broken).toHaveBeenCalledTimes(1)
    expect(healthy).toHaveBeenCalledTimes(1)
    stopBroken()
    stopHealthy()
  })
})
