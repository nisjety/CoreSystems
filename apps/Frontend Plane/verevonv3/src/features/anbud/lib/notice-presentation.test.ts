import { describe, expect, it } from 'vitest'
import type { TenderMatch } from '@/shared/api/anbud-client'
import { noticeTypeLabel, plannedNoticeLabel, searchMatches, sourceName } from './notice-presentation'

const tr = (no: string) => no
const base: TenderMatch = { noticeId: '2026-100010', title: 'Ny barnehage', cpv: [], nuts: [], verdict: 'unknown', band: 'none', gate: [], fit: [], gateCounts: { pass: 0, fail: 0, conditional: 0, unknown: 0 } }

describe('notice presentation', () => {
  it('names the day a prior information notice plans its call for, and nothing for other notices', () => {
    expect(plannedNoticeLabel({ ...base, noticeType: 'pin-only', plannedNotice: '2027-04-13' }, tr)).toBe('Utlysning planlagt 2027-04-13')
    expect(plannedNoticeLabel({ ...base, noticeType: 'cn-standard' }, tr)).toBeUndefined()
  })

  it('labels every notice type held, instead of showing its code', () => {
    expect(noticeTypeLabel({ ...base, noticeType: 'pmc' }, tr)).toBe('Markedsdialog')
    expect(noticeTypeLabel({ ...base, noticeType: 'veat' }, tr)).toBe('Intensjonskunngjøring')
    expect(noticeTypeLabel({ ...base, noticeType: 'can-modif' }, tr)).toBe('Kontraktsendring')
    expect(noticeTypeLabel({ ...base, noticeType: 'can-standard' }, tr)).toBe('Resultat')
    expect(noticeTypeLabel({ ...base, noticeType: 'cn-social' }, tr)).toBe('Konkurranse')
  })

  it('finds a title with an en dash by a hyphen, and the other way round, as the core does', () => {
    const enDash = String.fromCharCode(0x2013)
    const noBreak = String.fromCharCode(0xa0)
    const title = `Applikasjonssikkerhetssystem ${enDash} F5`
    expect(searchMatches(title, 'Applikasjonssikkerhetssystem - F5')).toBe(true)
    expect(searchMatches(title, `sikkerhetssystem${noBreak}-${noBreak}f5`)).toBe(true)
    expect(searchMatches('Drift - fagsystem', `drift ${enDash} fagsystem`)).toBe(true)
    expect(searchMatches(title, '  ')).toBe(true)
    expect(searchMatches(title, 'Applikasjonssikkerhetssystem F5')).toBe(false)
  })

  it('names the source a notice was fetched from', () => {
    expect(sourceName('ted')).toBe('TED')
    expect(sourceName('boamp')).toBe('BOAMP')
    expect(sourceName('fts')).toBe('Find a Tender')
    expect(sourceName('bzp')).toBe('BZP')
    expect(sourceName('doffin')).toBe('Doffin')
    expect(sourceName(undefined)).toBe('Doffin')
  })
})
