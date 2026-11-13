// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { normalizeApprovalGrant, type ApprovalGrant } from '@/shared/api/orchestration-client'
import { ThreadApprovalGrants, isLiveApprovalGrant } from './ThreadApprovalGrants'

// W2-WS13-S6-spa-grants: the live standing approvals of a thread, each with a one-click Revoke, read and
// revoked only through the BFF routes. Expired and revoked grants are not shown; an empty list renders
// nothing (a zero-retention principal's list is answered empty); a failed read is an alert, not an empty list.

const FUTURE = new Date(Date.now() + 3_600_000).toISOString()
const PAST = new Date(Date.now() - 3_600_000).toISOString()

function wireGrant(overrides: Record<string, unknown> = {}) {
  return {
    id: 'apg_live1',
    thread_id: 'thread-1',
    run_id: null,
    scope: 'thread',
    tool_name: 'mcp__notes__create_note',
    effect_class: 'external_write',
    cause_code: 'capability_high_risk',
    created_at: '2026-11-12T08:00:00Z',
    expires_at: FUTURE,
    revoked_at: null,
    use_count: 2,
    last_used_at: null,
    ...overrides,
  }
}

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('normalizeApprovalGrant', () => {
  it('reads a grant as model-gateway writes it', () => {
    expect(normalizeApprovalGrant(wireGrant({ scope: 'run', run_id: 'run-1' }))).toMatchObject({
      id: 'apg_live1',
      threadId: 'thread-1',
      runId: 'run-1',
      scope: 'run',
      toolName: 'mcp__notes__create_note',
      expiresAt: FUTURE,
      revokedAt: null,
      useCount: 2,
    })
  })

  it.each([
    ['no id', { id: undefined }],
    ['an unknown scope', { scope: 'org' }],
    ['a null scope', { scope: null }],
  ])('drops a row with %s', (_label, overrides) => {
    expect(normalizeApprovalGrant(wireGrant(overrides))).toBeNull()
  })
})

describe('isLiveApprovalGrant', () => {
  const grant = normalizeApprovalGrant(wireGrant()) as ApprovalGrant
  it('is live while not revoked and not expired', () => {
    expect(isLiveApprovalGrant(grant)).toBe(true)
    expect(isLiveApprovalGrant({ ...grant, revokedAt: PAST })).toBe(false)
    expect(isLiveApprovalGrant({ ...grant, expiresAt: PAST })).toBe(false)
    expect(isLiveApprovalGrant({ ...grant, expiresAt: '' })).toBe(false)
    expect(isLiveApprovalGrant({ ...grant, expiresAt: 'not a date' })).toBe(false)
  })
})

describe('ThreadApprovalGrants', () => {
  it('lists only the live grants, read from the thread route', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      json(200, {
        data: {
          grants: [
            wireGrant(),
            wireGrant({ id: 'apg_run1', scope: 'run', run_id: 'run-1', tool_name: 'mail.send' }),
            wireGrant({ id: 'apg_old', tool_name: 'expired.tool', expires_at: PAST }),
            wireGrant({ id: 'apg_gone', tool_name: 'revoked.tool', revoked_at: PAST }),
          ],
        },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)
    render(() => <ThreadApprovalGrants threadId="thread-1" />)

    await screen.findByText('mcp__notes__create_note')
    expect(String(fetchMock.mock.calls[0]![0])).toMatch(/\/api\/v1\/orchestration\/threads\/thread-1\/approval-grants$/)
    expect(screen.getByText('mail.send')).toBeTruthy()
    expect(screen.getByText(/For denne tråden|For this thread/)).toBeTruthy()
    expect(screen.getByText(/For denne kjøringen|For this run/)).toBeTruthy()
    expect(screen.queryByText('expired.tool')).toBeNull()
    expect(screen.queryByText('revoked.tool')).toBeNull()
  })

  it('renders nothing for an empty list (a zero-retention principal is answered empty)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(json(200, { data: { grants: [] } }))
    vi.stubGlobal('fetch', fetchMock)
    const view = render(() => <ThreadApprovalGrants threadId="thread-1" />)
    await waitFor(() => expect(fetchMock).toHaveBeenCalled())
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(view.container.querySelector('section')).toBeNull()
  })

  it('renders nothing when every grant has expired or was revoked', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(json(200, { data: { grants: [wireGrant({ expires_at: PAST })] } })),
    )
    const view = render(() => <ThreadApprovalGrants threadId="thread-1" />)
    await new Promise((resolve) => setTimeout(resolve, 10))
    expect(view.container.querySelector('section')).toBeNull()
  })

  it('revokes a grant by its id with DELETE and reads the list again', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(json(200, { data: { grants: [wireGrant()] } }))
      .mockResolvedValueOnce(json(200, { data: { grant: wireGrant({ revoked_at: new Date().toISOString() }) } }))
      .mockResolvedValueOnce(json(200, { data: { grants: [] } }))
    vi.stubGlobal('fetch', fetchMock)
    render(() => <ThreadApprovalGrants threadId="thread-1" />)

    fireEvent.click(await screen.findByRole('button', { name: /Trekk tilbake|Revoke/ }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3))
    const [url, init] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(String(url)).toMatch(/\/api\/v1\/orchestration\/approval-grants\/apg_live1$/)
    expect(init.method).toBe('DELETE')
    expect(init.body).toBeUndefined()
    await waitFor(() => expect(screen.queryByText('mcp__notes__create_note')).toBeNull())
  })

  it('keeps the grant and says so when the revoke fails', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(json(200, { data: { grants: [wireGrant()] } }))
      .mockResolvedValueOnce(json(503, { error: { code: 'upstream_unavailable', message: 'raw upstream text' } }))
    vi.stubGlobal('fetch', fetchMock)
    render(() => <ThreadApprovalGrants threadId="thread-1" />)

    fireEvent.click(await screen.findByRole('button', { name: /Trekk tilbake|Revoke/ }))

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).not.toContain('raw upstream text')
    expect(screen.getByText('mcp__notes__create_note')).toBeTruthy()
  })

  it('shows a failed read as an alert, not as an empty list', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(json(502, { error: { code: 'upstream_unavailable', message: 'raw upstream text' } })),
    )
    render(() => <ThreadApprovalGrants threadId="thread-1" />)
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBeTruthy()
    expect(alert.textContent).not.toContain('raw upstream text')
  })
})
