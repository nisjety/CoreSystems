import { expect, test } from '@playwright/test'
import { createHash } from 'node:crypto'
import { openFresh, restrictSources, send } from './product-acceptance-helpers'

// A bounded, attachment-grounded latency cohort. It is deliberately labelled as
// a microbenchmark, not a substitute for complete product-scenario sign-off.
const source = '# Fiktiv testbrief\n\nTeam Fjord har 8 medarbeidere. Pilotmålet er 24. september 2026, men datoen er ikke bekreftet. Nora eier planen. Amir eier teknisk avklaring. Teknisk avklaring må være ferdig før pilotdatoen kan bekreftes. Det finnes ingen godkjent budsjettramme. Ingen invitasjon er sendt. Neste beslutning er om teknisk avklaring er tilstrekkelig.\n'
const prompt = 'Bruk bare vedlegget. Svar direkte i chatten uten verktøy eller dokument: skriv en norsk statusoppdatering på 100–120 ord med tre avsnitt: situasjon, risiko og neste beslutning. Ta med alle fakta fra briefen. Skill mål fra bekreftede beslutninger. Ikke legg til nye fakta eller påstander om utførte handlinger.'
const samples = Math.max(1, Math.min(40, Number(process.env.Q08_PERF_SAMPLES) || 1))

