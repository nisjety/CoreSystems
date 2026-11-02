// @vitest-environment jsdom

import { cleanup, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { TrustCenterSection } from '@/features/settings/components/TrustCenterSection'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS05-S1: a non-admin reads the Trust Center through audit-core's content-free org-wide
// tool_action view (meta.scope org_redacted): no user_id, subject, resource or request id on
// any row. The workspace rollup must still count every row, so the redaction never turns the
// workspace summary into "no activity".

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

const REDACTED_ROWS = [
  { id: 1, occurred_at: '2026-10-01T10:00:00Z', ingested_at: '2026-10-01T10:00:01Z', plane: 'model', event: 'tool_action', outcome: 'ok', details: { tool: 'web_search', data_category: 'public', zdr: true, phase: 'complete' } },
  { id: 2, occurred_at: '2026-10-01T10:01:00Z', ingested_at: '2026-10-01T10:01:01Z', plane: 'model', event: 'tool_action', outcome: 'ok', details: { tool: 'lookup', data_category: 'customer', zdr: false, phase: 'complete' } },
  { id: 3, occurred_at: '2026-10-01T10:02:00Z', ingested_at: '2026-10-01T10:02:01Z', plane: 'model', event: 'tool_action', outcome: 'ok', details: { tool: 'lookup', data_category: 'customer', zdr: false } },
]

describe('Trust Center over redacted org-wide tool_action rows', () => {
  beforeEach(() => {
    setSessionUser({ id: 'member_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'member' })
    flush()
  })
  afterEach(() => {
    cleanup()
    clearSession()
    vi.unstubAllGlobals()
  })

  it('asks for the tool_action view with no user_id and aggregates the redacted rows into the rollup', async () => {
    const auditRequests: string[] = []
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path.startsWith('/api/v1/audit')) {
        auditRequests.push(path)
        return json({ data: REDACTED_ROWS, meta: { count: 3, scope: 'org_redacted' }, error: null })
      }
      return json({ error: { code: 'not_found', message: 'not found' } }, 404)
    }))

    render(() => <TrustCenterSection />)

    expect(await screen.findByText(/3\s+registrerte verktøyshandlinger/)).toBeTruthy()
    expect(screen.getByText(/1\s+under Zero Data Retention/)).toBeTruthy()
    expect(screen.getByText(/customer · 2/)).toBeTruthy()
    expect(screen.getByText(/public · 1/)).toBeTruthy()

    expect(auditRequests).toHaveLength(1)
    const query = new URL(auditRequests[0]!, 'http://localhost').searchParams
    expect(query.get('event')).toBe('tool_action')
    expect(query.get('limit')).toBe('200')
    expect(query.has('user_id')).toBe(false)
  })

  it('shows the telemetry-pending copy, not an empty rollup, when the audit read is refused', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path.startsWith('/api/v1/audit')) {
        return json({ error: { code: 'audit_scope_denied', message: 'You can only read your own audit events.' } }, 403)
      }
      return json({ error: { code: 'not_found', message: 'not found' } }, 404)
    }))

    render(() => <TrustCenterSection />)

    expect(await screen.findByText(/Telemetri avventes — ingen AI-verktøyshendelser/)).toBeTruthy()
    expect(screen.queryByText(/registrerte verktøyshandlinger/)).toBeNull()
  })
})
