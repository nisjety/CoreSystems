/**
 * The films' fictional world must stay consistent and free of real data:
 * the invoice arithmetic adds up, scene 02 names Bring (not the order system)
 * as the tracking source, and no real name, organisation or address appears.
 */
import { describe, expect, it } from 'vitest'
import * as base from './fjordform'
import * as undersok from './fjordform-undersok'

const NBSP = ' '

/** «1 380,00» (NBSP thousands separator) to 1380. */
function amount(text: string): number {
  return Number(text.replace(new RegExp(NBSP, 'g'), '').replace(',', '.').replace(/[^\d.]/g, ''))
}

function strings(value: unknown, out: string[] = []): string[] {
  if (typeof value === 'string') out.push(value)
  else if (Array.isArray(value)) for (const item of value) strings(item, out)
  else if (value && typeof value === 'object') for (const item of Object.values(value)) strings(item, out)
  return out
}

describe('scene 03 invoice story', () => {
  const [avtale, ordre, faktura] = undersok.undersokCompareRows
  const quantity = 120

  it('compares agreement, order and invoice, and only the invoice deviates', () => {
    expect(avtale!.label).toBe('Avtale')
    expect(ordre!.label).toBe('Ordre 4821')
    expect(faktura!.label).toBe('Faktura 20471')
    expect(amount(avtale!.value)).toBe(1240)
    expect(amount(ordre!.value)).toBe(1240)
    expect(amount(faktura!.value)).toBe(1380)
  })

  it('states the deviation as quantity times the price difference', () => {
    const difference = (amount(faktura!.value) - amount(avtale!.value)) * quantity
    expect(difference).toBe(16800)
    expect(amount(undersok.undersokDeviation.amount)).toBe(difference)
    expect(undersok.undersokProposal).toContain(`16${NBSP}800,00`)
    expect(undersok.undersokApproval.detail).toContain(`16${NBSP}800,00`)
    expect(undersok.undersokFindingChunks.join('')).toContain(`16${NBSP}800,00`)
  })

  it('reveals the document in five steps, ending with the proposal', () => {
    const lines = ([1, 2, 3, 4, 5] as const).map((stage) => undersok.undersokArtifactMarkdown(stage))
    expect(lines[0]!.split('\n').filter((l) => l.startsWith('- '))).toHaveLength(1)
    expect(lines[2]!.split('\n').filter((l) => l.startsWith('- '))).toHaveLength(3)
    expect(lines[3]).toContain('## Avvik')
    expect(lines[3]).not.toContain('## Forslag')
    expect(lines[4]).toContain('## Forslag')
    expect(lines[4]).toContain('Kreditnota')
  })

  it('is a customer problem: no fault of Verevon itself is described', () => {
    const text = strings(undersok).join(' ')
    expect(text).not.toMatch(/Verevon (feil|bug)|mapping|customer_id|eksport/i)
  })
})

describe('scene 02 delivery story', () => {
  it('takes tracking status from Bring, not from the order system', () => {
    const [tracking] = base.nordlysSources
    expect(tracking!.title).toMatch(/Bring/)
    expect(strings(base).join(' ')).not.toMatch(/Visma/i)
    expect(base.nordlysStatusLines.slice(0, 2).every((line) => line.ref === 'Bring')).toBe(true)
  })

  it('sends the suggestion text as the question', () => {
    expect(base.nordlysQuestion).toBe(base.nordlysSuggestion.text)
    expect(base.nordlysQuestion).toMatch(/^Bring melder at Nordlys-leveransen er forsinket\./)
  })
})

describe('both scenes', () => {
  it('contain no real person, organisation or address', () => {
    const everything = strings([base, undersok]).join(' ')
    expect(everything).not.toMatch(/\bIma\b|Aquatiq|Velion|Dacosta/i)
    for (const email of everything.match(/[\w.+-]+@[\w.-]+/g) ?? []) {
      expect(email.endsWith('@fjordform.example')).toBe(true)
    }
  })

  it('name the fictional organisation', () => {
    expect(base.fjordformOrg.name).toBe('Fjordform AS')
    expect(base.fjordformSession.org.name).toBe('Fjordform AS')
  })
})
