// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { anbudDetail, resetAnbudState } from '@/features/anbud/lib/anbud-state'
import type { TenderMatch, TenderNoticeDetail } from '@/shared/api/anbud-client'
import { TenderDetailView } from './TenderDetailView'
import { TenderDetailSidebar } from './TenderDetailSidebar'
const client = vi.hoisted(() => ({ fetchTenderNotice: vi.fn(), fetchTenderAudit: vi.fn() }))
vi.mock('@/shared/api/anbud-client', () => client)
vi.mock('@/shared/session/session-store', () => ({ getSession: () => ({ activeOrg: { id: 'org-a' } }) }))
const notice: TenderMatch = { noticeId: '2026-100001', title: 'Rammeavtale for tjenester', buyer: 'Oslo kommune', noticeType: 'cn-standard', status: 'active', arrangementTypes: ['framework'], cpv: ['72000000'], nuts: ['NO081'], estimatedValue: { amount: '1000000.00', currency: 'NOK' }, deadline: { at: '2026-11-30T08:00:00Z', osloDay: '2026-11-30', daysLeft: 20, kind: 'tender' }, verdict: 'unknown', band: 'none', gate: [], fit: [], gateCounts: { pass: 0, fail: 0, conditional: 0, unknown: 0 } }
const detail: TenderNoticeDetail = { result: notice, descriptionTruncated: false, notice: { description: 'Dette er beskrivelsen fra kunngjøringen.', lots: [{ id: 'LOT-1', title: 'Drift', cpv: [], awardCriteria: [] }], lotsTotal: 1, documents: [{ url: 'https://example.com/docs' }, { url: 'javascript:alert(1)' }], documentsTotal: 2, selectionCriteria: [{ type: 'references', name: 'Referanser', description: 'Tre relevante oppdrag.' }], selectionCriteriaTotal: 1, exclusionGrounds: 0, criteriaInDocuments: false }, evidence: { source: 'doffin', noticeId: notice.noticeId, sha256: 'a'.repeat(64), fetchedAt: '2026-10-28T12:00:00Z', bytes: 1200 } }
beforeEach(() => { resetAnbudState(); client.fetchTenderNotice.mockReset().mockResolvedValue(detail); anbudDetail.open(notice) })
afterEach(cleanup)
it('shows source details and navigates documents, requirements, lots and provenance from the sidebar', async () => {
  render(() => <><TenderDetailSidebar /><TenderDetailView /></>)
  expect(await screen.findByText(detail.notice.description)).toBeTruthy()
  expect(screen.getByRole('heading', { name: notice.title })).toBeTruthy()
  expect(screen.getByText('1 000 000 NOK')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: /Konkurransegrunnlag/ }))
  expect(screen.getByRole('link', { name: 'Åpne dokument' }).getAttribute('href')).toBe('https://example.com/docs')
  expect(screen.getAllByRole('link')).toHaveLength(1)
  fireEvent.click(screen.getByRole('button', { name: 'Kvalifikasjonskrav' }))
  expect(screen.getByText('Tre relevante oppdrag.')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: /Delkontrakter/ }))
  expect(screen.getByRole('heading', { name: 'Drift' })).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Kilde og sporbarhet' }))
  expect(screen.getByText(detail.evidence.sha256)).toBeTruthy()
  fireEvent.click(screen.getAllByRole('button', { name: 'Tilbake til anbud' })[0]!)
  expect(anbudDetail.id()).toBeNull()
})
it('shows when a prior information notice plans its call for tenders, and names TED as its source', async () => {
  const pin: TenderMatch = { ...notice, noticeId: 'TED-710374-2026', noticeType: 'pin-only', status: 'planning', deadline: undefined, plannedNotice: '2027-04-13' }
  client.fetchTenderNotice.mockReset().mockResolvedValue({ ...detail, result: pin, evidence: { ...detail.evidence, source: 'ted', noticeId: pin.noticeId } })
  anbudDetail.open(pin)
  render(() => <><TenderDetailSidebar /><TenderDetailView /></>)
  expect(await screen.findByText(/Utlysningen er planlagt/)).toBeTruthy()
  expect(screen.getAllByText('Planlagt utlysning').length).toBeGreaterThanOrEqual(2)
  expect(screen.getByText('2027-04-13')).toBeTruthy()
  expect(screen.getByText('Hentet fra TED')).toBeTruthy()
  expect(screen.queryByText('Hentet fra Doffin')).toBeNull()
})
it('retries a failed read without inventing notice details', async () => {
  client.fetchTenderNotice.mockRejectedValueOnce(new Error('unavailable'))
  render(() => <TenderDetailView />)
  expect(await screen.findByRole('alert')).toBeTruthy()
  expect(screen.queryByText(detail.notice.description)).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
  await waitFor(() => expect(screen.getByText(detail.notice.description)).toBeTruthy())
})
it('shows source evaluation weights, zero tender counts, exclusions and amendments', async () => {
  client.fetchTenderNotice.mockResolvedValue({ ...detail, notice: { ...detail.notice,
    lots: [{ id: 'LOT-1', cpv: [], awardCriteria: [{ type: 'quality', name: 'Kvalitet', weightPercent: 40, description: 'Metode' }] }],
    exclusionGrounds: 1, exclusionCriteria: [{ type: 'exg-fraud', description: 'Kildens avvisningsgrunn' }],
    awards: [{ lot: 'LOT-1', status: 'selec-w', tendersReceived: 0, contractSigned: '2026-10-01' }],
    change: { replaces: 'original-01', reasonCode: 'update-add', reason: 'Fristen er forlenget', sections: ['LOT-1'] },
  } })
  render(() => <><TenderDetailSidebar /><TenderDetailView /></>)
  await screen.findByText(detail.notice.description)
  fireEvent.click(screen.getByRole('button', { name: 'Tildelingskriterier' }))
  expect(screen.getByText('Kvalitet · 40%')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: /Tildeling og kontrakt/ }))
  expect(screen.getByText('0')).toBeTruthy()
  expect(screen.getByText('selec-w')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Kvalifikasjonskrav' }))
  expect(screen.getByText('Kildens avvisningsgrunn')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Endringer' }))
  expect(screen.getByText('Fristen er forlenget')).toBeTruthy()
  expect(screen.getByText('original-01')).toBeTruthy()
})
