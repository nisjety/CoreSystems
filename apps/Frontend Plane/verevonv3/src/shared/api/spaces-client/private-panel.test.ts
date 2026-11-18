import { afterEach, describe, expect, it, vi } from 'vitest'

type FetchCall = { url: string; init: RequestInit | undefined }

const STORAGE_KEY = 'verevon.spaces.privatePanelThreads.v1'

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

// The module keeps the ids it has read in memory, so each test loads its own copy.
async function loadPanel() {
  vi.resetModules()
  return import('./private-panel')
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('private panel thread hint (W3-FIX-14)', () => {
  it('makes isPrivatePanelThread true for the thread a GET or a POST named, and for no other id', async () => {
    const panel = await loadPanel()
    stubFetch(200, { data: { space_ref: 'room', thread_id: 'thr_get', agent_subject_id: null } })
    await panel.getSpacePrivatePanel('room')
    stubFetch(200, { data: { space_ref: 'room', thread_id: 'thr_post', agent_subject_id: null } })
    await panel.openSpacePrivatePanel('room')
    expect(panel.isPrivatePanelThread('thr_get')).toBe(true)
    expect(panel.isPrivatePanelThread('thr_post')).toBe(true)
    expect(panel.isPrivatePanelThread('thr_other')).toBe(false)
    expect(panel.isPrivatePanelThread(null)).toBe(false)
    expect(panel.isPrivatePanelThread(undefined)).toBe(false)
  })

  it('adds nothing when the pointer names no thread', async () => {
    const panel = await loadPanel()
    stubFetch(200, { data: { space_ref: 'room', thread_id: null, agent_subject_id: null } })
    const pointer = await panel.getSpacePrivatePanel('room')
    expect(pointer.threadId).toBeNull()
    expect(window.sessionStorage.getItem(STORAGE_KEY)).toBeNull()
    expect(panel.isPrivatePanelThread(pointer.threadId)).toBe(false)
  })

  it('reads the ids back from sessionStorage on a fresh module load', async () => {
    const first = await loadPanel()
    stubFetch(200, { data: { space_ref: 'room', thread_id: 'thr_kept', agent_subject_id: null } })
    await first.getSpacePrivatePanel('room')
    expect(JSON.parse(window.sessionStorage.getItem(STORAGE_KEY) ?? 'null')).toEqual(['thr_kept'])
    const second = await loadPanel()
    expect(second.isPrivatePanelThread('thr_kept')).toBe(true)
  })

  it('still answers from memory when storage is blocked', async () => {
    const getItem = vi.spyOn(window.sessionStorage, 'getItem').mockImplementation(() => {
      throw new Error('storage blocked')
    })
    const setItem = vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new Error('storage blocked')
    })
    const panel = await loadPanel()
    stubFetch(200, { data: { space_ref: 'room', thread_id: 'thr_memory', agent_subject_id: null } })
    await panel.getSpacePrivatePanel('room')
    expect(getItem).toHaveBeenCalled()
    expect(setItem).toHaveBeenCalledWith(STORAGE_KEY, JSON.stringify(['thr_memory']))
    expect(panel.isPrivatePanelThread('thr_memory')).toBe(true)
  })
})

describe('private panel pointer (WS17-S6)', () => {
  it('GETs and POSTs the room private-panel path with an encoded space ref', async () => {
    const panel = await loadPanel()
    const calls = stubFetch(200, { data: { space_ref: 'space 1/a', thread_id: null, agent_subject_id: null } })
    await panel.getSpacePrivatePanel('space 1/a')
    await panel.openSpacePrivatePanel('space 1/a')
    expect(calls[0]?.url.endsWith('/api/v1/spaces/space%201%2Fa/private-panel')).toBe(true)
    expect((calls[0]?.init?.method ?? 'GET').toUpperCase()).toBe('GET')
    expect(calls[1]?.url.endsWith('/api/v1/spaces/space%201%2Fa/private-panel')).toBe(true)
    expect(calls[1]?.init?.method).toBe('POST')
  })

  it('sends an empty JSON object as the POST body', async () => {
    const panel = await loadPanel()
    const calls = stubFetch(200, { data: { space_ref: 'room', thread_id: 'thr_1', agent_subject_id: null } })
    await panel.openSpacePrivatePanel('room')
    expect(JSON.parse(String(calls[0]?.init?.body))).toEqual({})
  })

  it('maps a null thread_id to threadId null and keeps the agent subject', async () => {
    const panel = await loadPanel()
    stubFetch(200, { data: { space_ref: 'room', thread_id: null, agent_subject_id: 'agent_1' } })
    await expect(panel.getSpacePrivatePanel('room')).resolves.toEqual({
      spaceRef: 'room',
      threadId: null,
      agentSubjectId: 'agent_1',
    })
  })
})