for (let sample = 1; sample <= samples; sample++) test(`Q08 latency microbenchmark ${sample}/${samples}`, async ({ page }, info) => {
  await openFresh(page)
  await restrictSources(page)
  await page.locator('input[type="file"]').first().setInputFiles({ name: 'latency-brief.md', mimeType: 'text/markdown', buffer: Buffer.from(source) })
  const result = await send(page, prompt, info, 'benchmark')
  // This workload explicitly asks for no tools. A model's validation loop
  // must not quietly override that choice just to count the draft's words.
  const stream = info.attachments.find(attachment => attachment.name === 'benchmark-stream')?.body?.toString('utf8') ?? ''
  expect(stream).not.toMatch(/^event: tool_call\s*$/m)
  await info.attach('benchmark-identity', { body: JSON.stringify({ workload: 'attachment-status-100-120-words', sample, sourceSha256: createHash('sha256').update(source).digest('hex'), promptSha256: createHash('sha256').update(prompt).digest('hex'), threadId: result?.threadId, model: result?.turns.at(-1)?.modelUsed }), contentType: 'application/json' })
  const text = result?.turns.at(-1)?.content ?? ''
  const streamedText = stream.split(/\r?\n\r?\n/).filter(frame => /^event: chunk\s*$/m.test(frame))
    .map(frame => { const chunk = JSON.parse(/^data: (.+)$/m.exec(frame)?.[1] ?? '{}'); return chunk.delta ?? chunk.content ?? '' }).join('')
  expect(streamedText, 'captured UTF-8 evidence must match the user-visible answer').toBe(text)
  expect(text).toMatch(/\b8\b|åtte/i)
  expect(text).toMatch(/Nora/)
  expect(text).toMatch(/Amir/)
  expect(text).toMatch(/24\./)
  expect(text).toMatch(/ikke (?:(?:er|en) )?bekreftet|ubekreftet|betinget/i)
  expect(text).toMatch(/budsjett/i)
  const prose = text.replace(/^\s*(?:#{1,6}\s+|\*\*)?(?:Situasjon|Risiko|Neste beslutning)(?:\*\*)?\s*:?[ \t]*$/gim, '')
  const words = prose.match(/[\p{L}\p{N}]+(?:[-’'][\p{L}\p{N}]+)*/gu)?.length ?? 0
  await info.attach('benchmark-word-count', { body: JSON.stringify({ words, minimum: 100, maximum: 120 }), contentType: 'application/json' })
  expect(words, 'a fast answer must also satisfy the requested length').toBeGreaterThanOrEqual(100)
  expect(words).toBeLessThanOrEqual(120)
  const receiptFrame = stream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
  const receipt = JSON.parse(/^data: (.+)$/m.exec(receiptFrame ?? '')?.[1] ?? '{}').receipt
  expect(receipt, 'the runtime must check the delivered text, not just a draft').toMatchObject({
    schemaVersion: 1, scope: 'direct_answer_word_range', passed: true, words,
    requirement: { minimum: 100, maximum: 120 },
  })
  expect(receipt.attempts).toBeGreaterThanOrEqual(1)
  expect(receipt.attempts).toBeLessThanOrEqual(3)
  expect(receipt.sourceReview).toMatchObject({ checker: 'attachment-source-review-v16', scope: 'semantic_source_review' })
  expect(receipt.sourceReview.contentHash).toBe(receipt.contentHash)
  expect(receipt.sourceReview.reviewedSegments).toBeGreaterThanOrEqual(3)
  expect(receipt.sourceReview.evidence.length).toBeGreaterThan(0)
  expect(text).not.toMatch(/(?:ingen|ikke)[^.!?\n]{0,40}(?:deltakere|eksterne parter)[^.!?\n]{0,40}(?:varslet|informert)|(?:deltakere|eksterne parter)[^.!?\n]{0,40}ikke[^.!?\n]{0,40}(?:varslet|informert)/i)
  // The source gates date confirmation, not invitation planning or every
  // subsequent activity. These are saved semantic-review false negatives.
  expect(text).not.toMatch(/først når[^.\n]+(?:øvrige steg|invitasjoner)[^.\n]+planlegges/i)
  expect(text).not.toMatch(/disse to forholdene er uavklarte/i)
  await info.attach('benchmark-validation-receipt', { body: JSON.stringify(receipt), contentType: 'application/json' })
  const persisted = await page.request.get(`/api/v1/chat/threads/${encodeURIComponent(result!.threadId)}/messages`)
  expect(persisted.ok()).toBe(true)
  const payload = await persisted.json()
  const saved = (payload.data ?? payload).messages.filter((message: { role: string }) => message.role === 'assistant').at(-1)
  expect(saved.content).toBe(text)
  expect(saved.resultReceipt).toEqual(receipt)
})

test('Q13 an exact supported quotation is not rejected as an invented claim', async ({ page }, info) => {
  const supported = 'Team Nord har seks medarbeidere og arbeider med en intern pilot. Eva eier planen, mens Jonas har ansvar for teknisk avklaring. Pilotdatoen er et mål som ennå ikke er bekreftet. Gruppen har ikke fastsatt noen dato for en ekstern lansering.\n\nDet finnes ingen godkjent budsjettramme for piloten. Ingen invitasjon er sendt. Kilden sier ikke om andre varsler er sendt, eller om teknisk avklaring er ferdig. Det er derfor ikke grunnlag for å fastslå disse forholdene ut fra denne statusen.\n\nNeste beslutning gjelder om den tekniske avklaringen er tilstrekkelig. Kilden navngir ingen beslutningstaker. Denne teksten beskriver bare opplysningene i den fiktive statusen og dokumenterer ingen nye handlinger.'
  await openFresh(page)
  await restrictSources(page)
  await page.locator('input[type="file"]').first().setInputFiles({ name: 'supported-status.md', mimeType: 'text/markdown', buffer: Buffer.from(supported) })
  const result = await send(page, 'Bruk bare vedlegget. Svar direkte i chatten uten verktøy eller dokument: gjengi teksten ordrett på 100–120 ord. Behold de tre avsnittene. Ikke legg til noe.', info, 'supported')
  expect(result?.turns.at(-1)?.content).toBe(supported)
  const stream = info.attachments.find(attachment => attachment.name === 'supported-stream')?.body?.toString('utf8') ?? ''
  const frame = stream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
  expect(JSON.parse(/^data: (.+)$/m.exec(frame ?? '')?.[1] ?? '{}').receipt.sourceReview).toMatchObject({ checker: 'attachment-source-review-v16', reviewedSegments: 3 })
})

test('Q13 a held-out English source keeps uncertainty and decision ownership', async ({ page }, info) => {
  const supported = 'Team North has six colleagues. Eva owns the plan, while Jonas is responsible for technical clarification. The pilot date is a target and has not been confirmed. The group has not set a date for an external launch.\n\nNo budget has been approved for the pilot. No invitation has been sent. The source does not say whether other notices have been sent or whether technical clarification is complete. These points therefore remain unknown from this record.\n\nThe next decision concerns whether the technical clarification is sufficient. The source does not name a decision maker. This fictional update records no new actions.'
  await openFresh(page)
  await restrictSources(page)
  await page.locator('input[type="file"]').first().setInputFiles({ name: 'english-status.md', mimeType: 'text/markdown', buffer: Buffer.from(supported) })
  const result = await send(page, 'Use only the attachment. Answer directly in the chat without tools or a document: reproduce the English text verbatim in 100–120 words, keeping its three paragraphs. Add nothing.', info, 'english')
  expect(result?.turns.at(-1)?.content).toBe(supported)
  const stream = info.attachments.find(attachment => attachment.name === 'english-stream')?.body?.toString('utf8') ?? ''
  expect(stream).not.toMatch(/^event: tool_call\s*$/m)
  const frame = stream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
  expect(JSON.parse(/^data: (.+)$/m.exec(frame ?? '')?.[1] ?? '{}').receipt.sourceReview).toMatchObject({ checker: 'attachment-source-review-v16', reviewedSegments: 3 })
})
