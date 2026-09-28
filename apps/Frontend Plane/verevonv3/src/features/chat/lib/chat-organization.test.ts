import { beforeEach, describe, expect, it } from 'vitest'
import { applyChatOrganizationAction as apply, chatDateGroup, chatOrganizationStorageKey, emptyChatOrganization, readChatOrganization, saveChatOrganization, selectOrganizedChats } from './chat-organization'
import type { ChatThreadHistoryItem } from './chat-thread-history'

const rows: ChatThreadHistoryItem[] = [
  { threadId: 'a', title: 'Zulu', preview: 'Quarterly report', updatedAt: '2026-09-26T12:00:00Z', latestRunStatus: 'completed', pinned: true },
  { threadId: 'b', title: 'Alpha', preview: 'Budget review', updatedAt: '2026-09-28T12:00:00Z', latestRunStatus: 'running' },
  { threadId: 'c', title: 'Beta', preview: 'Support issue', updatedAt: '2026-09-27T12:00:00Z', latestRunStatus: 'awaiting_approval' },
]
const ids = (items: ChatThreadHistoryItem[]) => items.map((item) => item.threadId)

describe('personal chat organization', () => {
  beforeEach(() => { localStorage.clear(); sessionStorage.clear() })

  it('scopes saved projects to both account and workspace and survives a reload', () => {
    const key = chatOrganizationStorageKey('user-a', 'org-a')!
    const state = apply(emptyChatOrganization(), { type: 'project.create', id: 'work', name: '  Work  ' })
    saveChatOrganization(key, state)
    expect(readChatOrganization(key).projects[0]?.name).toBe('Work')
    expect(readChatOrganization(chatOrganizationStorageKey('user-b', 'org-a')).projects).toEqual([])
    expect(readChatOrganization(chatOrganizationStorageKey('user-a', 'org-b')).projects).toEqual([])
    expect(chatOrganizationStorageKey(undefined, 'org-a')).toBeNull()
    expect(chatOrganizationStorageKey('user-a', undefined)).toBeNull()
  })

  it('recovers gracefully from malformed or unsupported stored data', () => {
    localStorage.setItem('bad-json', '{')
    localStorage.setItem('bad-version', JSON.stringify({ ...emptyChatOrganization(), version: 2 }))
    expect(readChatOrganization('bad-json')).toEqual(emptyChatOrganization())
    expect(readChatOrganization('bad-version')).toEqual(emptyChatOrganization())
  })

  it('renames and pins projects without mutating earlier states', () => {
    const initial = apply(emptyChatOrganization(), { type: 'project.create', id: 'work', name: 'Work' })
    const renamed = apply(initial, { type: 'project.rename', id: 'work', name: 'Research' })
    const pinned = apply(renamed, { type: 'project.pin', id: 'work', pinned: true })
    expect(initial.projects[0]).toEqual({ id: 'work', name: 'Work', pinned: false })
    expect(pinned.projects[0]).toEqual({ id: 'work', name: 'Research', pinned: true })
    expect(() => apply(pinned, { type: 'project.create', id: 'duplicate', name: ' research ' })).toThrow('already exists')
    expect(() => apply(pinned, { type: 'project.rename', id: 'missing', name: 'Something' })).toThrow('not found')
    expect(() => apply(pinned, { type: 'project.rename', id: 'work', name: ' ' })).toThrow()
  })

  it('moves and unassigns chats, and removes a project without altering conversations or other projects', () => {
    let state = apply(emptyChatOrganization(), { type: 'project.create', id: 'work', name: 'Work' })
    state = apply(state, { type: 'project.create', id: 'other', name: 'Other' })
    state = apply(state, { type: 'thread.move', threadId: 'a', projectId: 'work' })
    state = apply(state, { type: 'thread.move', threadId: 'b', projectId: 'other' })
    const removed = apply(state, { type: 'project.remove', id: 'work' })
    expect(removed.assignments).toEqual({ b: 'other' })
    expect(ids(selectOrganizedChats(rows, removed, ''))).toEqual(['b', 'c', 'a'])
    expect(apply(removed, { type: 'thread.move', threadId: 'b', projectId: null }).assignments).toEqual({})
    expect(() => apply(removed, { type: 'thread.move', threadId: 'a', projectId: 'missing' })).toThrow('not found')
    expect(state.assignments).toEqual({ a: 'work', b: 'other' })
  })

  it('searches titles, previews and project names, combined with meaningful status filters', () => {
    let state = apply(emptyChatOrganization(), { type: 'project.create', id: 'work', name: 'Finance' })
    state = apply(state, { type: 'thread.move', threadId: 'b', projectId: 'work' })
    expect(ids(selectOrganizedChats(rows, state, ' QUARTERLY '))).toEqual(['a'])
    expect(ids(selectOrganizedChats(rows, state, 'finance'))).toEqual(['b'])
    expect(ids(selectOrganizedChats(rows, state, 'beta'))).toEqual(['c'])
    for (const [filter, expected] of [['active', 'b'], ['attention', 'c'], ['pinned', 'a']] as const) {
      const filtered = apply(state, { type: 'view.update', view: { ...state.view, filter } })
      expect(ids(selectOrganizedChats(rows, filtered, ''))).toEqual([expected])
      expect(ids(selectOrganizedChats(rows, filtered, 'no match'))).toEqual([])
    }
  })

  it('sorts deterministically without rearranging the source history', () => {
    const state = emptyChatOrganization()
    expect(ids(selectOrganizedChats(rows, state, ''))).toEqual(['b', 'c', 'a'])
    expect(ids(selectOrganizedChats(rows, { ...state, view: { ...state.view, sort: 'oldest' } }, ''))).toEqual(['a', 'c', 'b'])
    expect(ids(selectOrganizedChats(rows, { ...state, view: { ...state.view, sort: 'title' } }, ''))).toEqual(['b', 'c', 'a'])
    expect(ids(rows)).toEqual(['a', 'b', 'c'])
  })

  it('groups by local calendar dates across month boundaries', () => {
    const now = new Date(2026, 9, 2, 12)
    expect(chatDateGroup(new Date(2026, 9, 2, 1).toISOString(), now)).toBe('today')
    expect(chatDateGroup(new Date(2026, 8, 26, 0).toISOString(), now)).toBe('week')
    expect(chatDateGroup(new Date(2026, 8, 25, 23, 59).toISOString(), now)).toBe('earlier')
    expect(chatDateGroup('not a date', now)).toBe('earlier')
  })
})
