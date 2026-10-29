import { afterEach, describe, expect, it, vi } from 'vitest'
import { isMarketingWorkSnapshot, loadMarketingWork, saveMarketingOpportunity, saveMarketingTechnical, saveMarketingAuditFinding, verifyMarketingTask, updateMarketingTask, sanitizeMarketingWorkSnapshot } from './marketing-work-client.ts'

const site = 'https://aquatiq.com/'
const websiteId = `web_${'1'.repeat(32)}`
const evidence = { kind: 'gsc_opportunity', key: 'water hygiene', opportunityType: 'title_meta', period: { startDate: '2026-10-01', endDate: '2026-10-20' }, clicks: 2, impressions: 200, ctr: 0.01, position: 5.25, observedAt: '2026-10-21T10:00:00Z' }
const task = { id: `task_${'2'.repeat(32)}`, websiteId, site, revision: 1, status: 'todo', createdAt: '2026-10-21T10:00:00Z', updatedAt: '2026-10-21T10:00:00Z', evidence }
const snapshot = () => ({ websiteId, site, tasks: [task], activity: [{ id: `evt_${'3'.repeat(32)}`, taskId: task.id, kind: 'created', status: 'todo', occurredAt: task.createdAt }], summary: { open: 1, inProgress: 0, done: 0, dismissed: 0, total: 1, completionPercent: 0 } })
const response = (data = snapshot(), status = 200) => new Response(JSON.stringify({ data }), { status })

