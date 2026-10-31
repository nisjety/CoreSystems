// @vitest-environment jsdom
import { render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/browser-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/browser-client')>()),
  listBrowserEgressReceipts: vi.fn(),
  listBrowserReceipts: vi.fn(),
  compileBrowserProcedure: vi.fn(),
}))
vi.mock('@/shared/api/auth-client', () => ({
  getSessionContext: vi.fn(async () => ({ userId: 'user-1', orgs: [{ id: 'org-1', name: 'Org', role: 'owner' }] })),
}))

import {
  listBrowserEgressReceipts,
  listBrowserReceipts,
  compileBrowserProcedure,
  type BrowserEgressReceiptsResponse,
} from '@/shared/api/browser-client'
import { EgressReceiptsPanel } from './EgressReceiptsPanel'

const egressPage: BrowserEgressReceiptsResponse = {
  receipts: [
    { sequence: 1, tabId: 'tab-1', method: 'GET', url: 'https://example.com/', decision: 'allow', policy: 'open_web', timestampMs: 1_700_000_000_000 },
    { sequence: 2, tabId: 'tab-1', method: 'GET', url: 'http://10.0.0.5/admin', decision: 'block', policy: 'private_host', timestampMs: 1_700_000_001_000 },
  ],
  counts: { allow: 1, block: 1 },
  nextSequence: 2,
  zdr: false,
}

beforeEach(() => {
  vi.mocked(listBrowserEgressReceipts).mockReset()
  vi.mocked(listBrowserReceipts).mockReset()
  vi.mocked(compileBrowserProcedure).mockReset()
})

afterEach(() => vi.restoreAllMocks())

describe('EgressReceiptsPanel', () => {
  it('lists the redacted egress rows with their decisions', async () => {
    vi.mocked(listBrowserEgressReceipts).mockResolvedValue(egressPage)
    render(() => <EgressReceiptsPanel sessionId="session-1" zdr={false} />)

    expect(await screen.findByText('https://example.com/')).toBeTruthy()
    expect(screen.getByText('http://10.0.0.5/admin')).toBeTruthy()
    expect(document.querySelector('li[data-decision="block"]')).toBeTruthy()
    expect(document.querySelector('li[data-decision="allow"]')).toBeTruthy()
    expect(listBrowserEgressReceipts).toHaveBeenCalledWith(
      'org-1',
      'session-1',
      expect.objectContaining({ limit: 200 }),
      expect.any(AbortSignal),
    )
  })

  it('for a ZDR session reads the live egress only, shows the not-retained chip, and requests nothing else', async () => {
    vi.mocked(listBrowserEgressReceipts).mockResolvedValue({ ...egressPage, zdr: true })
    render(() => <EgressReceiptsPanel sessionId="session-1" zdr={true} />)

    await waitFor(() => expect(listBrowserEgressReceipts).toHaveBeenCalled())
    expect(document.querySelector('.browser-evidence-chip--zdr')).toBeTruthy()
    expect(listBrowserReceipts).not.toHaveBeenCalled()
    expect(compileBrowserProcedure).not.toHaveBeenCalled()
  })

  it('shows the empty state when there are no rows', async () => {
    vi.mocked(listBrowserEgressReceipts).mockResolvedValue({ receipts: [], counts: { allow: 0, block: 0 }, nextSequence: 0, zdr: false })
    render(() => <EgressReceiptsPanel sessionId="session-1" zdr={false} />)

    await waitFor(() => expect(listBrowserEgressReceipts).toHaveBeenCalled())
    expect(await screen.findByText(/Ingen utgående|No outbound/)).toBeTruthy()
  })
})
