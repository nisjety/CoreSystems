// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  CHAT_PROJECT_START_EVENT,
  audienceConfirmed,
  chatProjectsEnabled,
  listChatProjects,
  projectThreadRef,
  queueChatProjectStart,
  readProjectAudience,
  readProjectContext,
  rememberAudienceConfirmed,
  rememberProjectThread,
  takeChatProjectStart,
} from './chat-projects'

const spacesClient = vi.hoisted(() => ({
  getSpaceContext: vi.fn(),
  getSpaceMembership: vi.fn(),
  getSpaceRoster: vi.fn(),
  listSpaces: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', () => ({
  getSpaceContext: spacesClient.getSpaceContext,
  getSpaceMembership: spacesClient.getSpaceMembership,
  getSpaceRoster: spacesClient.getSpaceRoster,
  listSpaces: spacesClient.listSpaces,
}))

const personal = { space_ref: 'space_personal', name: 'Personal Space', kind: 'personal', lifecycle: 'active' }
const launchRoom = { space_ref: 'space_launch', name: 'Launch', kind: 'room', lifecycle: 'active', is_organization_room: false }
const viewerRoom = { space_ref: 'space_viewer', name: 'Read only', kind: 'room', lifecycle: 'active', is_organization_room: false }
const orgRoom = { space_ref: 'space_org', name: 'AQUATIQ AS', kind: 'room', lifecycle: 'active', is_organization_room: true }
const unknownRoom = { space_ref: 'space_unknown', name: 'Old room', kind: 'room', lifecycle: 'active' }
const archivedRoom = { space_ref: 'space_archived', name: 'Archived', kind: 'room', lifecycle: 'archived', is_organization_room: false }
const unreadableRoom = { space_ref: 'space_unreadable', name: 'Unreadable', kind: 'room', lifecycle: 'active', is_organization_room: false }

function membership(spaceRef: string, role: string) {
  return { space_ref: spaceRef, org_id: 'org_1', subject_id: 'user_1', kind: 'shared', role, revisions: {} }
}

