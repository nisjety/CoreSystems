import { afterEach, describe, expect, it, vi } from 'vitest'
import { getBrowserSession } from './browser-client'

afterEach(() => vi.unstubAllGlobals())
describe('owner browser session projection', () => {
  it('normalizes the real raw Quarry owner response into the session contract', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ data: {
      runId: 'session-1', live: true, status: 'active', controlMode: 'agent_control', controlEpoch: 4,
      profileScope: 'run_scoped', profileStorage: 'ephemeral', currentUrl: 'https://example.com/',
      viewport: { width: 1280, height: 800 }, tabs: [{ tabId: 'tab-1', active: true, title: 'Example', url: 'https://example.com/' }], zdr: false,
    } }), { status: 200, headers: { 'content-type': 'application/json' } })))
    const result = await getBrowserSession('org-1', 'session-1')
    expect(result.session.id).toBe('session-1')
    expect(result.session.control).toEqual({ mode: 'agent_control', epoch: 4 })
    expect(result.session.tabs?.[0]?.tabId).toBe('tab-1')
    expect(result.session.capabilities).toEqual([])
    expect(result.session.viewport).toEqual({ width: 1280, height: 800 })
  })
  it('rejects a projection belonging to another session or missing control state', async () => {
    for (const body of [{ runId: 'other', live: true, controlMode: 'agent_control' }, { runId: 'session-1', live: true }]) {
      vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ data: body }), { status: 200 })))
      await expect(getBrowserSession('org-1', 'session-1')).rejects.toThrow('Invalid browser owner projection')
    }
  })
})
