import { expect, test } from '@playwright/test'
import { readFileSync, readdirSync } from 'node:fs'
import { resolve } from 'node:path'
import { openFresh, restrictSources, send, transcript, latestArtifacts, type Transcript } from './product-acceptance-helpers'
import { containsRoundedNumber, grossProfitChangeClaims } from './product-sales-oracle'
import { scenarioRecording } from './product-recording-camera'
import { unsupportedDatedApprovalRows } from './product-project-oracle'

const root = resolve('apps/verevon-web/plans/product-recordings')
const scenarios = ['02-salgsrapport', '03-kampanje', '04-prosjektplan'] as const
const words = (value: string) => value.match(/[\p{L}\p{N}]+(?:[-’'][\p{L}\p{N}]+)*/gu)?.length ?? 0
const assistantText = (value: Transcript | null) => value?.turns.filter(turn => turn.role === 'assistant').at(-1)?.content ?? ''
const output = (value: Transcript | null) => latestArtifacts(value).map(a => a.content).join('\n\n') + '\n' + assistantText(value)

// Independent calculation from the maintained CSV; never ask the model to
// grade its own arithmetic or use an expected answer as input to the task.
function salesOracle() {
  const rows = readFileSync(resolve(root, '02-salgsrapport/salg.csv'), 'utf8').trim().split(/\r?\n/).slice(1)
  const totals = new Map<string, { revenue: number; cost: number }>()
  for (const row of rows) {
    const [week, group, , , revenue, cost] = row.split(',')
    for (const key of [`${week}/${group}`, `${week}/Totalt`]) {
      const total = totals.get(key) ?? { revenue: 0, cost: 0 }
      total.revenue += Number(revenue); total.cost += Number(cost); totals.set(key, total)
    }
  }
  return [...totals].map(([key, t]) => ({ key, revenue: t.revenue, profit: t.revenue - t.cost, margin: 100 * (t.revenue - t.cost) / t.revenue }))
}

function containsNumber(text: string, number: number) {
  const normalized = text.replace(/(?<=\d)[ \u00a0\u202f](?=\d{3}(?:\D|$))/g, '').replace(/,/g, '.')
  return new RegExp(`(?<![\\d.])${String(number).replace('.', '\\.')}\\b`).test(normalized)
}

function checkSalesNarrative(text: string) {
  const totals = salesOracle()
  // Correct table cells cannot excuse a contradictory change in the summary.
  const groups = [...new Set(totals.map(row => row.key.split('/')[1]))].filter(group => group !== 'Totalt')
  for (const claim of grossProfitChangeClaims(text, groups)) {
    const before = totals.find(row => row.key === `36/${claim.group}`)!
    const after = totals.find(row => row.key === `37/${claim.group}`)!
    const profitChange = after.profit - before.profit
    expect(claim.amount, `${claim.group} gross-profit change in the prose must match the CSV`).toBe(profitChange)
    if (claim.percentage) {
      expect(containsRoundedNumber(claim.percentage, 100 * profitChange / before.profit), `${claim.group} gross-profit percentage change in the prose`).toBe(true)
    }
  }
  // S2 describes offers, without a tender/procurement process. The two desk
  // channels both fell 30%; the saved "both channels halved" claim is false.
  expect(text).not.toMatch(/\b[\p{L}-]*anbud[\p{L}-]*\b/iu)
  expect(text).not.toMatch(/\bbåde\s+nettbutikk\s+og\s+bedriftssalg\s+halverte\b/i)
}

function section(text: string, heading: RegExp, dateMetadata?: RegExp) {
  const headings = [...text.matchAll(/^(#{1,6})\s+(.+)$/gm)]
  const matches = (match: RegExpMatchArray, index: number) => {
    if (heading.test(match[2])) return true
    if (!dateMetadata || !/LinkedIn|innlegg/i.test(match[2])) return false
    const next = headings.slice(index + 1).find(next => next[1].length <= match[1].length)
    // A numbered post can declare its date in a leading metadata block.
    // Never infer the date from an incidental mention in the customer copy.
    const metadata = text.slice(match.index! + match[0].length, next?.index ?? text.length).trim().split(/\r?\n\r?\n/)[0]
    return dateMetadata.test(metadata)
  }
  const index = headings.findIndex((match, index) => {
    if (!matches(match, index)) return false
    const following = headings.slice(index + 1)
    const end = following.findIndex(next => next[1].length <= match[1].length)
    // A document title such as "LinkedIn og e-post" is a container when it
    // has a matching child section. Measure the actual deliverable body.
    return !following.slice(0, end < 0 ? undefined : end).some((child, offset) => matches(child, index + 1 + offset))
  })
  expect(index, `missing deliverable heading matching ${heading}`).toBeGreaterThanOrEqual(0)
  const start = headings[index]
  const next = headings.slice(index + 1).find(match => match[1].length <= start[1].length)
  return text.slice(start.index! + start[0].length, next?.index ?? text.length).trim()
}

function copyBody(text: string) {
  return text.split(/\r?\n/).filter(line => !/^\s*(?:#{1,6}\s|---+$|\*?\*?(?:Rapportdato|Dato|Vinkel|Status|(?:Antall )?ord(?:antall|telling|tall)?(?:\s+brødtekst)?(?:\s*\([^)]*\))?|Maks emneknagger|Rekkevidde(?:\s*\([^)]*\))?|Kilde(?:r|grunnlag)?|Produktinformasjon|Bygger på)\s*\*?\*?\s*:)/i.test(line))
    .join('\n').replace(/[*`]/g, '').trim()
}

function campaignCopy(text: string) {
  // A source note may sit below each piece instead of in one final appendix.
  // Its explicit label separates internal evidence from customer-facing copy.
  const sourceNote = /^\s*(?:(?:#{1,6}\s*)?(?:\*\*)?(?:Produktinformasjon(?:\s+[^\n]*)?|Kildenoter|Kildegrunnlag|Faktagrunnlag|Bygger på)[^\n]*|#{1,6}\s+(?:Kilder|Sources)(?:[\s/][^\n]*)?)$/im.exec(text)
  return copyBody(sourceNote ? text.slice(0, sourceNote.index) : text)
    .split(/\r?\n/).filter(line => !/^\s*Produktgrunnlag(?:\s*\([^)]*\))?\s*:|^\s*\(?\d+\s+ord\)?\s*$/i.test(line)).join('\n').trim()
}

function checkCampaign(text: string) {
  const posts = [21, 23, 25].map(day => campaignCopy(section(text,
    new RegExp(`(?:LinkedIn|innlegg).*\\b${day}\\b|^${day}\\.`, 'i'),
    new RegExp(`^\\s*\\*{0,2}Dato\\*{0,2}:\\*{0,2}\\s*${day}\\.\\s*sep(?:tember)?(?:\\s+2026)?\\s*$`, 'im'))))
  let email = section(text, /e-post|epost/i)
  // Anchor the label so "Forhåndsvisningstekst:" is not mistaken for "Tekst:".
  const bodyLabel = /^\s*(?:#{1,6}\s+)?(?:\*{1,2})?(?:Brødtekst|E-posttekst|Tekst)(?:\*{1,2})?(?:\s*\*?\([^\n)]*\)\*?)?\s*:(?:\*{1,2})?/im.exec(email)
  if (bodyLabel) email = email.slice(bodyLabel.index + bodyLabel[0].length)
  else email = email.split(/\r?\n/).filter(line => !/emne(?:felt)?\s*\*?\*?:|forhåndsvisning|preview|preheader/i.test(line)).join('\n')
  email = campaignCopy(email)
  for (const [index, body] of [...posts, email].entries()) {
    const min = index < 3 ? 60 : 80; const max = index < 3 ? 90 : 120
    expect(words(body), `piece ${index + 1}: ${words(body)} words\n${body}`).toBeGreaterThanOrEqual(min)
    expect(words(body), `piece ${index + 1}: ${words(body)} words\n${body}`).toBeLessThanOrEqual(max)
    expect(body).not.toMatch(/\b(?:du|deg|din|ditt|dine)\b/i)
    expect(body).not.toMatch(/https?:|www\.|\p{Extended_Pictographic}/u)
    expect(body).not.toMatch(/stabilt?|bærekraft|energispar|energibespar|produktiv|neste dag|revolusjon|verdens beste|garantert|kundeuttalelse/i)
    // M2 gives dimensions and colours, not claims about footprint or suitability.
    expect(body).not.toMatch(/tar lite[n]? plass|fungerer i nøytrale kontormiljøer|uten å flytte på annet utstyr|uten å kreve en full ominnredning/i)
    // M2 documents controls and colours, not app absence, suitability for
    // unspecified interiors, or an exhaustive information guarantee.
    expect(body).not.toMatch(/ingen app|uten app|passer inn i ulike (?:interiører|kontormiljøer)|passer til de fleste kontormiljøer|uten å forstyrre kollegene|alt dere trenger/i)
    expect(body).not.toMatch(/passer inn på de fleste kontorplasser|gjør det enkelt å tilpasse lampen til eksisterende inventar/i)
    // The supplied sources contain no customer interviews or conversations.
    expect(body).not.toMatch(/(?:kontoransvarlige|kunder)\s+vi\s+(?:ofte\s+)?snakker med|våre kunder\s+(?:sier|forteller)/i)
    if (index < 3) expect(body.match(/#[\p{L}\p{N}]+/gu)?.length ?? 0).toBeLessThanOrEqual(2)
  }
  return { posts, email }
}

function checkProjectRepairUncertainty(text: string) {
  expect(text).toMatch(/(?:feilretting|retting)[^\n]*(?:ukjent|ikke estimert|ikke anslått|uavklart|varighet (?:er )?ikke oppgitt|ikke (?:lagt inn|avsatt|satt av) tid|ingen buffer)|(?:ukjent|ikke estimert|ikke anslått|uavklart|varighet (?:er )?ikke oppgitt|ikke (?:lagt inn|avsatt|satt av) tid|ingen buffer)[^\n]*(?:feilretting|retting)|(?:varighet|tid)[^\n]*(?:feilretting|retting)[^\n]*(?:ikke oppgitt|ukjent|uavklart)/i)
  // P2 supplies no repair estimate. A caveat elsewhere cannot justify a
  // factual minimum duration or date range invented for technical repairs.
  expect(text).not.toMatch(/(?:feilretting|retting)[^\n.]*\b(?:tar|trenger|krever)\s+minst\s+(?:\d|en\b|én\b|ett\b|to\b|tre\b|fire\b)/i)
}

for (const scenario of scenarios) test(`Q05 ${scenario}: fixed brief, follow-up and durable artifacts`, async ({ page }, info) => {
  const capture = scenarioRecording(page, info, scenario)
  try {
  const errors: string[] = []; page.on('pageerror', error => errors.push(error.message))
  page.on('console', entry => { if (entry.type() === 'error') errors.push(entry.text()) })
  await openFresh(page)
  await restrictSources(page)
  const folder = resolve(root, scenario)
  const sourceFiles = readdirSync(folder).filter(name => name === 'kildepakke.md' || name.endsWith('.csv'))
  await page.locator('input[type="file"]').first().setInputFiles(sourceFiles.map(name => resolve(folder, name)))
  const initial = await send(page, readFileSync(resolve(folder, 'prompt.txt'), 'utf8').trim(), info, 'initial', capture ? () => capture.prompt('initial') : undefined)
  capture?.mark('initial-complete')
  if (initial) capture?.thread(initial.threadId)
  await info.attach('initial-transcript', { body: JSON.stringify(initial, null, 2), contentType: 'application/json' })
  const initialArtifacts = latestArtifacts(initial)
  expect(initialArtifacts.length, 'the result must be a reopenable deliverable').toBeGreaterThan(0)
  const initialOutput = output(initial)
  const initialStream = info.attachments.find(attachment => attachment.name === 'initial-stream')?.body?.toString('utf8') ?? ''
  const initialFrame = initialStream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
  const initialReceipt = JSON.parse(/^data: (.+)$/m.exec(initialFrame ?? '')?.[1] ?? '{}').receipt
  const countCalls = (stream: string) => stream.split(/\r?\n\r?\n/).filter(frame => /^event: tool_call\s*$/m.test(frame))
    .map(frame => JSON.parse(/^data: (.+)$/m.exec(frame)?.[1] ?? '{}')).filter(call => call.name === 'count_words')
  expect(countCalls(initialStream), 'these initial document tasks need no separate word-count generation round').toHaveLength(0)
  if (initialArtifacts.length === 1) {
    expect(initialReceipt, 'a checked document must finish with its bound receipt, not a second unchecked version in chat').toMatchObject({
      scope: 'artifact_creation_binding', artifactId: initialArtifacts[0].id, version: initialArtifacts[0].version,
    })
  }
  expect(assistantText(initial)).not.toMatch(/<artifact\b/i)
  expect(initialOutput).not.toMatch(/FF-1042|HARBOR-739|Nordvik Studio/)
  if (scenario === '02-salgsrapport') {
    const oracle = salesOracle()
    await info.attach('independent-sales-oracle', { body: JSON.stringify(oracle, null, 2), contentType: 'application/json' })
    for (const total of oracle) {
      expect(containsNumber(initialOutput, total.revenue), `${total.key} revenue ${total.revenue}`).toBe(true)
      expect(containsNumber(initialOutput, total.profit), `${total.key} profit ${total.profit}`).toBe(true)
      expect(containsRoundedNumber(initialOutput, total.margin), `${total.key} weighted margin correctly rounded at the displayed precision`).toBe(true)
    }
    expect(initialOutput).toMatch(/hypotes|mulig|ikke.*(?:dokumentert|bevist|attribusjon)/i)
    expect(initialOutput).toMatch(/tilbud/i)
    checkSalesNarrative(initialOutput)
  } else if (scenario === '03-kampanje') {
    const stream = info.attachments.find(attachment => attachment.name === 'initial-stream')?.body?.toString('utf8') ?? ''
    const frame = stream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
    const receipt = JSON.parse(/^data: (.+)$/m.exec(frame ?? '')?.[1] ?? '{}').receipt
    const checks = receipt?.artifactChecks?.[0]?.sourceReview?.documentChecks
    expect(checks, 'the final campaign bodies must have local checks bound to the accepted artifact').toHaveLength(4)
    for (const check of checks) {
      expect(check.words).toBeGreaterThanOrEqual(check.minimum)
      expect(check.words).toBeLessThanOrEqual(check.maximum)
      expect(check.bodyHash).toMatch(/^[a-f0-9]{64}$/)
    }
    for (const day of [21, 23, 25]) expect(initialOutput).toMatch(new RegExp(`${day}\\.\\s*(?:september|sept|09)`))
    expect(initialOutput).toMatch(/emne(?:felt)?/i)
    expect(initialOutput).toMatch(/forhåndsvisning|preview|preheader/i)
    expect(initialOutput).toMatch(/M2|produktark/i)
    checkCampaign(initialArtifacts.map(a => a.content).join('\n\n'))
  } else {
    for (const day of [16, 17, 18, 21, 22, 23, 24, 28, 30]) expect(initialOutput).toMatch(new RegExp(`\\b${day}\\b`))
    for (const concept of [/avhengig/i, /ferdigkriter|godkjenningskriter/i, /stedfortred|reserve|backup|buffer/i, /tidligst|tidligste/i]) expect(initialOutput).toMatch(concept)
    expect(initialOutput).not.toMatch(/vedtatt[^\n]{0,40}\|?\s*Pilotstart\s*:\s*30/i)
    // P1 permits schema approval on the 16th and targets a pilot on the 30th;
    // neither date is an already approved milestone.
    expect(initialOutput).not.toMatch(/\b(?:16|30)\.\s*sep(?:tember)?(?:\s*\([^\n)]*\))?\s*(?:\*\*)?\s*\[VEDTATT\]/i)
    expect(initialOutput).not.toMatch(/\[VEDTATT\]\s*(?:\*\*)?\s*(?:16|30)\.\s*sep/i)
    // Check an affirmative status in its own cell without rejecting "ikke vedtatt".
    expect(unsupportedDatedApprovalRows(initialOutput)).toEqual([])
    // Timeline annotations are claims too, even inside a code fence.
    expect(initialOutput).not.toMatch(/\b(?:16|30)\.\s*sep[^\n]*(?:←|→|—|–|:)\s*(?:\*\*)?vedtatt(?:\s+(?:dato|mål))?/i)
    // P2 explicitly excludes weekends. Check stated inclusive date-range
    // counts independently; the saved output called Sep 24–26 three workdays.
    const numberWords = ['null', 'én', 'to', 'tre', 'fire', 'fem', 'seks', 'sju', 'åtte', 'ni', 'ti']
    for (const span of initialOutput.matchAll(/\b(\d+|én|to|tre|fire|fem|seks|sju|åtte|ni|ti)\s+arbeidsdager\s*\((\d{1,2})\s*[–−-]\s*(\d{1,2})\.\s*september\)/gi)) {
      let actual = 0
      for (let day = Number(span[2]); day <= Number(span[3]); day++) {
        const weekday = new Date(Date.UTC(2026, 8, day)).getUTCDay()
        if (weekday > 0 && weekday < 6) actual++
      }
      const claimed = /^\d+$/.test(span[1]) ? Number(span[1]) : numberWords.indexOf(span[1].toLowerCase())
      expect(claimed, `working days in September ${span[2]}–${span[3]}`).toBe(actual)
    }
    const weekdayNames = [/^(?:søn|sun)/i, /^(?:man|mon)/i, /^(?:tir|tue)/i, /^(?:ons|wed)/i, /^(?:tor|thu)/i, /^(?:fre|fri)/i, /^(?:lør|sat)/i]
    for (const date of initialOutput.replace(/\*/g, '').matchAll(/\b(\d{1,2})\.\s*sep(?:tember)?(?:\s+2026)?\s*\((mandag|tirsdag|onsdag|torsdag|fredag|lørdag|søndag|monday|tuesday|wednesday|thursday|friday|saturday|sunday|man|tir|ons|tor|fre|lør|søn|mon|tue|wed|thu|fri|sat|sun)\)/gi)) {
      expect(date[2], `weekday for September ${date[1]}, 2026`).toMatch(weekdayNames[new Date(Date.UTC(2026, 8, Number(date[1]))).getUTCDay()])
    }
    checkProjectRepairUncertainty(initialOutput)
  }
  await page.screenshot({ path: info.outputPath('initial.png'), fullPage: true })
  await capture?.source()
  await capture?.result('initial', initialArtifacts.at(-1)!.title)
  const followUp = /\*\*Oppfølging å sende:\*\* «([^»]+)»/.exec(readFileSync(resolve(folder, 'REGI.md'), 'utf8'))?.[1]
  expect(followUp).toBeTruthy()
  const revised = await send(page, followUp!, info, 'revision', capture ? () => capture.prompt('revision') : undefined)
  capture?.mark('revision-complete')
  await info.attach('revised-transcript', { body: JSON.stringify(revised, null, 2), contentType: 'application/json' })
  const artifacts = latestArtifacts(revised)
  const revisedStream = info.attachments.find(attachment => attachment.name === 'revision-stream')?.body?.toString('utf8') ?? ''
  const revisedFrame = revisedStream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
  const revisedReceipt = JSON.parse(/^data: (.+)$/m.exec(revisedFrame ?? '')?.[1] ?? '{}').receipt
  expect(revisedReceipt?.scope, 'the follow-up also needs a checked artifact completion').toMatch(/^artifact_(?:creation|revision)_binding$/)
  expect(revisedReceipt?.artifactChecks?.[0]?.sourceReview?.checker).toBe('attachment-source-review-v16')
  expect(assistantText(revised)).not.toMatch(/<artifact\b/i)
  expect(artifacts.length).toBeGreaterThanOrEqual(initialArtifacts.length)
  if (scenario === '04-prosjektplan') {
    const plan = initialArtifacts[0]
    expect(artifacts.some(artifact => artifact.id === plan.id && artifact.content === plan.content),
      'the project plan must remain current when a separate status note is requested').toBe(true)
    expect(artifacts.some(artifact => artifact.id !== plan.id), 'the internal status needs its own artifact').toBe(true)
    expect(revisedReceipt?.artifactId).not.toBe(plan.id)
  }
  if (scenario === '03-kampanje') {
    expect(countCalls(revisedStream), 'campaign revision counts also belong to the local document checker').toHaveLength(0)
    const revisionStream = info.attachments.find(attachment => attachment.name === 'revision-stream')?.body?.toString('utf8') ?? ''
    const revisionFrame = revisionStream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
    const revisionReceipt = JSON.parse(/^data: (.+)$/m.exec(revisionFrame ?? '')?.[1] ?? '{}').receipt
    expect(revisionReceipt?.artifactChecks?.[0]?.sourceReview?.documentChecks, 'body checks must also run after revision and preservation').toHaveLength(4)
    expect(artifacts.some(a => initialArtifacts.some(first => a.id === first.id && a.version > first.version)), 'revise the existing campaign artifact').toBe(true)
    expect(output(revised)).toMatch(/team/i)
    const before = checkCampaign(initialArtifacts.map(a => a.content).join('\n\n'))
    const after = checkCampaign(artifacts.map(a => a.content).join('\n\n'))
    expect(after.posts[0], 'September 21 remains unchanged').toBe(before.posts[0])
    expect(after.posts[2], 'September 25 remains unchanged').toBe(before.posts[2])
    expect(after.email, 'email remains unchanged').toBe(before.email)
    const initialDocument = initialArtifacts.map(a => a.content).join('\n\n')
    const revisedDocument = artifacts.map(a => a.content).join('\n\n')
    if (/^#+ .*publiseringsoversikt/im.test(initialDocument) && /^#+ .*publiseringsoversikt/im.test(revisedDocument)) {
      const overviewBefore = section(initialDocument, /publiseringsoversikt/i)
      const overviewAfter = section(revisedDocument, /publiseringsoversikt/i)
      if (overviewBefore !== overviewAfter) {
        expect(assistantText(revised), 'the summary must reflect the updated overview').not.toMatch(/publiseringsoversikt(?:en)?[^\n]{0,100}uendret|publiseringsoversikten viser fortsatt/i)
      }
    }
  } else {
    const changed = artifacts.filter(a => !initialArtifacts.some(first => first.id === a.id && first.content === a.content))
    const note = changed.map(a => a.content).join('\n') || assistantText(revised)
    expect(words(copyBody(note)), 'requested concise follow-up body').toBeLessThanOrEqual(scenario === '02-salgsrapport' ? 120 : 100)
    expect(note).not.toMatch(/FF-1042|HARBOR-739|Nordvik Studio/)
    if (scenario === '02-salgsrapport') {
      // S2 describes offers, without establishing a tender/procurement process.
      checkSalesNarrative(note)
    }
  }
  const finalTitle = artifacts.find(a => !initialArtifacts.some(first => first.id === a.id && first.content === a.content))?.title ?? artifacts.at(-1)!.title
  await capture?.result('revised', finalTitle)
  await page.evaluate(() => {
    localStorage.removeItem('verevon.chat.threadTranscripts.v1')
    sessionStorage.removeItem('verevon.chat.threadTranscripts.v1')
  })
  capture?.mark('reload')
  await page.reload()
  await expect(page.locator('.verevon-chat-page')).toBeVisible()
  await expect.poll(async () => latestArtifacts(await transcript(page)).map(a => ({ id: a.id, version: a.version, content: a.content })), { timeout: 20000 })
    .toEqual(artifacts.map(a => ({ id: a.id, version: a.version, content: a.content })))
  await page.screenshot({ path: info.outputPath('revision-reloaded.png'), fullPage: true })
  await capture?.result('reloaded', finalTitle)
  expect(errors).toEqual([])
  capture?.complete()
  } finally { await capture?.save() }
})
