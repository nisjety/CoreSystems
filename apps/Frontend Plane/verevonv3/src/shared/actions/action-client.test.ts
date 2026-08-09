import { afterEach, describe, expect, it, vi } from 'vitest'
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'

const actor: ActionActor = { type: 'human', userId: 'user_1', orgId: 'org_acme' }

afterEach(() => {
  vi.unstubAllGlobals()
})

// Every action here has a real gateway dispatcher, so executeAction must reach
// the gateway rather than throwing the honest "not available" client guard.
const wiredCases: Array<{ actionId: Parameters<typeof executeAction>[0]; input: unknown }> = [
  { actionId: 'brreg.lookup_organization', input: { q: 'coresystem as', size: 5 } },
  {
    actionId: 'social.create_draft',
    input: { title: 'Launch note', body: 'Body copy', platforms: ['linkedin'], sourceKind: 'manual' },
  },
  {
    actionId: 'social.schedule_post',
    input: { postId: 'social_post_1', scheduledAt: '2026-07-08T10:00:00Z', platforms: ['linkedin'] },
  },
  {
    actionId: 'social.publish_post',
    input: { postId: 'social_post_1', platforms: ['linkedin', 'x'], approvalId: 'approval_1' },
  },
  { actionId: 'inbox.set_csat_preference', input: { conversationId: 'conversation_1', optedIn: true } },
]

describe('executeAction wiring', () => {
  for (const testCase of wiredCases) {
    it(`dispatches ${testCase.actionId} to the gateway /actions/execute route`, async () => {
      vi.stubGlobal(
        'fetch',
        vi.fn(async () =>
          jsonResponse({
            actionId: testCase.actionId,
            runId: 'run_1',
            status: 'completed',
            auditId: 'audit_1',
            eventStream: '',
          }),
        ),
      )

      const execution = await executeAction(testCase.actionId, actor, testCase.input)

      const [url, init] = vi.mocked(fetch).mock.calls[0]!
      const headers = init?.headers as Headers
      expect(String(url)).toBe('/api/v1/actions/execute')
      expect(init?.method).toBe('POST')
      expect(headers.get('x-verevon-org-id')).toBe('org_acme')
      expect(init?.credentials).toBe('include')
      expect(JSON.parse(String(init?.body)).actionId).toBe(testCase.actionId)
      expect(execution.actionId).toBe(testCase.actionId)
    })
  }

  it('refuses unknown/undispatched actions honestly instead of fabricating a run', async () => {
    vi.stubGlobal('fetch', vi.fn())

    // Every action registered today has a dispatcher (the registry/executor
    // reconciliation removed the orphans), so a registered-but-undispatched id
    // can no longer be constructed from real registry data. The reachable
    // refusal is the unknown-action guard: it must throw and never hit the
    // network — no fabricated runs either way.
    await expect(
      executeAction('future.not_yet_dispatched' as Parameters<typeof executeAction>[0], actor, {
        url: 'https://example.com/docs',
      }),
    ).rejects.toThrow('Unknown action')
    expect(vi.mocked(fetch)).not.toHaveBeenCalled()
  })

  it('refuses unsafe ticket resource URLs before they can reach the gateway', async () => {
    vi.stubGlobal('fetch', vi.fn())

    await expect(executeAction('tickets.link_resource', actor, {
      ticketId: 'ticket_1',
      resourceKind: 'external',
      resourceUrl: 'javascript:alert(1)',
    })).rejects.toThrow(/Invalid action input/i)

    expect(vi.mocked(fetch)).not.toHaveBeenCalled()
  })
})

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify({ data: body }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
