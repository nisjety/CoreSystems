import { describe, expect, it } from 'vitest'
import type { TenderCheck, TenderMatch } from '@/shared/api/anbud-client'
import { fitReason } from '@/features/anbud/lib/fit-reason'

const no = (text: string) => text
const check = (rule: string, outcome: TenderCheck['outcome']): TenderCheck => ({ rule, outcome, detail: `${rule} ${outcome}`, cite: { field: rule } })
const match = (gate: TenderCheck[], fit: TenderCheck[]): TenderMatch => ({
  noticeId: '2026-1', title: 'Åpen drift', cpv: ['72212000'], nuts: ['NO081'], verdict: 'pass', band: 'strong', gate, fit, gateCounts: { pass: 0, conditional: 0, fail: 0, unknown: 0 },
} as TenderMatch)

describe('fitReason', () => {
  it('says which checks a tender passes, in plain words', () => {
    const reason = fitReason(match([check('deadline', 'pass')], [check('cpv', 'pass'), check('region', 'pass')]), no)
    expect(reason).toBe('Passer: fristen, bransjekoden og regionen.')
  })

  it('names the blocker before anything else, even when other checks pass', () => {
    const reason = fitReason(match([check('deadline', 'fail')], [check('cpv', 'pass')]), no)
    expect(reason).toBe('Kan ikke by: fristen er passert.')
  })

  it('adds what still has to be checked after the passing checks', () => {
    const reason = fitReason(match([check('requirement', 'conditional')], [check('value', 'pass')]), no)
    expect(reason).toBe('Passer: verdien. Må sjekkes: kvalifiseringskrav.')
  })

  it('says what has to be checked when nothing else is known', () => {
    expect(fitReason(match([check('document_requirement', 'unknown')], []), no)).toBe('Må sjekkes: krav i dokumentene.')
  })

  it('says so when there is nothing to judge on, instead of guessing', () => {
    expect(fitReason(match([], []), no)).toBe('Ikke nok data til å vurdere.')
  })

  it('gives the same meaning in English', () => {
    const en = (_no: string, english: string) => english
    expect(fitReason(match([check('deadline', 'pass')], [check('cpv', 'pass')]), en)).toBe('Fits: the deadline and the industry code.')
  })
})
