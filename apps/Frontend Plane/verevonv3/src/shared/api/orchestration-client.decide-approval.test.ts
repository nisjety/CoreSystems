import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  getActionDescriptor,
  ORCHESTRATION_APPROVAL_REASON_MAX,
  orchestrationDecideApprovalInput,
} from '@/shared/actions/action-registry'
import { ApiError } from './http'
import { decideApproval } from './orchestration-client'

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } })
}

// The reason is checked against the orchestration.decide_approval action
// contract before any request is sent. These cases cover the redirect channel.
describe('decideApproval contract check', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('validates with the same schema object the registry publishes for orchestration.decide_approval', () => {
    expect(getActionDescriptor('orchestration.decide_approval')?.inputSchema).toBe(orchestrationDecideApprovalInput)
  })

  it('sends a redirect reason as a reject, trimmed', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ approval: { id: 'appr_1', state: 'APPROVAL_STATE_DENIED' } }))
    vi.stubGlobal('fetch', fetchMock)

    await decideApproval('appr_1', 'reject', '  open the invoice tab  ')

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(init.body as string)).toEqual({ decision: 'reject', reason: 'open the invoice tab' })
  })

  it('refuses a reason over the cap without sending anything', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    const attempt = decideApproval('appr_1', 'reject', 'x'.repeat(ORCHESTRATION_APPROVAL_REASON_MAX + 1))

    await expect(attempt).rejects.toBeInstanceOf(ApiError)
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('refuses an empty approval id without sending anything', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    await expect(decideApproval('', 'approve')).rejects.toBeInstanceOf(ApiError)
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
