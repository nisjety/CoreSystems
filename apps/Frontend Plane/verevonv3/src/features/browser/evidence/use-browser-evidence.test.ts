// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/browser-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/browser-client')>()),
  listBrowserEgressReceipts: vi.fn(),
  listBrowserReceipts: vi.fn(),
  compileBrowserProcedure: vi.fn(),
}))
vi.mock('@/shared/api/auth-client', () => ({ getSessionContext: vi.fn() }))

import { listBrowserEgressReceipts, type BrowserEgressReceiptsResponse } from '@/shared/api/browser-client'
import {
  EGRESS_ROWS_MAX,
  EVIDENCE_POLL_INTERVAL_MS,
  emptyEgressState,
  mergeEgressPage,
  startEgressPolling,
  type EvidenceSource,
} from './use-browser-evidence'

const source: EvidenceSource = { orgId: 'org-1', sessionId: 'session-1', zdr: false }

function setVisibility(state: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state })
}

function page(sequence: number): BrowserEgressReceiptsResponse {
  return {
    receipts: [
      { sequence, method: 'GET', url: `https://example.com/${sequence}`, decision: 'allow', policy: 'open_web', timestampMs: sequence },
    ],
    counts: { allow: 1, block: 0 },
    nextSequence: sequence,
    zdr: false,
  }
}

beforeEach(() => {
  vi.useFakeTimers()
  setVisibility('visible')
  vi.mocked(listBrowserEgressReceipts).mockReset()
})

afterEach(() => {
  vi.useRealTimers()
  setVisibility('visible')
})

describe('mergeEgressPage', () => {
  it('sums the counts, advances the cursor and keeps the newest rows', () => {
    let state = emptyEgressState()
    for (let index = 1; index <= EGRESS_ROWS_MAX + 10; index += 1) {
      state = mergeEgressPage(state, page(index))
    }
    expect(state.rows).toHaveLength(EGRESS_ROWS_MAX)
    expect(state.rows[state.rows.length - 1]?.sequence).toBe(EGRESS_ROWS_MAX + 10)
    expect(state.counts.allow).toBe(EGRESS_ROWS_MAX + 10)
    expect(state.cursor).toBe(EGRESS_ROWS_MAX + 10)
  })
})

describe('startEgressPolling', () => {
  it('polls every interval with the cursor from the previous page', async () => {
    vi.mocked(listBrowserEgressReceipts).mockResolvedValueOnce(page(1)).mockResolvedValueOnce(page(2))
    const onState = vi.fn()
    const stop = startEgressPolling({ source, onState, onError: vi.fn() })

    await vi.advanceTimersByTimeAsync(0)
    expect(listBrowserEgressReceipts).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(EVIDENCE_POLL_INTERVAL_MS)
    expect(listBrowserEgressReceipts).toHaveBeenCalledTimes(2)
    expect(vi.mocked(listBrowserEgressReceipts).mock.calls[1]?.[2]).toMatchObject({ afterSequence: 1 })
    stop()
  })

  it('does not poll while the document is hidden, and resumes when it is visible again', async () => {
    vi.mocked(listBrowserEgressReceipts).mockResolvedValue(page(1))
    const stop = startEgressPolling({ source, onState: vi.fn(), onError: vi.fn() })
    await vi.advanceTimersByTimeAsync(0)
    expect(listBrowserEgressReceipts).toHaveBeenCalledTimes(1)

    setVisibility('hidden')
    await vi.advanceTimersByTimeAsync(EVIDENCE_POLL_INTERVAL_MS * 4)
    expect(listBrowserEgressReceipts).toHaveBeenCalledTimes(1)

    setVisibility('visible')
    document.dispatchEvent(new Event('visibilitychange'))
    await vi.advanceTimersByTimeAsync(0)
    expect(listBrowserEgressReceipts).toHaveBeenCalledTimes(2)
    stop()
  })

  it('aborts the in-flight read on stop and delivers no late state', async () => {
    let captured: AbortSignal | undefined
    vi.mocked(listBrowserEgressReceipts).mockImplementation(
      (_org, _session, _options, signal) => {
        captured = signal
        return new Promise(() => undefined)
      },
    )
    const onState = vi.fn()
    const stop = startEgressPolling({ source, onState, onError: vi.fn() })
    await vi.advanceTimersByTimeAsync(0)
    expect(captured?.aborted).toBe(false)

    stop()
    expect(captured?.aborted).toBe(true)
    expect(onState).not.toHaveBeenCalled()
  })
})
