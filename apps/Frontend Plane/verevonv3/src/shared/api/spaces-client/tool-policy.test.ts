import { afterEach, describe, expect, it, vi } from 'vitest'

import { getSpaceToolPolicy, setSpaceAgentToolPolicy, setSpaceToolPolicy } from './tool-policy'

type FetchCall = { url: string; init: RequestInit | undefined }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init })
      return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
    }),
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const layer = (over: Record<string, unknown> = {}) => ({
  layer: 'space_ceiling',
  revision: 2,
  entries: [{ tool_key: 'mail.send', level: 'ask' }],
  ...over,
})

const merged = {
  tools: { 'mail.send': { level: 'ask', source: 'space_ceiling' } },
  ceiling_revision: 2,
  agent_revision: 0,
  digest: 'digest_1',
}

describe('getSpaceToolPolicy (WS10-S5b)', () => {
  it('GETs the Space tool policy, and adds the agent filter only when an agent is named', async () => {
    const calls = stubFetch(200, { data: { ceiling: layer(), agents: [], merged } })
    await getSpaceToolPolicy('space 1')
    expect(calls[0]?.url.endsWith('/api/v1/spaces/space%201/tool-policy')).toBe(true)
    expect((calls[0]?.init?.method ?? 'GET').toUpperCase()).toBe('GET')
    await getSpaceToolPolicy('space 1', 'agent/1')
    expect(calls[1]?.url.endsWith('/api/v1/spaces/space%201/tool-policy?agent_subject_id=agent%2F1')).toBe(true)
  })

  it('reads an unknown level as block and an unknown source as default', async () => {
    stubFetch(200, {
      data: {
        ceiling: layer({ entries: [{ tool_key: 'mail.send', level: 'maybe' }] }),
        agents: [
          {
            layer: 'agent',
            agent_subject_id: 'agent_1',
            revision: 1,
            entries: [{ tool_key: 'web.fetch', level: 'read-only' }],
          },
        ],
        merged: {
          tools: { 'mail.send': { level: 'maybe', source: 'elsewhere' } },
          ceiling_revision: 2,
          agent_revision: 1,
          digest: 'digest_2',
        },
      },
    })
    const view = await getSpaceToolPolicy('room')
    expect(view.ceiling.entries).toEqual([{ toolKey: 'mail.send', level: 'block' }])
    expect(view.agents[0]?.entries).toEqual([{ toolKey: 'web.fetch', level: 'block' }])
    expect(view.merged.tools['mail.send']).toEqual({ level: 'block', source: 'default' })
  })

  it('reads null entries as empty and drops an entry with no tool key', async () => {
    stubFetch(200, {
      data: { ceiling: { layer: 'space_ceiling', revision: 0, entries: null }, agents: null, merged: {} },
    })
    const empty = await getSpaceToolPolicy('room')
    expect(empty.ceiling.entries).toEqual([])
    expect(empty.agents).toEqual([])

    stubFetch(200, {
      data: {
        ceiling: layer({ entries: [null, { tool_key: '', level: 'allow' }, { tool_key: 'docs.read', level: 'allow' }] }),
        merged: {},
      },
    })
    expect((await getSpaceToolPolicy('room')).ceiling.entries).toEqual([{ toolKey: 'docs.read', level: 'allow' }])
  })
})

describe('setSpaceToolPolicy and setSpaceAgentToolPolicy (WS10-S5b)', () => {
  it('PUTs the Space ceiling entries with the expected revision', async () => {
    const calls = stubFetch(200, { data: { policy: layer({ revision: 4 }), merged, projection: 'pending' } })
    const write = await setSpaceToolPolicy('space 1', [{ toolKey: 'mail.send', level: 'block' }], 3)
    expect(calls[0]?.url.endsWith('/api/v1/spaces/space%201/tool-policy')).toBe(true)
    expect(calls[0]?.init?.method).toBe('PUT')
    expect(JSON.parse(String(calls[0]?.init?.body))).toEqual({
      entries: [{ tool_key: 'mail.send', level: 'block' }],
      expected_revision: 3,
    })
    expect(write.policy.revision).toBe(4)
    expect(write.projection).toBe('pending')
  })

  it('PUTs one agent layer to its binding path', async () => {
    const calls = stubFetch(200, { data: { policy: layer({ layer: 'agent', revision: 1 }), merged } })
    const write = await setSpaceAgentToolPolicy('space 1', 'bind/1', [], 0)
    expect(calls[0]?.url.endsWith('/api/v1/spaces/space%201/agents/bind%2F1/tool-policy')).toBe(true)
    expect(calls[0]?.init?.method).toBe('PUT')
    expect(JSON.parse(String(calls[0]?.init?.body))).toEqual({ entries: [], expected_revision: 0 })
    expect(write.policy.layer).toBe('agent')
    expect(write.projection).toBeUndefined()
  })
})