describe('marketing work queue contract', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('validates and reconstructs bounded task, evidence, activity and summary projections', () => {
    const input = { ...snapshot(), token: 'secret', tasks: [{ ...task, evidence: { ...evidence, providerPayload: 'secret' } }] }
    expect(isMarketingWorkSnapshot(input)).toBe(true)
    const safe = sanitizeMarketingWorkSnapshot(input)
    expect(safe.tasks[0]?.evidence).not.toHaveProperty('providerPayload')
    expect(safe).not.toHaveProperty('token')
  })

  it('projects technical observations and independent verification without changing task status', () => {
    const verification = { status: 'passed', score: 1, observedAt: '2026-10-22T10:00:00Z', lighthouseVersion: '13.0.1', recordedAt: '2026-10-22T10:01:00Z', token: 'secret' }
    const technical = { ...task, evidence: { kind: 'pagespeed_check', key: 'canonical', strategy: 'mobile', checkStatus: 'failed', checkScore: 0, observedAt: task.createdAt, lighthouseVersion: '13.0.1', token: 'secret' }, verification }
    const input = { ...snapshot(), tasks: [technical], activity: [{ ...snapshot().activity[0], kind: 'verified', verification }] }
    expect(isMarketingWorkSnapshot(input)).toBe(true)
    const safe = sanitizeMarketingWorkSnapshot(input)
    expect(safe.tasks[0]?.status).toBe('todo')
    expect(safe.tasks[0]?.evidence).not.toHaveProperty('token')
    expect(safe.tasks[0]?.verification).not.toHaveProperty('token')
    expect(safe.activity[0]?.verification).not.toHaveProperty('token')
    for (const patch of [{ key: 'arbitrary' }, { checkStatus: 'passed', checkScore: 1 }, { checkScore: 1 }, { strategy: 'tablet' }, { strategy: ['mobile'] }, { checkStatus: ['manual'], checkScore: null }]) {
      expect(isMarketingWorkSnapshot({ ...input, tasks: [{ ...technical, evidence: { ...technical.evidence, ...patch } }] })).toBe(false)
    }
    expect(isMarketingWorkSnapshot({ ...input, tasks: [{ ...technical, verification: { ...verification, score: 0 } }] })).toBe(false)
    expect(isMarketingWorkSnapshot({ ...input, tasks: [{ ...technical, evidence: { ...technical.evidence, clicks: 2 } }] })).toBe(false)
    expect(isMarketingWorkSnapshot({ ...snapshot(), tasks: [{ ...task, evidence: { ...evidence, strategy: 'mobile' } }] })).toBe(false)
    expect(isMarketingWorkSnapshot({ ...input, tasks: [{ ...technical, verification: { ...verification, observedAt: task.createdAt } }] })).toBe(false)
    expect(isMarketingWorkSnapshot({ ...input, tasks: [{ ...technical, verification: { ...verification, recordedAt: '2026-10-21T00:00:00Z' } }] })).toBe(false)
    expect(isMarketingWorkSnapshot({ ...input, tasks: [{ ...technical, verification: { ...verification, observedAt: '2026-10-21T10:00:00.000000001Z' } }] })).toBe(true)
    expect(isMarketingWorkSnapshot({ ...input, activity: [{ ...input.activity[0], verification: undefined }] })).toBe(false)
    expect(isMarketingWorkSnapshot({ ...input, activity: [{ ...input.activity[0], kind: ['verified'] }] })).toBe(false)
    expect(isMarketingWorkSnapshot({ ...snapshot(), tasks: [{ ...task, verification }] })).toBe(false)
  })

  it('projects only scoped audit evidence and newer verified checks without invented metrics', () => {
    const auditEvidence = { kind: 'site_audit_finding', key: 'missing_title_signal', observedAt: task.createdAt, auditId: `audit_${'4'.repeat(32)}`, findingId: `finding_${'5'.repeat(32)}`, pageUrl: `${site}insights`, severity: 'warning', runId: 'run_01K7WKF1W4A0KWXY74XRXBZ6TT', fingerprint: `blake3:${'a'.repeat(64)}`, relatedUrls: [`${site}other`], token: 'secret' }
    const verification = { status: 'passed', observedAt: '2026-10-22T10:00:00Z', recordedAt: '2026-10-22T10:01:00Z', auditId: `audit_${'6'.repeat(32)}`, findingId: '', pageUrl: auditEvidence.pageUrl, ruleId: auditEvidence.key, runId: auditEvidence.runId, fingerprint: auditEvidence.fingerprint, token: 'secret' }
    const auditTask = { ...task, evidence: auditEvidence, verification }
    const input = { ...snapshot(), tasks: [auditTask], activity: [{ ...snapshot().activity[0], kind: 'verified', verification }] }
    expect(isMarketingWorkSnapshot(input)).toBe(true)
    const safe = sanitizeMarketingWorkSnapshot(input)
    expect(safe.tasks[0]?.status).toBe('todo')
    expect(safe.tasks[0]?.evidence).not.toHaveProperty('token')
    expect(safe.tasks[0]?.evidence).not.toHaveProperty('clicks')
    expect(safe.tasks[0]?.verification).not.toHaveProperty('score')
    expect(safe.activity[0]?.verification).not.toHaveProperty('token')
    expect(safe.tasks[0]?.evidence).toMatchObject({ auditId: auditEvidence.auditId, pageUrl: auditEvidence.pageUrl, relatedUrls: auditEvidence.relatedUrls })
    for (const patch of [{ auditId: 'bad' }, { findingId: 'bad' }, { key: 'unknown_rule' }, { fingerprint: 'invented' }, { runId: 'run_fake' }, { pageUrl: 'https://other.no/' }, { pageUrl: `${site}admin` }, { pageUrl: `${site}?token=secret` }, { severity: 'critical' }, { relatedUrls: ['https://other.no/'] }, { clicks: 0 }, { checkScore: 0 }]) {
      expect(isMarketingWorkSnapshot({ ...input, tasks: [{ ...auditTask, evidence: { ...auditEvidence, ...patch } }] })).toBe(false)
    }
    for (const patch of [{ auditId: auditEvidence.auditId }, { pageUrl: `${site}other` }, { ruleId: 'http_error' }, { score: 1 }, { status: 'failed', findingId: '' }, { status: 'missing' }, { observedAt: task.createdAt }, { recordedAt: task.createdAt }]) {
      expect(isMarketingWorkSnapshot({ ...input, tasks: [{ ...auditTask, verification: { ...verification, ...patch } }] })).toBe(false)
    }
    expect(isMarketingWorkSnapshot({ ...input, activity: [{ ...input.activity[0], verification: { ...verification, pageUrl: `${site}other` } }] })).toBe(false)
    if (safe.tasks[0]?.evidence.kind === 'site_audit_finding') safe.tasks[0].evidence.relatedUrls[0] = `${site}changed`
    expect(auditEvidence.relatedUrls).toEqual([`${site}other`])
  })

  it('saves an audit finding using references only and rejects invalid identities before network access', async () => {
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(response()))
    vi.stubGlobal('fetch', fetchMock)
    const input = { websiteId, site, websiteRevision: 2, auditId: `audit_${'4'.repeat(32)}`, findingId: `finding_${'5'.repeat(32)}` }
    await saveMarketingAuditFinding({ ...input, evidence: 'untrusted' } as typeof input)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/visibility/work/audit')
    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toEqual(input)
    await expect(saveMarketingAuditFinding({ ...input, auditId: 'invalid' })).rejects.toThrow()
    await expect(saveMarketingAuditFinding({ ...input, findingId: 'invalid' })).rejects.toThrow()
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('saves and verifies only authoritative technical identities, not browser metrics', async () => {
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(response()))
    vi.stubGlobal('fetch', fetchMock)
    await saveMarketingTechnical({ websiteId, site, websiteRevision: 2, strategy: 'mobile', checkId: 'canonical', fetchTime: task.createdAt })
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/visibility/work/technical')
    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toEqual({ websiteId, site, websiteRevision: 2, strategy: 'mobile', checkId: 'canonical', fetchTime: task.createdAt })
    await verifyMarketingTask({ websiteId, site, websiteRevision: 2, taskId: task.id, revision: 1 })
    expect(fetchMock.mock.calls[1]?.[0]).toBe(`/api/v1/visibility/work/${task.id}/verify`)
    expect(JSON.parse(String(fetchMock.mock.calls[1]?.[1]?.body))).toEqual({ websiteId, site, websiteRevision: 2, revision: 1 })
  })

  it.each([
    { tasks: [{ ...task, revision: 0 }] },
    { tasks: [{ ...task, site: 'https://other.example/' }] },
    { tasks: [{ ...task, status: 'running' }] },
    { tasks: [{ ...task, evidence: { ...evidence, ctr: 0.2 } }] },
    { tasks: [{ ...task, evidence: { ...evidence, period: { startDate: '2026-02-30', endDate: '2026-03-01' } } }] },
    { activity: [{ id: `evt_${'3'.repeat(32)}`, taskId: `task_${'9'.repeat(32)}`, kind: 'created', status: 'todo', occurredAt: task.createdAt }] },
    { summary: { open: 0, inProgress: 0, done: 0, dismissed: 0, total: 2, completionPercent: 0 } },
    { summary: { open: 0, inProgress: 0, done: 0, dismissed: 0, total: 0, completionPercent: 0 } },
  ])('rejects malformed or inconsistent snapshot %j', (patch) => {
    expect(isMarketingWorkSnapshot({ ...snapshot(), ...patch })).toBe(false)
  })

  it('loads only the selected registered website context', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response())
    vi.stubGlobal('fetch', fetchMock)
    expect(await loadMarketingWork(websiteId, site)).toEqual(snapshot())
    expect(fetchMock.mock.calls[0]?.[0]).toBe(`/api/v1/visibility/work?websiteId=${websiteId}&site=${encodeURIComponent(site)}`)
  })

  it('sends only the explicit finding identity when saving a finding', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response())
    vi.stubGlobal('fetch', fetchMock)
    await saveMarketingOpportunity({ websiteId, site, websiteRevision: 2, quarter: 'Q4', query: 'water hygiene' })
    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toEqual({ websiteId, site, websiteRevision: 2, quarter: 'Q4', query: 'water hygiene' })
  })

  it('allows a documented opportunity from the current full-year view', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response())
    vi.stubGlobal('fetch', fetchMock)
    await saveMarketingOpportunity({ websiteId, site, websiteRevision: 2, quarter: 'FY', query: 'water hygiene' })
    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toMatchObject({ quarter: 'FY', query: 'water hygiene' })
  })

  it('updates task status with optimistic task revision and selected website revision', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response())
    vi.stubGlobal('fetch', fetchMock)
    await updateMarketingTask({ taskId: task.id, websiteId, site, websiteRevision: 2, revision: 1, status: 'in_progress' })
    expect(fetchMock.mock.calls[0]?.[0]).toBe(`/api/v1/visibility/work/${task.id}`)
    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toEqual({ websiteId, site, websiteRevision: 2, revision: 1, status: 'in_progress' })
  })

  it('rejects stale contexts, malformed envelopes and backend failures', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...snapshot(), site: 'https://other.example/' })))
    await expect(loadMarketingWork(websiteId, site)).rejects.toThrow()
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{}', { status: 409 })))
    await expect(updateMarketingTask({ taskId: task.id, websiteId, site, websiteRevision: 2, revision: 1, status: 'done' })).rejects.toThrow()
  })
})
