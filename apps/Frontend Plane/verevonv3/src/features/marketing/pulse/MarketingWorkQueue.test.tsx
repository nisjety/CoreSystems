// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { batch, createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { MarketingWorkQueue } from './MarketingWorkQueue.tsx'

const websiteId = `web_${'1'.repeat(32)}`
const site = 'https://aquatiq.com/'
const taskId = `task_${'2'.repeat(32)}`
const task = (status = 'todo', revision = 1) => ({ id: taskId, websiteId, site, revision, status, createdAt: '2026-10-21T10:00:00Z', updatedAt: '2026-10-21T10:00:00Z', evidence: { kind: 'gsc_opportunity', key: 'water hygiene', opportunityType: 'title_meta', period: { startDate: '2026-10-01', endDate: '2026-10-20' }, clicks: 2, impressions: 200, ctr: 0.01, position: 5.25, observedAt: '2026-10-21T10:00:00Z' } })
const work = (current = task(), events = [{ id: `evt_${'3'.repeat(32)}`, taskId, kind: 'created', status: 'todo', occurredAt: '2026-10-21T10:00:00Z' }]) => ({ websiteId, site, tasks: [current], activity: events, summary: { open: current.status === 'todo' || current.status === 'in_progress' ? 1 : 0, inProgress: current.status === 'in_progress' ? 1 : 0, done: current.status === 'done' ? 1 : 0, dismissed: current.status === 'dismissed' ? 1 : 0, total: 1, completionPercent: current.status === 'dismissed' ? null : current.status === 'done' ? 100 : 0 } })
const response = (data = work()) => new Response(JSON.stringify({ data }))
const mount = (onSnapshot = vi.fn(), onTechnicalRechecked = vi.fn()) => render(() => <I18nProvider><MarketingWorkQueue websiteId={websiteId} site={site} revision={2} refreshKey={0} view="board" onSnapshot={onSnapshot} onTechnicalRechecked={onTechnicalRechecked} /></I18nProvider>)

