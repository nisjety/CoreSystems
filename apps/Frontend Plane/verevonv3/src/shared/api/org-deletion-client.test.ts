import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from './http'
import {
  acknowledgeDeletion,
  getDeletionStatus,
  markExported,
  restoreOrg,
  triggerOrgSoftDelete,
} from './org-deletion-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

describe('org deletion client', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('soft-deletes with the confirm flag and exact org name in the body', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ organization_id: 'org_1' }))
    vi.stubGlobal('fetch', fetchMock)

    await triggerOrgSoftDelete('org_1', true, 'Acme AS')

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/gdpr/soft-delete')
    expect(init.method).toBe('DELETE')
    expect(JSON.parse(String(init.body))).toEqual({ confirm: true, org_name: 'Acme AS' })
  })

  it('surfaces an org-name mismatch as a typed ApiError instead of swallowing it', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse(
          { error: 'organization name confirmation does not match; type the exact organization name to confirm' },
          400,
        ),
      ),
    )

    await expect(triggerOrgSoftDelete('org_1', true, 'Wrong Name')).rejects.toBeInstanceOf(ApiError)
  })

  it('restores a pending-deletion org via POST', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ ok: true }))
    vi.stubGlobal('fetch', fetchMock)

    await restoreOrg('org_1')

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/gdpr/restore')
    expect(init.method).toBe('POST')
  })

  it('marks the calling member as having exported their data', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ ok: true }))
    vi.stubGlobal('fetch', fetchMock)

    await markExported('org_1')

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/gdpr/deletion/mark-exported')
    expect(init.method).toBe('POST')
  })

  it('acknowledges the pending-deletion notice for the calling member', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ ok: true }))
    vi.stubGlobal('fetch', fetchMock)

    await acknowledgeDeletion('org_1')

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/gdpr/deletion/acknowledge')
    expect(init.method).toBe('POST')
  })

  it('reads the pending-deletion status including the deadline and member checkpoint', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          pending: true,
          deadline: '2026-08-19T00:00:00Z',
          org_name: 'Acme AS',
          member_status: { user_id: 'user_1', exported_at: null, acknowledged_at: null },
        }),
      ),
    )

    const status = await getDeletionStatus('org_1')

    const [path] = vi.mocked(fetch).mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/gdpr/deletion/status')
    expect(status.pending).toBe(true)
    expect(status.deadline).toBe('2026-08-19T00:00:00Z')
    expect(status.member_status?.user_id).toBe('user_1')
  })

  it('reports pending:false with no member checkpoint when nothing is scheduled', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => jsonResponse({ pending: false, org_name: 'Acme AS' })),
    )

    const status = await getDeletionStatus('org_1')

    expect(status.pending).toBe(false)
    expect(status.member_status).toBeUndefined()
    expect(status.deadline).toBeUndefined()
  })
})
