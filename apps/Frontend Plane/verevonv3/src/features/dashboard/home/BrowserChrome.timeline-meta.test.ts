// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import type { BrowserOwnerTimelineItem } from '@/shared/api/browser-client'
import { ERROR_COPY } from '@/shared/i18n/errors'
import { ownerTimelineEventMeta } from './BrowserChrome'

const row = (fields: Partial<BrowserOwnerTimelineItem>): BrowserOwnerTimelineItem => ({
  id: 'evt-1',
  kind: 'action',
  occurredAt: '2026-10-19T10:00:00Z',
  ...fields,
})

describe('the owner timeline error line', () => {
  it('shows the end-user copy of a known error code, never the code', () => {
    for (const errorCode of ['browser_session_cap', 'browser_site_blocked', 'USER_TOOK_OVER', 'browser_process_lost']) {
      const meta = ownerTimelineEventMeta(row({ outcome: 'failed', errorCode }))
      expect(meta).toBe(`failed · ${ERROR_COPY[errorCode]!.no}`)
      expect(meta).not.toContain(errorCode)
    }
  })

  it('shows a neutral word for a code with no copy, never the code', () => {
    const meta = ownerTimelineEventMeta(row({ outcome: 'failed', errorCode: 'brand_new_quarry_code' }))
    expect(meta).toBe('failed · Feil')
    expect(meta).not.toContain('brand_new_quarry_code')
  })

  it('keeps who opened the tab beside the outcome', () => {
    expect(ownerTimelineEventMeta(row({ kind: 'tab', operation: 'opened', outcome: 'completed', initiatedBy: 'agent', tabId: 'tab-1' }))).toBe(
      'completed · agent · tab-1',
    )
  })
})
