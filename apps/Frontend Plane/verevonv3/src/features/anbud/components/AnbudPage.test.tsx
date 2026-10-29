// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/http'
import { resetAnbudState, anbudWorkspace, anbudFilters } from '@/features/anbud/lib/anbud-state'
import AnbudPage from './AnbudPage'

const client = vi.hoisted(() => ({
  searchTenderMatches: vi.fn(),
  searchTenderCatalogue: vi.fn(),
  fetchTenderAudit: vi.fn(),
  fetchTenderNotice: vi.fn(),
  fetchRetenders: vi.fn(),
  readWorkspaceProfile: vi.fn(),
  readWorkspacePipeline: vi.fn(),
  writeWorkspaceProfile: vi.fn(),
  writeWorkspaceStage: vi.fn(),
  suggestWorkspaceProfile: vi.fn(),
  readWorkspaceSeen: vi.fn(),
  markWorkspaceSeen: vi.fn(),
}))

vi.mock('@/shared/api/anbud-client', () => client)
vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org_1781699307874' } }),
}))

const tender = {
  noticeId: '2026-100001',
  title: 'Åpen drift',
  buyer: 'Eksempelkommune',
  cpv: ['72212000'],
  nuts: ['NO081'],
  deadline: { at: '2026-11-30T08:00:00Z', osloDay: '2026-11-30', daysLeft: 20, kind: 'tender', lot: 'LOT-0001' },
  verdict: 'pass',
  band: 'strong',
  gate: [
    {
      rule: 'deadline',
      outcome: 'pass',
      detail: '20 days left (Oslo calendar days)',
      cite: { field: 'deadline', lot: 'LOT-0001' },
      daysLeft: 20,
    },
  ],
  fit: [{ rule: 'cpv', outcome: 'pass', detail: 'CPV 72212000 is within 72212000', cite: { field: 'cpv' } }],
  gateCounts: { pass: 1, conditional: 0, fail: 0, unknown: 0 },
}

const matches = {
  asOf: '2026-11-10T08:00:00Z',
  strictness: 'balanced',
  funnel: {
    scanned: 4,
    unreadable: 1,
    unsupported: 1,
    awards: 1,
    opportunities: 1,
    superseded: 0,
    gate: { pass: 1, conditional: 0, fail: 0 },
    matched: 1,
  },
  gainedByWidening: { wide: 2 },
  results: [tender],
}

function fillCpv(value: string) {
  fireEvent.click(screen.getByRole('button', { name: 'Kundeprofil' }))
  fireEvent.input(screen.getByLabelText(/CPV-koder/), { target: { value } })
}

beforeEach(() => {
  resetAnbudState()
  // These existing cases exercise the explicitly selected follow-up workflow.
  anbudWorkspace.setView('pipeline')
  vi.clearAllMocks()
  client.searchTenderMatches.mockReset().mockResolvedValue({ ...matches, results: [], trackedResults: [] })
  client.fetchTenderNotice.mockResolvedValue({ result: tender, descriptionTruncated: false, notice: { description: 'Kunngjøringens beskrivelse.', documents: [], documentsTotal: 0, selectionCriteria: [], selectionCriteriaTotal: 0, lots: [], lotsTotal: 0, exclusionGrounds: 0, criteriaInDocuments: false }, evidence: { source: 'doffin', noticeId: tender.noticeId, sha256: 'a'.repeat(64), fetchedAt: '2026-11-09T08:00:00Z', bytes: 100 } })
  client.readWorkspaceProfile.mockResolvedValue(null)
  client.readWorkspacePipeline.mockResolvedValue([])
  client.writeWorkspaceProfile.mockResolvedValue(undefined)
  client.writeWorkspaceStage.mockResolvedValue(undefined)
  client.suggestWorkspaceProfile.mockResolvedValue({ applied: false, reason: 'no_wins', suggestion: { orgNumber: '923456783', wins: 0, cpv: [], nuts: [] } })
  client.readWorkspaceSeen.mockResolvedValue('2026-10-01T00:00:00Z')
  client.markWorkspaceSeen.mockResolvedValue('2026-10-28T10:00:00Z')
  client.searchTenderCatalogue.mockImplementation(async (request: { query?: string }) => ({ results: !request.query || `${tender.title} ${tender.buyer}`.toLocaleLowerCase().includes(request.query.toLocaleLowerCase()) ? [{ ...tender, verdict: 'unknown', band: 'none' }] : [], totalMatched: 1, scanned: 1 }))
})

