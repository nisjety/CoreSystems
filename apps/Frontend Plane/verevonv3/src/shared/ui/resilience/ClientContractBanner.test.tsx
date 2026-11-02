// @vitest-environment jsdom

import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// WS07-S3: the banner reads the module's `clientContractMismatch` signal, which only
// `checkClientContract` writes, so the tests drive it through a mocked gateway answer.
vi.mock('@/shared/build/build-info', () => ({
  BUILD_ID: 'abc123def456',
  isDevBuild: () => false,
}))

const answer = (minContract: number, policy = 'banner') =>
  new Response(
    JSON.stringify({
      data: { contract: 1, min_contract: minContract, policy, revision: 'abc', build_date: 'x' },
    }),
    { status: 200, headers: { 'content-type': 'application/json' } },
  )

async function mountBanner() {
  vi.resetModules()
  const contract = await import('@/shared/build/client-contract')
  const { ClientContractBanner } = await import('./ClientContractBanner')
  const view = render(() => <ClientContractBanner />)
  return { contract, view }
}

let fetchMock: ReturnType<typeof vi.fn>

beforeEach(() => {
  fetchMock = vi.fn()
  vi.stubGlobal('fetch', fetchMock)
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('ClientContractBanner', () => {
  it('renders nothing while there is no contract mismatch', async () => {
    await mountBanner()
    expect(screen.queryByRole('status')).toBeNull()
    expect(screen.queryByTestId('client-contract-banner')).toBeNull()
    expect(screen.queryByRole('button', { name: /Last på nytt|Reload/ })).toBeNull()
  })

  it('stays hidden after a check that finds the bundle current', async () => {
    fetchMock.mockResolvedValue(answer(1))
    const { contract } = await mountBanner()
    await contract.checkClientContract({ force: true })
    expect(screen.queryByRole('status')).toBeNull()
  })

  it('shows a polite status region with a Reload button for a stale bundle', async () => {
    fetchMock.mockResolvedValue(answer(99))
    const { contract } = await mountBanner()
    await contract.checkClientContract({ force: true })
    const status = await screen.findByRole('status')
    expect(status.getAttribute('aria-live')).toBe('polite')
    expect(status.getAttribute('data-testid')).toBe('client-contract-banner')
    expect(status.textContent).toMatch(/utdatert|out of date/)
    const button = await screen.findByRole('button', { name: /Last på nytt|Reload/ })
    expect(button.closest('[role="status"]')).toBe(status)
  })

  it('does not reload the page by itself', async () => {
    const reload = vi.fn()
    const original = window.location
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...original, reload },
    })
    try {
      fetchMock.mockResolvedValue(answer(99))
      const { contract } = await mountBanner()
      await contract.checkClientContract({ force: true })
      await screen.findByRole('status')
      expect(reload).not.toHaveBeenCalled()
      const button = await screen.findByRole('button', { name: /Last på nytt|Reload/ })
      button.click()
      await waitFor(() => expect(reload).toHaveBeenCalledTimes(1))
    } finally {
      Object.defineProperty(window, 'location', { configurable: true, value: original })
    }
  })

  it('is hidden again once the policy is switched off', async () => {
    fetchMock.mockResolvedValueOnce(answer(99))
    const { contract } = await mountBanner()
    await contract.checkClientContract({ force: true })
    await screen.findByRole('status')
    fetchMock.mockResolvedValueOnce(answer(99, 'off'))
    await contract.checkClientContract({ force: true })
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull())
  })
})
