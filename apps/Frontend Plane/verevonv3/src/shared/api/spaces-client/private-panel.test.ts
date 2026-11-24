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
  window.localStorage.clear()
  window.sessionStorage.clear()
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
    expect(window.localStorage.getItem(STORAGE_KEY)).toBeNull()
    expect(panel.isPrivatePanelThread(pointer.threadId)).toBe(false)
  })

  it('reads the ids back from localStorage on a fresh module load', async () => {
    const first = await loadPanel()
    stubFetch(200, { data: { space_ref: 'room', thread_id: 'thr_kept', agent_subject_id: null } })
    await first.getSpacePrivatePanel('room')
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? 'null')).toEqual(['thr_kept'])
    const second = await loadPanel()
    expect(second.isPrivatePanelThread('thr_kept')).toBe(true)
  })

  // E-PRIO-01 (4): "Continue in Chat" opened in a new tab starts with an empty sessionStorage. The scope must hold there.
  it('keeps the scope in a new tab, which shares localStorage but not sessionStorage', async () => {
    const roomTab = await loadPanel()
    stubFetch(200, { data: { space_ref: 'room', thread_id: 'thr_new_tab', agent_subject_id: null } })
    await roomTab.openSpacePrivatePanel('room')
    window.sessionStorage.clear()
    const chatTab = await loadPanel()
    expect(chatTab.isPrivatePanelThread('thr_new_tab')).toBe(true)
  })

  it('sees a panel another tab opened after this page loaded', async () => {
    const chatTab = await loadPanel()
    expect(chatTab.isPrivatePanelThread('thr_later')).toBe(false)
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(['thr_later']))
    expect(chatTab.isPrivatePanelThread('thr_later')).toBe(true)
  })

  it('still reads an id an older build kept in sessionStorage, and keeps the ids of other tabs on a write', async () => {
    window.sessionStorage.setItem(STORAGE_KEY, JSON.stringify(['thr_legacy']))
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(['thr_other_tab']))
    const panel = await loadPanel()
    expect(panel.isPrivatePanelThread('thr_legacy')).toBe(true)
    stubFetch(200, { data: { space_ref: 'room', thread_id: 'thr_mine', agent_subject_id: null } })
    await panel.getSpacePrivatePanel('room')
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? 'null')).toEqual(['thr_other_tab', 'thr_mine'])
  })

  it('forgets every kept id, in memory and in both storages, on clearPrivatePanelThreads (sign-out)', async () => {
    window.sessionStorage.setItem(STORAGE_KEY, JSON.stringify(['thr_old_tab']))
    const panel = await loadPanel()
    stubFetch(200, { data: { space_ref: 'room', thread_id: 'thr_kept', agent_subject_id: null } })
    await panel.getSpacePrivatePanel('room')
    expect(panel.isPrivatePanelThread('thr_kept')).toBe(true)

    panel.clearPrivatePanelThreads()
    expect(window.localStorage.getItem(STORAGE_KEY)).toBeNull()
    expect(window.sessionStorage.getItem(STORAGE_KEY)).toBeNull()
    expect(panel.isPrivatePanelThread('thr_kept')).toBe(false)
    expect(panel.isPrivatePanelThread('thr_old_tab')).toBe(false)
  })

  it('still answers from memory when storage is blocked', async () => {
    const getItem = vi.spyOn(window.localStorage, 'getItem').mockImplementation(() => {
      throw new Error('storage blocked')
    })
    const setItem = vi.spyOn(window.localStorage, 'setItem').mockImplementation(() => {
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
