import { chatOrganizationActionSchema, chatOrganizationSchema, type ChatOrganization, type ChatOrganizationAction } from '@/shared/actions/chat-organization'
import { readClientValue, writeClientJson } from '@/shared/session/client-storage'
import type { ChatThreadHistoryItem } from './chat-thread-history'

export function emptyChatOrganization(): ChatOrganization {
  return { version: 1, projects: [], assignments: {}, view: { group: 'project', sort: 'recent', filter: 'all' } }
}
export function chatOrganizationStorageKey(userId: string | undefined, orgId: string | undefined): string | null {
  return userId && orgId ? `verevon.chat-organization.v1:${encodeURIComponent(orgId)}:${encodeURIComponent(userId)}` : null
}
export function readChatOrganization(key: string | null): ChatOrganization {
  if (!key) return emptyChatOrganization()
  try {
    const result = chatOrganizationSchema.safeParse(JSON.parse(readClientValue(key) ?? 'null'))
    return result.success ? result.data : emptyChatOrganization()
  } catch { return emptyChatOrganization() }
}
export function saveChatOrganization(key: string, state: ChatOrganization) { writeClientJson(key, state) }

export function applyChatOrganizationAction(state: ChatOrganization, input: ChatOrganizationAction): ChatOrganization {
  const action = chatOrganizationActionSchema.parse(input)
  if (action.type === 'view.update') return { ...state, view: action.view }
  if (action.type === 'thread.move') {
    if (action.projectId && !state.projects.some((project) => project.id === action.projectId)) throw new Error('Project not found.')
    const assignments = { ...state.assignments }
    if (action.projectId) assignments[action.threadId] = action.projectId
    else delete assignments[action.threadId]
    return { ...state, assignments }
  }
  if (action.type === 'project.create' || action.type === 'project.rename') {
    if (state.projects.some((project) => project.id !== (action.type === 'project.rename' ? action.id : null) && project.name.toLocaleLowerCase() === action.name.toLocaleLowerCase())) throw new Error('A project with this name already exists.')
  }
  if (action.type === 'project.create') {
    if (state.projects.length >= 100) throw new Error('The preview supports up to 100 projects.')
    if (state.projects.some((project) => project.id === action.id)) throw new Error('Project already exists.')
    return { ...state, projects: [...state.projects, { id: action.id, name: action.name, pinned: false }] }
  }
  if (!state.projects.some((project) => project.id === action.id)) throw new Error('Project not found.')
  if (action.type === 'project.remove') return {
    ...state,
    projects: state.projects.filter((project) => project.id !== action.id),
    assignments: Object.fromEntries(Object.entries(state.assignments).filter(([, projectId]) => projectId !== action.id)),
  }
  return { ...state, projects: state.projects.map((project) => project.id !== action.id ? project : action.type === 'project.rename' ? { ...project, name: action.name } : { ...project, pinned: action.pinned }) }
}

const activeStatuses = new Set(['running', 'in_progress', 'queued', 'pending'])
const attentionStatuses = new Set(['failed', 'timed_out', 'denied', 'rejected', 'awaiting_approval', 'waiting_approval', 'paused', 'blocked', 'ambiguous'])
export function selectOrganizedChats(items: ChatThreadHistoryItem[], state: ChatOrganization, query: string): ChatThreadHistoryItem[] {
  const needle = query.trim().toLocaleLowerCase()
  const matches = items.filter((item) => {
    const projectName = state.projects.find((project) => project.id === state.assignments[item.threadId])?.name ?? ''
    if (needle && !`${item.title} ${item.preview} ${projectName}`.toLocaleLowerCase().includes(needle)) return false
    const status = item.latestRunStatus?.trim().toLowerCase() ?? ''
    return state.view.filter === 'all' || (state.view.filter === 'pinned' && item.pinned === true) || (state.view.filter === 'active' && activeStatuses.has(status)) || (state.view.filter === 'attention' && attentionStatuses.has(status))
  })
  return matches.sort((a, b) => {
    if (state.view.sort === 'title') return a.title.localeCompare(b.title) || a.threadId.localeCompare(b.threadId)
    const time = (Date.parse(a.updatedAt) || 0) - (Date.parse(b.updatedAt) || 0)
    return (state.view.sort === 'oldest' ? time : -time) || a.threadId.localeCompare(b.threadId)
  })
}

export function chatDateGroup(updatedAt: string, now = new Date()): 'today' | 'week' | 'earlier' {
  const time = new Date(updatedAt)
  if (time.toDateString() === now.toDateString()) return 'today'
  const weekStart = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 6).getTime()
  return time.getTime() >= weekStart && time.getTime() <= now.getTime() ? 'week' : 'earlier'
}
