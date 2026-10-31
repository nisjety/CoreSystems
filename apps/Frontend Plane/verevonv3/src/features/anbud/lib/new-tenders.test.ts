import { describe, expect, it } from 'vitest'
import type { TenderMatch } from '@/shared/api/anbud-client'
import { newSince } from '@/features/anbud/lib/new-tenders'

const match = (issuedAt: string | undefined, verdict: TenderMatch['verdict'] = 'pass') => ({ noticeId: issuedAt ?? 'none', issuedAt, verdict } as TenderMatch)

describe('newSince', () => {
  it('counts the matched tenders issued after the last mark, and no others', () => {
    const matches = [match('2026-10-27T08:00:00Z'), match('2026-10-29T08:00:00Z'), match('2026-10-30T08:00:00Z')]
    expect(newSince(matches, '2026-10-28T10:00:00Z')).toBe(2)
  })

  it('counts nothing before the organisation has marked anything', () => {
    expect(newSince([match('2026-10-29T08:00:00Z')], null)).toBe(0)
  })

  it('never counts a tender with no issue time, or one that is not a match', () => {
    expect(newSince([match(undefined), match('2026-10-29T08:00:00Z', 'unknown')], '2026-10-28T10:00:00Z')).toBe(0)
  })
})