describe('chat projects', () => {
  beforeEach(() => {
    window.localStorage.clear()
    window.sessionStorage.clear()
    spacesClient.listSpaces.mockReset().mockResolvedValue([personal, orgRoom, viewerRoom, launchRoom, unknownRoom, archivedRoom, unreadableRoom])
    spacesClient.getSpaceMembership.mockReset().mockImplementation(async (spaceRef: string) => {
      if (spaceRef === 'space_unreadable') throw new Error('membership unavailable')
      if (spaceRef === 'space_viewer') return membership(spaceRef, 'viewer')
      return membership(spaceRef, spaceRef === 'space_personal' ? 'owner' : 'editor')
    })
  })

  afterEach(() => {
    vi.unstubAllEnvs()
    window.localStorage.clear()
    window.sessionStorage.clear()
  })

  describe('the project switch', () => {
    it('is off unless the build carries the literal "true"', () => {
      expect(chatProjectsEnabled()).toBe(false)
      vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'TRUE')
      expect(chatProjectsEnabled()).toBe(false)
      vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', '1')
      expect(chatProjectsEnabled()).toBe(false)
      vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
      expect(chatProjectsEnabled()).toBe(true)
    })
  })

  describe('listChatProjects', () => {
    it('lists only Spaces the caller can start a chat in, personal first', async () => {
      const projects = await listChatProjects()

      expect(projects.map((project) => project.spaceRef)).toEqual(['space_personal', 'space_launch'])
      expect(projects[1]).toEqual({ spaceRef: 'space_launch', name: 'Launch', kind: 'room', role: 'editor' })
    })

    it('never lists the organization channel, a room whose organization flag is unknown, or an inactive room', async () => {
      const refs = (await listChatProjects()).map((project) => project.spaceRef)

      expect(refs).not.toContain('space_org')
      expect(refs).not.toContain('space_unknown')
      expect(refs).not.toContain('space_archived')
    })

    it('leaves out a viewer and a Space whose membership cannot be read', async () => {
      const refs = (await listChatProjects()).map((project) => project.spaceRef)

      expect(refs).not.toContain('space_viewer')
      expect(refs).not.toContain('space_unreadable')
    })
  })

  describe('readProjectAudience', () => {
    it('counts the people in the roster and not the agents', async () => {
      spacesClient.getSpaceRoster.mockResolvedValue([
        { subject_type: 'user', subject_id: 'a', role: 'owner', revision: 1, display_name: 'A' },
        { subject_type: 'user', subject_id: 'b', role: 'editor', revision: 1, display_name: 'B' },
        { subject_type: 'service', subject_id: 'agent', role: 'viewer', revision: 1, display_name: '' },
      ])

      expect(await readProjectAudience('space_launch')).toBe(2)
    })

    it('reports an unreadable roster as unknown rather than as an empty room', async () => {
      spacesClient.getSpaceRoster.mockRejectedValue(new Error('not in room'))

      expect(await readProjectAudience('space_launch')).toBeNull()
    })
  })

  describe('readProjectContext', () => {
    it('returns the Space and the caller role, and null when the caller cannot read it', async () => {
      spacesClient.getSpaceContext.mockResolvedValueOnce({ space: launchRoom, membership: membership('space_launch', 'manager') })
      expect(await readProjectContext('space_launch')).toEqual({ space: launchRoom, role: 'manager' })

      spacesClient.getSpaceContext.mockRejectedValueOnce(new Error('forbidden'))
      expect(await readProjectContext('space_launch')).toBeNull()
    })
  })

  describe('the pending project start', () => {
    it('is handed to the chat page once, and announces itself to a mounted page', () => {
      const announced = vi.fn()
      window.addEventListener(CHAT_PROJECT_START_EVENT, announced)

      queueChatProjectStart('space_launch')

      expect(announced).toHaveBeenCalledTimes(1)
      expect(takeChatProjectStart()).toBe('space_launch')
      expect(takeChatProjectStart()).toBeNull()
      window.removeEventListener(CHAT_PROJECT_START_EVENT, announced)
    })
  })

  describe('the project a thread belongs to', () => {
    it('is remembered for the thread only while the switch is on', () => {
      rememberProjectThread('thread_1', 'space_launch')
      expect(projectThreadRef('thread_1')).toBeUndefined()

      vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
      expect(projectThreadRef('thread_1')).toBe('space_launch')
      expect(projectThreadRef('thread_2')).toBeUndefined()
      expect(projectThreadRef(null)).toBeUndefined()
    })

    it('never answers for a key inherited from the object prototype', () => {
      vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')

      expect(projectThreadRef('constructor')).toBeUndefined()
      expect(projectThreadRef('__proto__')).toBeUndefined()
    })

    it('keeps the most recent hints when the list is full', () => {
      vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
      for (let index = 0; index < 205; index += 1) rememberProjectThread(`thread_${index}`, 'space_launch')

      expect(projectThreadRef('thread_0')).toBeUndefined()
      expect(projectThreadRef('thread_204')).toBe('space_launch')
    })

    it('ignores a stored value that is not a list of hints', () => {
      vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
      window.localStorage.setItem('verevon.chat-project-threads.v1', '["not","a","record"]')

      expect(projectThreadRef('thread_1')).toBeUndefined()
      rememberProjectThread('thread_1', 'space_launch')
      expect(projectThreadRef('thread_1')).toBe('space_launch')
    })
  })

  describe('the audience confirmation', () => {
    it('is remembered per project in this browser', () => {
      expect(audienceConfirmed('space_launch')).toBe(false)

      rememberAudienceConfirmed('space_launch')

      expect(audienceConfirmed('space_launch')).toBe(true)
      expect(audienceConfirmed('space_other')).toBe(false)
    })

    it('degrades to asking again when browser storage refuses the write', () => {
      const refuse = () => {
        throw new DOMException('quota', 'QuotaExceededError')
      }
      const spies = [window.localStorage, window.sessionStorage].map((storage) => vi.spyOn(storage, 'setItem').mockImplementation(refuse))

      expect(() => rememberAudienceConfirmed('space_launch')).not.toThrow()
      expect(audienceConfirmed('space_launch')).toBe(false)
      for (const spy of spies) spy.mockRestore()
    })
  })
})
