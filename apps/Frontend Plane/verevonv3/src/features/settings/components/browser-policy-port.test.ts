import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/http'
import { BrowserPolicyRevisionConflict } from '@/features/settings/components/BrowserPolicySection'
import { browserPolicySettingsEnabled, createBrowserPolicyPort } from '@/features/settings/components/browser-policy-port'

const { executeAction } = vi.hoisted(() => ({ executeAction: vi.fn() }))

vi.mock('@/shared/actions/action-client', () => ({ executeAction }))

const actor = { type: 'human' as const, userId: 'user-1', orgId: 'org-1' }
const actorOf = () => actor

const storedDocument = {
  caps: { 'browser.act.input': 'disabled' },
  domains: { allow: ['example.com'], deny: ['blocked.example.org'] },
}

function execution(status: string, result?: unknown, runId = 'run-1') {
  return { actionId: 'x', runId, status, auditId: 'audit-1', eventStream: 'events', result }
}

afterEach(() => {
  executeAction.mockReset()
  vi.unstubAllEnvs()
})

describe('browser policy port (E3.2-g1)', () => {
  it('maps the read to the section view and leaves the per-origin list empty', async () => {
    executeAction.mockResolvedValue(
      execution('completed', {
        policy: storedDocument,
        effective: storedDocument,
        revision: 3,
        rev: 'abc123def456',
        entitlements: { 'web.paid_search': false },
        can_manage: true,
        updated_by: null,
        updated_at: null,
      }),
    )
    const port = createBrowserPolicyPort(actorOf)
    expect(await port.load('org-1')).toEqual({
      available: true,
      revision: 3,
      canManage: true,
      document: storedDocument,
      effective: storedDocument,
      entitlements: { 'web.paid_search': false },
      originConsents: [],
    })
    expect(executeAction).toHaveBeenCalledWith('browser.get_org_policy', actor, { orgId: 'org-1' })
  })

  it('hides the section without a session actor and never calls the gateway', async () => {
    const port = createBrowserPolicyPort(() => null)
    expect(await port.load('org-1')).toEqual({ available: false })
    expect(executeAction).not.toHaveBeenCalled()
  })

  it('hides the section when the answer does not have the expected shape', async () => {
    executeAction.mockResolvedValue(execution('completed', { revision: 'three' }))
    expect(await createBrowserPolicyPort(actorOf).load('org-1')).toEqual({ available: false })
  })

  it('sends the revision as ifMatch with the whole document and reports the applied revision', async () => {
    executeAction.mockResolvedValue(execution('completed', { revision: 4, rev: 'fff000aaa111' }))
    const result = await createBrowserPolicyPort(actorOf).propose('org-1', {
      ifMatch: 3,
      document: storedDocument,
    })
    expect(result).toEqual({ status: 'applied', revision: 4 })
    expect(executeAction).toHaveBeenCalledWith('browser.set_org_policy', actor, {
      orgId: 'org-1',
      ifMatch: 3,
      document: storedDocument,
    })
  })

  it('reports a change that waits for approval with its run', async () => {
    executeAction.mockResolvedValue(execution('waiting_approval', undefined, 'run-9'))
    expect(
      await createBrowserPolicyPort(actorOf).propose('org-1', { ifMatch: 3, document: storedDocument }),
    ).toEqual({ status: 'waiting_approval', runId: 'run-9' })
  })

  it('turns a 409 revision conflict into the error the section reloads on', async () => {
    executeAction.mockRejectedValue(new ApiError('changed', 409, 'revision_conflict'))
    await expect(
      createBrowserPolicyPort(actorOf).propose('org-1', { ifMatch: 3, document: storedDocument }),
    ).rejects.toBeInstanceOf(BrowserPolicyRevisionConflict)
  })

  it('rethrows every other refusal unchanged, so the section shows its message', async () => {
    const refusal = new ApiError('not in this plan', 402, 'not_entitled')
    executeAction.mockRejectedValue(refusal)
    await expect(
      createBrowserPolicyPort(actorOf).propose('org-1', { ifMatch: 3, document: storedDocument }),
    ).rejects.toBe(refusal)
  })

  it('refuses a save without a session actor before it reaches the gateway', async () => {
    await expect(
      createBrowserPolicyPort(() => null).propose('org-1', { ifMatch: 3, document: storedDocument }),
    ).rejects.toThrow('not available yet')
    expect(executeAction).not.toHaveBeenCalled()
  })

  it('mounts only when VITE_BROWSER_POLICY_SETTINGS is the literal true', () => {
    vi.stubEnv('VITE_BROWSER_POLICY_SETTINGS', '')
    expect(browserPolicySettingsEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_POLICY_SETTINGS', 'TRUE')
    expect(browserPolicySettingsEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_POLICY_SETTINGS', 'true')
    expect(browserPolicySettingsEnabled()).toBe(true)
  })
})
