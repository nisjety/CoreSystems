import { expect } from '@playwright/test'
import type { Artifact } from './product-acceptance-helpers'

export const internalHeading = /(?:^|\n)(?:>\s*)?(?:#{1,6}\s+|\*\*)[^\n]*(?:intern|kildeoversikt|kildehenvisninger)/i
export function assertCustomerDraft(artifact: Artifact, wordLimit: number, assistantContent: string) {
  const { content } = artifact
  const internalStart = content.search(internalHeading)
  // The requested separate note can follow the draft in the document or in
  // the accompanying assistant message. Do not require one presentation.
  expect(internalStart > 0 || internalHeading.test(assistantContent), 'internal notes must be separate from the customer draft').toBe(true)
  const draft = (internalStart > 0 ? content.slice(0, internalStart) : content).replace(/^---+\s*$/gm, '')
  const completeOutput = content + '\n' + assistantContent
  // The order reference may be the document's subject/title.
  expect(artifact.title + draft).toContain('FF-1042')
  // Norwegian prose may spell out these quantities; the factual values stay
  // fixed even when the author chooses words instead of digits.
  expect(draft).toMatch(/\b(?:9|ni)\b/i)
  expect(draft).toMatch(/\b(?:3|tre)\b/i)
  expect(draft).not.toContain('Verevon')
  // K2 says collection is not CONFIRMED. Lack of confirmation is not proof
  // the carrier has not collected anything (found during Q08 manual review).
  expect(completeOutput).not.toMatch(/(?:transportør(?:en)?\s+har\s+(?:(?:heller|ennå|fortsatt)\s+)*ikke\s+hentet|ingen transportør har hentet|etikett opprettet, ikke hentet)/i)
  // K3 requires an internal notification; it does not record one. No sending
  // tool is authorized in this draft-only fixture.
  expect(completeOutput).not.toMatch(/logistikk(?:ansvarlig)?\s+(?:er\s+)?varslet/i)
  expect(completeOutput).toMatch(/K3/)
  expect(completeOutput).toMatch(/neste (?:arbeidsdag|virkedag)|15\.\s*(?:september|sept|09)/i)
  // Subject/recipient metadata is separate from the customer-facing body.
  const greeting = draft.search(/^(?:Hei|Kjære)\b/im)
  const body = greeting >= 0 ? draft.slice(greeting) : draft
  expect(body.match(/[\p{L}\p{N}]+(?:[-’'][\p{L}\p{N}]+)*/gu)!.length).toBeLessThanOrEqual(wordLimit)
  const weekdays = ['søndag', 'mandag', 'tirsdag', 'onsdag', 'torsdag', 'fredag', 'lørdag']
  for (const match of completeOutput.matchAll(/(mandag|tirsdag|onsdag|torsdag|fredag|lørdag|søndag)\s+(\d{1,2})\.\s*september/gi)) {
    expect(match[1]!.toLowerCase(), 'calendar dates must agree with the 2026 fixture').toBe(weekdays[new Date(Date.UTC(2026, 8, Number(match[2]))).getUTCDay()])
  }
}
