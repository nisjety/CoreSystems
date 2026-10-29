import type { TenderMatch } from '@/shared/api/anbud-client'

/**
 * How many of the matched tenders were issued after the organisation last marked its tenders as seen. Nothing is
 * new before a mark is known. A tender with no issue time is never counted as new, since nothing shows it is.
 */
export function newSince(matches: TenderMatch[], lastSeen: string | null): number {
  if (!lastSeen) return 0
  const since = Date.parse(lastSeen)
  if (Number.isNaN(since)) return 0
  return matches.filter(match => match.verdict !== 'unknown' && match.issuedAt !== undefined && Date.parse(match.issuedAt) > since).length
}
