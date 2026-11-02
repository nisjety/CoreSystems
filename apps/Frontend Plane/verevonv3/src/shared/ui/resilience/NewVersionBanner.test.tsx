// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/build/build-info', () => ({
  BUILD_ID: 'build-aaaa',
  isDevBuild: () => false,
}))
vi.mock('@/shared/build/client-contract', () => ({ checkClientContract: vi.fn(() => Promise.resolve()) }))

let fetchMock: ReturnType<typeof vi.fn>

async function mount() {
  vi.resetModules()
  const versionCheck = await import('@/shared/build/version-check')
  const { NewVersionBanner } = await import('./NewVersionBanner')
  const view = render(() => <NewVersionBanner />)
  return { versionCheck, view }
}

beforeEach(() => {
  fetchMock = vi.fn()
  vi.stubGlobal('fetch', fetchMock)
  window.sessionStorage.clear()
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('NewVersionBanner', () => {
  it('renders nothing while no newer build is known', async () => {
    await mount()
    expect(screen.queryByRole('status')).toBeNull()
  })

  it('is a polite status with Reload and Dismiss when a newer build is live', async () => {
    const { versionCheck } = await mount()
    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify({ build_id: 'build-bbbb' }), { status: 200, headers: { 'content-type': 'application/json' } }),
    )
    await versionCheck.checkClientVersion({ force: true })
    const status = await screen.findByRole('status')
    expect(status.getAttribute('aria-live')).toBe('polite')
    expect(screen.queryByRole('button', { name: /reload|last på nytt/i })).not.toBeNull()
    expect(screen.queryByRole('button', { name: /dismiss|lukk/i })).not.toBeNull()
  })

  it('Dismiss hides the banner and remembers the build for the session', async () => {
    const { versionCheck } = await mount()
    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify({ build_id: 'build-bbbb' }), { status: 200, headers: { 'content-type': 'application/json' } }),
    )
    await versionCheck.checkClientVersion({ force: true })
    await screen.findByRole('status')
    fireEvent.click(screen.getByRole('button', { name: /dismiss|lukk/i }))
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull())
    expect(window.sessionStorage.getItem(versionCheck.DISMISSED_KEY)).toBe('build-bbbb')
  })

  it('Reload reloads the page and is the only thing that does', async () => {
    const reload = vi.fn()
    const original = window.location
    Object.defineProperty(window, 'location', { value: { ...original, reload }, configurable: true })
    try {
      const { versionCheck } = await mount()
      fetchMock.mockResolvedValueOnce(
        new Response(JSON.stringify({ build_id: 'build-bbbb' }), { status: 200, headers: { 'content-type': 'application/json' } }),
      )
      await versionCheck.checkClientVersion({ force: true })
      await screen.findByRole('status')
      expect(reload).not.toHaveBeenCalled()
      fireEvent.click(screen.getByRole('button', { name: /reload|last på nytt/i }))
      expect(reload).toHaveBeenCalledTimes(1)
    } finally {
      Object.defineProperty(window, 'location', { value: original, configurable: true })
    }
  })
})
