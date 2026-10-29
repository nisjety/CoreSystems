// @vitest-environment jsdom

import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ConversationActivityTimeline } from './ConversationActivityTimeline'

function jsonResponse(data: unknown) {
  return new Response(JSON.stringify({ data }), { headers: { 'Content-Type': 'application/json' } })
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('ConversationActivityTimeline', () => {
  it('renders only canonical lifecycle labels from the payload-redacted activity projection', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse([
      { id: 'audit_1', action: 'status.changed', actor_user_id: 'user_1', created_at: '2026-08-03T08:00:00.000Z' },
      { id: 'audit_2', action: 'note.created', actor_user_id: 'user_2', created_at: '2026-08-03T08:01:00.000Z' },
      { id: 'audit_3', action: 'ticket.linked', resource_kind: 'ticket', created_at: '2026-08-03T08:02:00.000Z' },
    ])))

    render(() => <ConversationActivityTimeline orgId="org_1" conversationId="conv_1" refreshKey={0} />)

    expect(await screen.findByText(/samtalestatus oppdatert|conversation status updated/i)).toBeTruthy()
    expect(screen.getByText(/internt notat lagt til|internal note added/i)).toBeTruthy()
    expect(screen.getByText(/sak koblet til arbeid|ticket linked to work/i)).toBeTruthy()
    expect(screen.queryByText('user_1')).toBeNull()
  })

  it('does not disguise an unavailable canonical read as an empty timeline', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ error: { code: 'upstream_unavailable', message: 'Unavailable' } }), {
      status: 503, headers: { 'Content-Type': 'application/json' },
    })))

    render(() => <ConversationActivityTimeline orgId="org_1" conversationId="conv_1" refreshKey={0} />)

    expect((await screen.findByRole('alert')).textContent).toMatch(/arbeidsaktivitet kunne ikke lastes|work activity could not be loaded/i)
    expect(screen.queryByText(/ingen arbeidsaktivitet|no work activity/i)).toBeNull()
  })
})
