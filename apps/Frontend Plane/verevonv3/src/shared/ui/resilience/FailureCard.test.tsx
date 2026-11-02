// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { checkClientVersion } = vi.hoisted(() => ({ checkClientVersion: vi.fn((_options?: { force?: boolean }) => Promise.resolve()) }))
vi.mock('@/shared/build/version-check', () => ({ checkClientVersion }))

import { FailureCard } from './FailureCard'

function apiError(status: number): Error {
  const error = new Error('secret message with /spaces/abc?token=zzz')
  error.name = 'ApiError'
  Object.assign(error, { status, code: null })
  return error
}

let writeText: ReturnType<typeof vi.fn>

beforeEach(() => {
  checkClientVersion.mockClear()
  writeText = vi.fn(() => Promise.resolve())
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
})

afterEach(() => {
  cleanup()
})

describe('FailureCard', () => {
  it('is an alert that names the class and region', () => {
    render(() => <FailureCard error={new Error('boom')} region="outlet" onRetry={() => undefined} />)
    const alert = screen.getByRole('alert')
    expect(alert.getAttribute('data-failure-class')).toBe('render_error')
    expect(alert.getAttribute('data-failure-region')).toBe('outlet')
  })

  it('shows Reload and Retry for a render error, and no Sign in', () => {
    const onRetry = vi.fn()
    render(() => <FailureCard error={new Error('boom')} region="panel" onRetry={onRetry} />)
    expect(screen.queryByRole('button', { name: /reload|last inn/i })).not.toBeNull()
    const retry = screen.getByRole('button', { name: /retry|prøv igjen/i })
    fireEvent.click(retry)
    expect(onRetry).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('button', { name: /sign in|logg inn/i })).toBeNull()
  })

  it('hides Retry for stale_deploy and asks for a fresh version check', async () => {
    render(() => <FailureCard error={new TypeError('Failed to fetch dynamically imported module: /a.js')} region="outlet" onRetry={() => undefined} />)
    expect(screen.queryByRole('button', { name: /retry|prøv igjen/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /reload|last inn/i })).not.toBeNull()
    await waitFor(() => expect(checkClientVersion).toHaveBeenCalledWith({ force: true }))
  })

  it('shows Sign in only for session_expired', () => {
    render(() => <FailureCard error={apiError(401)} region="root" onRetry={() => undefined} />)
    expect(screen.queryByRole('button', { name: /sign in|logg inn/i })).not.toBeNull()
  })

  it('does not check the version for a non-stale failure', () => {
    render(() => <FailureCard error={new Error('boom')} region="outlet" />)
    expect(checkClientVersion).not.toHaveBeenCalled()
  })

  it('Copy details puts only class, build_id, region, at and route_class on the clipboard', async () => {
    window.history.replaceState({}, '', '/spaces/abc-123?token=zzz')
    render(() => <FailureCard error={apiError(500)} region="space-tab" onRetry={() => undefined} />)
    fireEvent.click(screen.getByRole('button', { name: /copy details|kopier detaljer/i }))
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1))
    const payload = writeText.mock.calls[0]![0] as string
    const parsed = JSON.parse(payload) as Record<string, unknown>
    expect(Object.keys(parsed).sort()).toEqual(['at', 'build_id', 'class', 'region', 'route_class'])
    expect(parsed.class).toBe('render_error')
    expect(parsed.region).toBe('space-tab')
    expect(parsed.route_class).toBe('spaces')
    expect(payload).not.toContain('secret message')
    expect(payload).not.toContain('token')
    expect(payload).not.toContain('abc-123')
  })

  it('swallows a blocked clipboard', async () => {
    writeText.mockRejectedValueOnce(new Error('denied'))
    render(() => <FailureCard error={new Error('boom')} region="outlet" />)
    fireEvent.click(screen.getByRole('button', { name: /copy details|kopier detaljer/i }))
    await waitFor(() => expect(writeText).toHaveBeenCalled())
    expect(screen.queryByRole('alert')).not.toBeNull()
  })

  it('never renders the error message', () => {
    render(() => <FailureCard error={new Error('super secret detail')} region="outlet" />)
    expect(screen.getByRole('alert').textContent).not.toContain('super secret detail')
  })
})