describe('AnbudPage', () => {
  it('loads All tenders without a company profile on a fresh page', async () => {
    resetAnbudState()
    render(() => <AnbudPage section="matches" />)
    expect(screen.getByRole('tab', { name: 'Alle anbud' }).getAttribute('aria-selected')).toBe('true')
    expect(await screen.findByRole('table')).toBeTruthy()
    expect(client.searchTenderCatalogue).toHaveBeenCalled()
    expect(client.searchTenderMatches).not.toHaveBeenCalled()
    expect(screen.queryByText('Dine relevante anbud starter her')).toBeNull()
  })

  it('asks for a profile for automatic matching while loading saved follow-up work', () => {
    render(() => <AnbudPage section="matches" />)

    expect(screen.getByText('Dine relevante anbud starter her')).toBeTruthy()
    expect(screen.queryByLabelText(/CPV-koder/)).toBeNull()
    expect(client.searchTenderMatches).not.toHaveBeenCalled()
  })

  it('searches the profile as typed and shows the matches with the funnel', async () => {
    client.searchTenderMatches.mockResolvedValue(matches)
    render(() => <AnbudPage section="matches" />)

    fillCpv('72212000')
    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))

    expect(await screen.findByText('Åpen drift')).toBeTruthy()
    expect(screen.getByText('Viser 1 av 1 anbud')).toBeTruthy()
    expect(client.searchTenderMatches).toHaveBeenCalledWith(
      expect.objectContaining({ profile: expect.objectContaining({ cpv: ['72212000'] }), strictness: 'balanced', paginate: true }),
      expect.any(AbortSignal),
    )
  })

  it('says when the search is busy', async () => {
    client.searchTenderMatches.mockRejectedValue(new ApiError('too many scans are running', 429, 'rate_limited'))
    render(() => <AnbudPage section="matches" />)

    fillCpv('72212000')
    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))

    expect(await screen.findByText('Søket er opptatt akkurat nå. Prøv igjen om litt.')).toBeTruthy()
  })

  it('moving the slider searches again at the new step', async () => {
    client.searchTenderMatches.mockResolvedValue(matches)
    render(() => <AnbudPage section="matches" />)

    fillCpv('72212000')
    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))
    await screen.findByText('Åpen drift')
    fireEvent.click(screen.getByRole('button', { name: 'Kundeprofil' }))

    fireEvent.click(screen.getByRole('button', { name: 'Bredt' }))

    await waitFor(() => expect(client.searchTenderMatches).toHaveBeenCalledTimes(2))
    expect(client.searchTenderMatches.mock.calls[1]?.[0]).toMatchObject({ strictness: 'wide' })
  })

  it('opens the audit sheet of a tender with its checks and where its text came from', async () => {
    client.searchTenderMatches.mockResolvedValue(matches)
    client.fetchTenderAudit.mockResolvedValue({
      result: tender,
      evidence: { source: 'doffin', noticeId: '2026-100001', sha256: 'aaaaaaaaaaaa', fetchedAt: '2026-11-09T08:00:00Z', bytes: 3839 },
      parse: {
        root: 'ContractNotice',
        sdkVersion: '1.14',
        flavour: 'eu',
        contactPoints: 1,
        issuesTotal: 0,
        unread: ['ContractNotice/RegulatoryDomain'],
        unreadTotal: 1,
      },
    })
    render(() => <AnbudPage section="matches" />)

    fillCpv('72212000')
    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Åpne anbud' }))

    await screen.findByText('Kunngjøringens beskrivelse.')
    fireEvent.change(screen.getByRole('combobox', { name: 'Anbudinnhold' }), { target: { value: 'assessment' } })
    expect(await screen.findByRole('heading', { name: 'Krav' })).toBeTruthy()
    expect(screen.getByText('20 days left (Oslo calendar days)')).toBeTruthy()
    expect(screen.getByText('doffin, hentet 2026-11-09')).toBeTruthy()
    expect(client.fetchTenderAudit).toHaveBeenCalledWith(
      '2026-100001',
      expect.objectContaining({ strictness: 'balanced' }),
      expect.any(AbortSignal),
    )
  })

  it('shows the radar with how many contracts match, and marks the near ones', async () => {
    client.fetchRetenders.mockResolvedValue({
      asOf: '2026-11-10T08:00:00Z',
      window: { from: '2026-11-10', to: '2027-11-10' },
      strictness: 'balanced',
      matching: 65,
      scanned: 4,
      awards: 1,
      winners: 'not_configured',
      widening: [{ to: 'wide', adds: 10, dials: ['cpv_division'] }],
      results: [
        {
          noticeId: '2025-300002',
          title: 'Drift',
          buyer: 'Kommune',
          endBasis: 'stated',
          contractEnd: '2027-06-01',
          winners: [],
          reach: 'near',
        },
      ],
    })
    render(() => <AnbudPage section="radar" />)

    fillCpv('72212000')
    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))

    expect(await screen.findByText('Viser 1 av 65 kontrakter')).toBeTruthy()
    expect(screen.getByText('+10 ved Bredt')).toBeTruthy()
    expect(screen.getByText('CPV: samme divisjon (to første siffer)')).toBeTruthy()
    expect(screen.getByText('Nær')).toBeTruthy()
  })
  it('switches to all results and filters the table by buyer', async () => {
    client.searchTenderMatches.mockResolvedValue(matches)
    render(() => <AnbudPage section="matches" />)
    fillCpv('72212000')
    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))
    await screen.findByText('Åpen drift')
    fireEvent.click(screen.getByRole('tab', { name: 'Alle anbud' }))
    await waitFor(() => expect(client.searchTenderCatalogue).toHaveBeenCalled())
    fireEvent.input(screen.getByRole('textbox', { name: 'Søk i anbud' }), { target: { value: 'ukjent' } })
    await waitFor(() => expect(screen.queryByText('Åpen drift')).toBeNull())
    fireEvent.input(screen.getByRole('textbox', { name: 'Søk i anbud' }), { target: { value: 'Eksempelkommune' } })
    expect(await screen.findByText('Åpen drift')).toBeTruthy()
  })

  it('moves a tender between session-only pipeline stages', async () => {
    client.searchTenderMatches.mockResolvedValue(matches)
    render(() => <AnbudPage section="matches" />)
    fillCpv('72212000')
    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))
    await screen.findByText('Åpen drift')
    fireEvent.change(screen.getByRole('combobox', { name: 'Status for Åpen drift' }), { target: { value: 'review' } })
    expect(screen.queryByText('Åpen drift')).toBeNull()
    anbudWorkspace.setStage('review')
    expect(screen.getByText('Åpen drift')).toBeTruthy()
  })

  it('uses the Support-style tablist without an internal sidebar', () => {
    const { container } = render(() => <AnbudPage section="matches" />)
    const tab = screen.getByRole('tab', { name: 'Oppfølging' })
    expect(screen.getByRole('tablist', { name: 'Anbudvisning' }).classList.contains('verevon-support-switcher')).toBe(true)
    expect(container.querySelector('aside')).toBeNull()
    fireEvent.keyDown(tab, { key: 'Home' })
    expect(screen.getByRole('tab', { name: 'Alle anbud' }).getAttribute('aria-selected')).toBe('true')
  })

  it('switches between cards and a table and hides optional columns', async () => {
    client.searchTenderMatches.mockResolvedValue(matches)
    render(() => <AnbudPage section="matches" />)
    fillCpv('72212000')
    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))
    await screen.findByText('Åpen drift')
    fireEvent.click(screen.getByRole('tab', { name: 'Alle anbud' }))
    expect(await screen.findByRole('table')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Kolonner' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'Innkjøper' }))
    expect(screen.queryByRole('columnheader', { name: 'Innkjøper' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Kort' }))
    expect(screen.queryByRole('table')).toBeNull()
    expect(screen.getByRole('button', { name: 'Åpne anbud' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Liste' }))
    expect(await screen.findByRole('table')).toBeTruthy()
  })

  it('applies shared buyer, geography, CPV and deadline filters to results', async () => {
    client.searchTenderMatches.mockResolvedValue(matches)
    render(() => <AnbudPage section="matches" />)
    fillCpv('72212000')
    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))
    await screen.findByText('Åpen drift')
    anbudFilters.setFilters(prev => ({ ...prev, buyer: 'annen' }))
    expect(screen.queryByText('Åpen drift')).toBeNull()
    anbudFilters.setFilters(prev => ({ ...prev, buyer: 'Eksempel', cpv: '722', region: 'NO08' }))
    expect(screen.getByText('Åpen drift')).toBeTruthy()
    anbudFilters.setFilters(prev => ({ ...prev, deadline: 'soon' }))
    expect(screen.queryByText('Åpen drift')).toBeNull()
    anbudFilters.reset()
    expect(screen.getByText('Åpen drift')).toBeTruthy()
  })

  it('loads the catalogue on Alle anbud without submitting a profile', async () => {
    render(() => <AnbudPage section="matches" />)
    fireEvent.click(screen.getByRole('tab', { name: 'Alle anbud' }))
    expect(await screen.findByText('Åpen drift')).toBeTruthy()
    expect(client.searchTenderCatalogue).toHaveBeenCalledWith(expect.not.objectContaining({ profile: expect.anything() }), expect.any(AbortSignal))
    expect(client.searchTenderMatches).not.toHaveBeenCalled()
    expect(screen.queryByText('Dine relevante anbud starter her')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Kolonner' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'Match' }))
    expect(screen.getByText('Ikke vurdert')).toBeTruthy()
  })

  it('sorts the whole catalogue from a header in both directions', async () => {
    render(() => <AnbudPage section="matches" />)
    fireEvent.click(screen.getByRole('tab', { name: 'Alle anbud' }))
    await screen.findByRole('table')
    fireEvent.click(screen.getByRole('button', { name: 'Sorter etter Innkjøper' }))
    await waitFor(() => expect(client.searchTenderCatalogue.mock.calls.at(-1)?.[0]).toMatchObject({ sort: 'buyer', direction: 'asc' }))
    fireEvent.click(screen.getByRole('button', { name: 'Sorter etter Innkjøper' }))
    await waitFor(() => expect(client.searchTenderCatalogue.mock.calls.at(-1)?.[0]).toMatchObject({ sort: 'buyer', direction: 'desc' }))
    expect(screen.getByRole('columnheader', { name: 'Innkjøper' }).getAttribute('aria-sort')).toBe('descending')
  })

})

describe('AnbudPage saved workspace', () => {
  it('searches with the saved company profile and shows each tender at its saved stage', async () => {
    client.readWorkspaceProfile.mockResolvedValue({ profile: { cpv: ['72212000'], nuts: [], keywords: [], strictness: 'balanced' }, updatedAt: '2026-11-09T08:00:00Z' })
    client.readWorkspacePipeline.mockResolvedValue([{ noticeId: tender.noticeId, stage: 'review', updatedAt: '2026-11-09T08:00:00Z' }])
    client.searchTenderMatches.mockResolvedValue(matches)
    anbudWorkspace.setStage('review')
    render(() => <AnbudPage section="matches" />)
    const stageSelect = (await screen.findByRole('combobox', { name: `Status for ${tender.title}` })) as HTMLSelectElement
    expect(stageSelect.value).toBe('review')
    expect(client.searchTenderMatches).toHaveBeenCalledWith(expect.objectContaining({ profile: expect.objectContaining({ cpv: ['72212000'] }) }), expect.anything())
  })
})

describe('AnbudPage new tenders', () => {
  it('counts the matched tenders published since the last visit, and marks them seen', async () => {
    client.readWorkspaceProfile.mockResolvedValue({ profile: { cpv: ['72212000'], nuts: [], keywords: [], strictness: 'balanced', confirmed: true }, updatedAt: '2026-11-09T08:00:00Z' })
    client.readWorkspaceSeen.mockResolvedValue('2026-10-27T09:00:00Z')
    client.searchTenderMatches.mockResolvedValue({ ...matches, results: [{ ...tender, issuedAt: '2026-10-28T08:00:00Z' }] })
    render(() => <AnbudPage section="matches" />)
    expect(await screen.findByText(/1 nye anbud siden 2026-10-27/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Marker som sett' }))
    await waitFor(() => expect(screen.queryByText(/nye anbud siden/)).toBeNull())
    expect(client.markWorkspaceSeen).toHaveBeenCalledTimes(1)
  })
})

it('keeps rows visible during refresh and restores cached searches immediately', async () => {
  resetAnbudState()
  let answer: ((value: unknown) => void) | undefined
  client.searchTenderCatalogue.mockResolvedValueOnce({ results: [tender], scanned: 1, totalMatched: 1 })
    .mockImplementationOnce(() => new Promise(resolve => { answer = resolve }))
  render(() => <AnbudPage section="matches" />)
  await screen.findByRole('table')
  fireEvent.input(screen.getByRole('textbox', { name: 'Søk i anbud' }), { target: { value: 'renhold' } })
  await waitFor(() => expect(answer).toBeDefined())
  expect(screen.getByRole('table')).toBeTruthy()
  expect(screen.getByText('Åpen drift')).toBeTruthy()
  expect(screen.getByText('Oppdaterer treff…')).toBeTruthy()
  answer!({ results: [{ ...tender, title: 'Renhold' }], scanned: 1, totalMatched: 1 })
  await screen.findByText('Renhold')
  client.searchTenderCatalogue.mockImplementationOnce(() => new Promise(() => {}))
  fireEvent.input(screen.getByRole('textbox', { name: 'Søk i anbud' }), { target: { value: '' } })
  await screen.findByText('Åpen drift')
  expect(screen.getByText('Oppdaterer treff…')).toBeTruthy()
})

it('keeps expired mismatches visible in a saved stage without a company profile', async () => {
  client.readWorkspacePipeline.mockResolvedValue([{ noticeId: tender.noticeId, stage: 'working' }])
  client.searchTenderMatches.mockResolvedValue({ ...matches, results: [], trackedResults: [{ ...tender, retained: true, verdict: 'fail', band: 'none', deadline: { ...tender.deadline, daysLeft: -5 } }] })
  anbudWorkspace.setStage('working')
  render(() => <AnbudPage section="matches" />)
  expect(await screen.findByText('Åpen drift')).toBeTruthy()
  expect(screen.getByText('Beholdt i oppfølging')).toBeTruthy()
  expect(screen.getByText('Kan ikke by')).toBeTruthy()
  expect(client.searchTenderMatches).toHaveBeenCalledWith(expect.objectContaining({ trackedOnly: true }), expect.any(AbortSignal))
})

it('preserves personalised server ranking until the user chooses deadline sorting', async () => {
  client.readWorkspaceProfile.mockResolvedValue({ profile: { cpv: ['72000000'], nuts: [], keywords: [], strictness: 'balanced', confirmed: true } })
  client.searchTenderMatches.mockResolvedValue({ ...matches, results: [
    { ...tender, noticeId: '2026-100002', title: 'Relevant for arbeidsrollen', deadline: { ...tender.deadline, daysLeft: 30 } },
    { ...tender, noticeId: '2026-100003', title: 'Nærmeste frist', deadline: { ...tender.deadline, daysLeft: 2 } },
  ] })
  const page = render(() => <AnbudPage section="matches" />)
  await screen.findByText('Relevant for arbeidsrollen')
  expect(page.container.querySelector('.anbud-card h3')?.textContent).toBe('Relevant for arbeidsrollen')
  fireEvent.change(screen.getByRole('combobox', { name: 'Sorter anbud' }), { target: { value: 'deadline' } })
  expect(page.container.querySelector('.anbud-card h3')?.textContent).toBe('Nærmeste frist')
})

it('pages through matching results, prefetches and returns to the previous page', async () => {
  client.readWorkspaceProfile.mockResolvedValue({ profile: { cpv: ['72000000'], nuts: [], keywords: [], strictness: 'balanced', confirmed: true } })
  const first = { ...matches, results: [tender], page: 1, pageSize: 1, totalMatched: 2, inboxTotal: 2, nextCursor: 'page-two', previousCursor: '' }
  const second = { ...first, results: [{ ...tender, noticeId: '2026-100002', title: 'Eldre relevant anbud' }], page: 2, nextCursor: '', previousCursor: 'page-one' }
  client.searchTenderMatches.mockImplementation(async request => request.cursor === 'page-two' ? second : first)
  render(() => <AnbudPage section="matches" />)
  await screen.findAllByText('Side 1 av 2')
  await waitFor(() => expect(client.searchTenderMatches).toHaveBeenCalledWith(expect.objectContaining({ cursor: 'page-two' }), expect.any(AbortSignal)))
  fireEvent.click(screen.getAllByRole('button', { name: 'Neste' })[0]!)
  await screen.findByText('Eldre relevant anbud')
  await screen.findAllByText('Side 2 av 2')
  await waitFor(() => expect(screen.getAllByRole('button', { name: 'Forrige' })[0]!.hasAttribute('disabled')).toBe(false))
  expect(screen.getAllByRole('button', { name: 'Neste' })[0]!.hasAttribute('disabled')).toBe(true)
  fireEvent.click(screen.getAllByRole('button', { name: 'Forrige' })[0]!)
  await screen.findAllByText('Side 1 av 2')
  expect(screen.getByText('Åpen drift')).toBeTruthy()
})

it('restarts expired matching snapshots on page one with an explanation', async () => {
  client.readWorkspaceProfile.mockResolvedValue({ profile: { cpv: ['72000000'], nuts: [], keywords: [], strictness: 'balanced', confirmed: true } })
  let fresh = 0
  client.searchTenderMatches.mockImplementation(async request => {
    if (request.cursor) throw new ApiError('changed', 409, 'matching_snapshot_expired')
    fresh++
    return { ...matches, results: [tender], page: 1, pageSize: 1, totalMatched: 2, inboxTotal: 2, nextCursor: fresh === 1 ? 'expired' : '' }
  })
  render(() => <AnbudPage section="matches" />)
  await screen.findAllByText('Side 1 av 2')
  fireEvent.click(screen.getAllByRole('button', { name: 'Neste' })[0]!)
  await screen.findByText('Treffene er oppdatert. Du er tilbake på første side.')
  await waitFor(() => expect(fresh).toBeGreaterThan(1))
  expect(screen.getByText('Åpen drift')).toBeTruthy()
})

it('sends inbox filters to the server and resets pagination for a new search', async () => {
  client.readWorkspaceProfile.mockResolvedValue({ profile: { cpv: ['72000000'], nuts: [], keywords: [], strictness: 'balanced', confirmed: true } })
  client.searchTenderMatches.mockResolvedValue({ ...matches, results: [tender], page: 1, pageSize: 50, totalMatched: 100, inboxTotal: 100, nextCursor: 'later-page' })
  render(() => <AnbudPage section="matches" />)
  await screen.findAllByText('Side 1 av 2')
  fireEvent.input(screen.getByRole('textbox', { name: 'Søk i anbud' }), { target: { value: 'older opportunity' } })
  await waitFor(() => expect(client.searchTenderMatches).toHaveBeenCalledWith(expect.objectContaining({ cursor: undefined, filters: expect.objectContaining({ query: 'older opportunity' }) }), expect.any(AbortSignal)))
})
