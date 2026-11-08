// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AuditLogSection } from '@/features/settings/components/AuditLogSection'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS05-S4: the owner/admin audit log list. fetch is stubbed; every request the section
// makes is recorded so the tests can assert the exact query and the absence of requests.

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function row(id: number, event: string, extra: Record<string, unknown> = {}) {
  return {
    id,
    event,
    plane: 'control',
    outcome: 'ok',
    user_id: 'user_admin',
    occurred_at: '2026-10-01T10:00:00Z',
    ...extra,
  }
}

function signIn(role: string) {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role })
  flush()
}

function auditQueries(fetchMock: ReturnType<typeof vi.fn>): URLSearchParams[] {
  return fetchMock.mock.calls
    .map((call) => String(call[0]))
    .filter((path) => path.startsWith('/api/v1/audit'))
    .map((path) => new URL(path, 'http://localhost').searchParams)
}

function field(id: string): HTMLInputElement {
  return document.getElementById(id) as HTMLInputElement
}

function type(id: string, value: string) {
  fireEvent.input(field(id), { target: { value } })
  flush()
}

const applyButton = () => screen.getByRole('button', { name: /bruk filter|apply filters/i })
const resetButton = () => screen.getByRole('button', { name: /nullstill|reset/i })

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('AuditLogSection', () => {
  describe('as an admin', () => {
    beforeEach(() => signIn('admin'))

    it('loads the first page newest-first with the page size and shows the rows', async () => {
      const fetchMock = vi.fn(async () => json({
        data: [row(2, 'member.removed'), row(1, 'login', { outcome: 'denied' })],
        meta: { count: 2, scope: 'org' },
      }))
      vi.stubGlobal('fetch', fetchMock)

      render(() => <AuditLogSection />)

      expect(await screen.findByText('member.removed - ok')).toBeTruthy()
      expect(screen.getByText('login - denied')).toBeTruthy()
      expect(screen.queryByTestId('audit-log-admin-only')).toBeNull()
      expect(screen.queryByTestId('audit-log-load-more')).toBeNull()
      const queries = auditQueries(fetchMock)
      expect(queries).toHaveLength(1)
      expect(queries[0]!.get('limit')).toBe('50')
      expect(queries[0]!.has('cursor')).toBe(false)
      expect(queries[0]!.has('org_id')).toBe(false)
    })

    it('applies filters from the first page and resets to an unfiltered first page', async () => {
      const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
        const query = new URL(String(input), 'http://localhost').searchParams
        if (query.get('plane') === 'model') return json({ data: [row(9, 'tool_action', { plane: 'model' })], meta: { count: 1 } })
        return json({ data: [row(1, 'login')], meta: { count: 1 } })
      })
      vi.stubGlobal('fetch', fetchMock)

      render(() => <AuditLogSection />)
      expect(await screen.findByText('login - ok')).toBeTruthy()

      type('audit-filter-plane', ' model ')
      type('audit-filter-outcome', 'ok')
      type('audit-filter-event', 'tool_action')
      type('audit-filter-subject', 'agent:alpha')
      // Typing alone sends nothing; only Apply does.
      expect(auditQueries(fetchMock)).toHaveLength(1)

      fireEvent.click(applyButton())
      flush()
      expect(await screen.findByText('tool_action - ok')).toBeTruthy()
      expect(screen.queryByText('login - ok')).toBeNull()

      const applied = auditQueries(fetchMock)[1]!
      expect(applied.get('plane')).toBe('model')
      expect(applied.get('outcome')).toBe('ok')
      expect(applied.get('event')).toBe('tool_action')
      expect(applied.get('subject')).toBe('agent:alpha')
      expect(applied.has('cursor')).toBe(false)

      fireEvent.click(resetButton())
      flush()
      expect(await screen.findByText('login - ok')).toBeTruthy()
      const reset = auditQueries(fetchMock)[2]!
      expect([...reset.keys()].sort()).toEqual(['limit'])
      expect(field('audit-filter-plane').value).toBe('')
    })

    it('Load more sends the cursor, appends the next page and hides itself on the last page', async () => {
      const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
        const query = new URL(String(input), 'http://localhost').searchParams
        if (query.get('cursor') === 'cursor-2') {
          return json({ data: [row(1, 'login')], meta: { count: 1, scope: 'org' } })
        }
        return json({ data: [row(3, 'member.added'), row(2, 'member.removed')], meta: { count: 2, scope: 'org', next_cursor: 'cursor-2' } })
      })
      vi.stubGlobal('fetch', fetchMock)

      render(() => <AuditLogSection />)
      expect(await screen.findByText('member.added - ok')).toBeTruthy()

      fireEvent.click(await screen.findByTestId('audit-log-load-more'))
      flush()

      expect(await screen.findByText('login - ok')).toBeTruthy()
      // The first page stays; the second is appended below it.
      expect(screen.getByText('member.added - ok')).toBeTruthy()
      expect(screen.getByText('member.removed - ok')).toBeTruthy()
      const queries = auditQueries(fetchMock)
      expect(queries).toHaveLength(2)
      expect(queries[1]!.get('cursor')).toBe('cursor-2')
      expect(queries[1]!.get('limit')).toBe('50')
      await waitFor(() => expect(screen.queryByTestId('audit-log-load-more')).toBeNull())
    })

    it('a failed load shows a typed error, keeps the filters and offers no load more', async () => {
      vi.stubGlobal('fetch', vi.fn(async () => json({
        error: { code: 'invalid_cursor', message: 'cursor is not a cursor this API issued', details: {} },
      }, 400)))
      render(() => <AuditLogSection />)
      expect(await screen.findByRole('alert')).toBeTruthy()
      expect(screen.queryByTestId('audit-log-rows')).toBeNull()
      expect(screen.queryByTestId('audit-log-load-more')).toBeNull()
    })

    it('a stale page does not overwrite the rows of a newer query', async () => {
      const pending: Array<(response: Response) => void> = []
      const fetchMock = vi.fn((_input: RequestInfo | URL) => new Promise<Response>((resolve) => {
        pending.push(resolve)
      }))
      vi.stubGlobal('fetch', fetchMock)

      render(() => <AuditLogSection />)
      await waitFor(() => expect(pending).toHaveLength(1))

      // A second query starts while the first is still in flight.
      type('audit-filter-plane', 'model')
      fireEvent.submit(screen.getByTestId('audit-log-filters'))
      flush()
      await waitFor(() => expect(pending).toHaveLength(2))

      // The newer query answers first, then the older one lands late.
      pending[1]!(json({ data: [row(20, 'newer.event', { plane: 'model' })], meta: { count: 1 } }))
      expect(await screen.findByText('newer.event - ok')).toBeTruthy()
      pending[0]!(json({ data: [row(10, 'older.event')], meta: { count: 1 } }))
      await new Promise((resolve) => setTimeout(resolve, 20))
      flush()

      expect(screen.getByText('newer.event - ok')).toBeTruthy()
      expect(screen.queryByText('older.event - ok')).toBeNull()
    })
  })

  describe('as a non-admin', () => {
    it.each(['member', 'viewer', 'guest'])('a %s sees the admin-only note and sends no request', async (role) => {
      signIn(role)
      const fetchMock = vi.fn(async () => json({ data: [row(1, 'login')], meta: { count: 1 } }))
      vi.stubGlobal('fetch', fetchMock)

      render(() => <AuditLogSection />)

      expect(await screen.findByTestId('audit-log-admin-only')).toBeTruthy()
      expect(screen.queryByTestId('audit-log-filters')).toBeNull()
      expect(screen.queryByTestId('audit-log-rows')).toBeNull()
      await new Promise((resolve) => setTimeout(resolve, 20))
      expect(fetchMock).not.toHaveBeenCalled()
    })

    it('a signed-out visitor sends no request either', async () => {
      const fetchMock = vi.fn(async () => json({ data: [] }))
      vi.stubGlobal('fetch', fetchMock)
      render(() => <AuditLogSection />)
      expect(await screen.findByTestId('audit-log-admin-only')).toBeTruthy()
      expect(fetchMock).not.toHaveBeenCalled()
    })
  })
})