describe('MarketingWorkQueue', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals() })

  it('renders persisted per-site evidence and real activity instead of demo placeholders', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response())
    vi.stubGlobal('fetch', fetchMock)
    mount()
    expect(await screen.findByText('water hygiene')).toBeTruthy()
    expect(screen.getByText(/Title and meta description|Tittel og metabeskrivelse/)).toBeTruthy()
    expect(screen.getByTestId('pulse-backlog-count').textContent).toBe('1')
    expect(screen.getByTestId('pulse-log-count').textContent).toBe('1')
    expect(screen.getByText(/Added to backlog|Lagt til i oppgavelisten/)).toBeTruthy()
    expect(fetchMock.mock.calls[0]?.[0]).toContain(`/api/v1/visibility/work?websiteId=${websiteId}`)
  })
  it('renders technical evidence, rechecks explicitly and keeps a passed check open for human review', async () => {
    const technical = { ...task(), evidence: { kind: 'pagespeed_check', key: 'canonical', strategy: 'mobile', checkStatus: 'failed', checkScore: 0, observedAt: task().createdAt, lighthouseVersion: '13.0.1' } }
    const verification = { status: 'passed', score: 1, observedAt: '2026-10-22T10:00:00Z', lighthouseVersion: '13.0.1', recordedAt: '2026-10-22T10:01:00Z' }
    const current = { ...work(), tasks: [technical] }
    const verified = { ...current, tasks: [{ ...technical, revision: 2, verification }], activity: [{ ...current.activity[0], id: `evt_${'4'.repeat(32)}`, kind: 'verified', verification }] }
    const report = { site, source: 'google_pagespeed', strategy: 'mobile', status: 'complete', report: { performance: 90, accessibility: 100, bestPractices: 100, seo: 100, lcpMs: 1000, cls: 0, tbtMs: 0, fcpMs: 500, speedIndexMs: 900, fetchTime: verification.observedAt, lighthouseVersion: '13.0.1', checks: [{ id: 'canonical', status: 'passed', score: 1 }] } }
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: current }))).mockResolvedValueOnce(new Response(JSON.stringify({ data: report }))).mockResolvedValueOnce(new Response(JSON.stringify({ data: verified })))
    vi.stubGlobal('fetch', fetchMock)
    const onTechnicalRechecked = vi.fn()
    mount(vi.fn(), onTechnicalRechecked)
    await screen.findByText(/^(Kanonisk adresse|Canonical URL)$/)
    expect(screen.queryByText(/2 klikk|2 clicks/)).toBeNull()
    expect(fetchMock).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: /Kontroller på nytt|Recheck/ }))
    await screen.findByTestId('marketing-work-verification')
    expect(screen.getByText(/Siste kontroll: Bestått|Latest verification: Passed/)).toBeTruthy()
    expect(screen.getByText(/endrer ikke oppgavestatus|does not change task status/)).toBeTruthy()
    expect(screen.getByTestId('pulse-backlog-count').textContent).toBe('1')
    expect(fetchMock.mock.calls[1]?.[0]).toBe('/api/v1/visibility/pagespeed')
    expect(fetchMock.mock.calls[2]?.[0]).toBe(`/api/v1/visibility/work/${taskId}/verify`)
    expect(fetchMock.mock.calls.some((call) => call[1]?.method === 'PATCH')).toBe(false)
    expect(onTechnicalRechecked).toHaveBeenCalledOnce()
  })

  it('shows captured audit provenance and confirms only a previously completed audit without starting provider work', async () => {
    const auditEvidence = { kind: 'site_audit_finding', key: 'missing_title_signal', observedAt: task().createdAt, auditId: `audit_${'4'.repeat(32)}`, findingId: `finding_${'5'.repeat(32)}`, pageUrl: `${site}insights`, severity: 'warning', runId: 'run_01K7WKF1W4A0KWXY74XRXBZ6TT', fingerprint: `blake3:${'a'.repeat(64)}`, relatedUrls: [] }
    const auditTask = { ...task(), evidence: auditEvidence }
    const verification = { status: 'passed', observedAt: '2026-10-22T10:00:00Z', recordedAt: '2026-10-22T10:01:00Z', auditId: `audit_${'6'.repeat(32)}`, findingId: '', pageUrl: auditEvidence.pageUrl, ruleId: auditEvidence.key, runId: auditEvidence.runId, fingerprint: auditEvidence.fingerprint }
    const current = { ...work(), tasks: [auditTask] }
    const verified = { ...current, tasks: [{ ...auditTask, revision: 2, verification }], activity: [{ ...current.activity[0], id: `evt_${'7'.repeat(32)}`, kind: 'verified', verification }] }
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: current }))).mockResolvedValueOnce(new Response(JSON.stringify({ data: verified })))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    await screen.findByText(/^(Sidetittel mangler|Missing page title)$/)
    expect(screen.getByRole('link', { name: auditEvidence.pageUrl }).getAttribute('href')).toBe(auditEvidence.pageUrl)
    expect(screen.getByText(auditEvidence.runId)).toBeTruthy()
    expect(screen.queryByText(/Lighthouse/)).toBeNull()
    expect(screen.queryByText(/2 clicks|2 klikk/)).toBeNull()
    expect(fetchMock).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: /Bekreft siste revisjonskontroll|Confirm latest audit check/ }))
    await screen.findByTestId('marketing-work-verification')
    expect(screen.getByText(/Siste kontroll: Bestått|Latest verification: Passed/)).toBeTruthy()
    expect(screen.getByText(/Kjør en ny nettstedsrevisjon|Run a new website audit/)).toBeTruthy()
    expect(screen.getByTestId('pulse-backlog-count').textContent).toBe('1')
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(fetchMock.mock.calls[1]?.[0]).toBe(`/api/v1/visibility/work/${taskId}/verify`)
    expect(JSON.parse(String(fetchMock.mock.calls[1]?.[1]?.body))).toEqual({ websiteId, site, websiteRevision: 2, revision: 1 })
    expect(fetchMock.mock.calls.some((call) => String(call[0]).includes('/pagespeed') || String(call[0]).endsWith('/audits') || call[1]?.method === 'PATCH')).toBe(false)
  })

  it('explains that a newer completed audit is needed without claiming a passed result on conflict', async () => {
    const auditTask = { ...task(), evidence: { kind: 'site_audit_finding', key: 'http_error', observedAt: task().createdAt, auditId: `audit_${'4'.repeat(32)}`, findingId: `finding_${'5'.repeat(32)}`, pageUrl: site, severity: 'error', runId: 'run_01K7WKF1W4A0KWXY74XRXBZ6TT', fingerprint: `blake3:${'a'.repeat(64)}`, relatedUrls: [] } }
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...work(), tasks: [auditTask] } }))).mockResolvedValueOnce(new Response('{}', { status: 409 }))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    fireEvent.click(await screen.findByRole('button', { name: /Bekreft siste revisjonskontroll|Confirm latest audit check/ }))
    const error = await screen.findByRole('alert')
    expect(error.textContent).toMatch(/nyere.*revisjon|newer.*audit/)
    expect(screen.queryByTestId('marketing-work-verification')).toBeNull()
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('updates task status with current revisions and displays returned activity', async () => {
    const events = [
      { id: `evt_${'3'.repeat(32)}`, taskId, kind: 'created', status: 'todo', occurredAt: '2026-10-21T10:00:00Z' },
      { id: `evt_${'4'.repeat(32)}`, taskId, kind: 'status_changed', status: 'in_progress', occurredAt: '2026-10-21T11:00:00Z' },
    ]
    const fetchMock = vi.fn().mockResolvedValueOnce(response()).mockResolvedValueOnce(new Response(JSON.stringify({ data: work(task('in_progress', 2), events) })))
    const onSnapshot = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    mount(onSnapshot)
    fireEvent.click(await screen.findByRole('button', { name: /Start/ }))
    await waitFor(() => expect(screen.getByText(/Status changed|Status endret/)).toBeTruthy())
    expect(fetchMock.mock.calls[1]?.[0]).toBe(`/api/v1/visibility/work/${taskId}`)
    expect(JSON.parse(String(fetchMock.mock.calls[1]?.[1]?.body))).toEqual({ websiteId, site, websiteRevision: 2, revision: 1, status: 'in_progress' })
    expect(onSnapshot).toHaveBeenCalled()
  })

  it('asks to reload after an unconfirmed recheck response instead of claiming nothing was saved', async () => {
    const technical = { ...task(), evidence: { kind: 'pagespeed_check', key: 'canonical', strategy: 'mobile', checkStatus: 'failed', checkScore: 0, observedAt: task().createdAt, lighthouseVersion: '13.0.1' } }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...work(), tasks: [technical] } }))).mockRejectedValueOnce(new Error('network unavailable')))
    mount()
    fireEvent.click(await screen.findByRole('button', { name: /Kontroller på nytt|Recheck/ }))
    await screen.findByRole('alert')
    expect(screen.getByRole('alert').textContent).toMatch(/kunne ikke bekreftes|could not be confirmed/)
    expect(screen.getByRole('alert').textContent).not.toMatch(/Ingen kontroll.*lagret|No verification.*saved/)
    expect(screen.getByTestId('pulse-backlog-count').textContent).toBe('1')
  })

  it('preserves null completion for an empty or dismissed-only queue', async () => {
    const empty = { websiteId, site, tasks: [], activity: [], summary: { open: 0, inProgress: 0, done: 0, dismissed: 0, total: 0, completionPercent: null } }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response(empty)))
    mount()
    await screen.findByText(/No prioritized work yet|Ingen prioriterte oppgaver ennå/)
    expect(screen.getByTestId('pulse-backlog-count').textContent).toBe('0')
    expect(screen.getByTestId('pulse-log-count').textContent).toBe('0')
    expect(screen.getByText(/No prioritized work yet|Ingen prioriterte oppgaver ennå/)).toBeTruthy()
  })

  it('can show and reopen completed work with the current task revision', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(response(work(task('done', 2)))).mockResolvedValueOnce(response(work(task('todo', 3))))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    await screen.findByText(/No prioritized work yet|Ingen prioriterte oppgaver ennå/)
    fireEvent.click(screen.getByRole('checkbox', { name: /Show closed|Vis avsluttede/ }))
    fireEvent.click(await screen.findByRole('button', { name: /Back to open|Tilbake til åpen/ }))
    await waitFor(() => expect(screen.getByTestId('pulse-backlog-count').textContent).toBe('1'))
    expect(JSON.parse(String(fetchMock.mock.calls[1]?.[1]?.body))).toMatchObject({ revision: 2, status: 'todo' })
  })

  it('lets the user retry a failed work projection without inventing a count', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce(response()))
    mount()
    const retry = await screen.findByRole('button', { name: /Reload work queue|Last arbeidskø på nytt/ })
    expect(screen.getByTestId('pulse-backlog-count').textContent).toBe('—')
    fireEvent.click(retry)
    await screen.findByText('water hygiene')
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('clears old site work while loading and ignores stale responses', async () => {
    let resolveOld: ((value: Response) => void) | undefined
    const otherWebsiteId = `web_${'5'.repeat(32)}`
    const fetchMock = vi.fn().mockResolvedValueOnce(response()).mockReturnValueOnce(new Promise<Response>((resolve) => { resolveOld = resolve })).mockResolvedValueOnce(new Response(JSON.stringify({ data: { websiteId: otherWebsiteId, site: 'https://other.example/', tasks: [], activity: [], summary: { open: 0, inProgress: 0, done: 0, dismissed: 0, total: 0, completionPercent: null } } })))
    vi.stubGlobal('fetch', fetchMock)
    const [selectedSite, setSelectedSite] = createSignal(site)
    const [selectedWebsiteId, setSelectedWebsiteId] = createSignal(websiteId)
    const [refreshKey, setRefreshKey] = createSignal(0)
    const view = render(() => <I18nProvider><MarketingWorkQueue websiteId={selectedWebsiteId()} site={selectedSite()} revision={2} refreshKey={refreshKey()} view="board" onSnapshot={() => undefined} /></I18nProvider>)
    await screen.findByText('water hygiene')
    setRefreshKey(1)
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    batch(() => {
      setSelectedSite('https://other.example/')
      setSelectedWebsiteId(otherWebsiteId)
    })
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3))
    expect(view.container.textContent).not.toContain('water hygiene')
    resolveOld?.(response())
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(view.container.textContent).not.toContain('water hygiene')
  })
})
