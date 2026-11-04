import { describe, expect, it } from 'vitest'

import {
  audienceConfirmRequired,
  chatProjectRefusal,
  projectTurnsFromTranscript,
  requestedProjectRef,
  sandboxToolsRefusedNotice,
} from './use-chat-project'

const room = { kind: 'room', lifecycle: 'active', is_organization_room: false }
const personal = { kind: 'personal', lifecycle: 'active' }

describe('requestedProjectRef', () => {
  it('sends a new chat to the project picked for it', () => {
    expect(requestedProjectRef({ isNewThread: true, pickedRef: 'space_launch' })).toBe('space_launch')
  })

  it('keeps a thread in the project it belongs to, whatever is picked now', () => {
    expect(requestedProjectRef({ isNewThread: false, threadRef: 'space_launch', pickedRef: 'space_other' })).toBe('space_launch')
  })

  it('never gives an existing ordinary chat a project from the picker', () => {
    expect(requestedProjectRef({ isNewThread: false, pickedRef: 'space_launch' })).toBeUndefined()
  })

  it('sends nothing to a new chat with no pick, which is an ordinary chat', () => {
    expect(requestedProjectRef({ isNewThread: true, pickedRef: null })).toBeUndefined()
    expect(requestedProjectRef({ isNewThread: true, pickedRef: '' })).toBeUndefined()
  })
})

describe('chatProjectRefusal', () => {
  it('lets an editor start a chat in a room and a personal Space', () => {
    expect(chatProjectRefusal({ temporary: false, role: 'editor', space: room })).toBeNull()
    expect(chatProjectRefusal({ temporary: false, role: 'owner', space: personal })).toBeNull()
  })

  it('refuses a viewer, because a viewer reads a room without posting to it', () => {
    expect(chatProjectRefusal({ temporary: false, role: 'viewer', space: room })).toBe('role_not_allowed')
  })

  it('refuses an unknown role rather than assuming one', () => {
    expect(chatProjectRefusal({ temporary: false, role: undefined, space: room })).toBe('role_not_allowed')
  })

  it('excludes the organization channel, even for an owner', () => {
    expect(chatProjectRefusal({ temporary: false, role: 'owner', space: { ...room, is_organization_room: true } })).toBe('not_eligible')
  })

  it('excludes a room whose organization flag is unknown', () => {
    expect(chatProjectRefusal({ temporary: false, role: 'owner', space: { kind: 'room', lifecycle: 'active' } })).toBe('not_eligible')
  })

  it('excludes an inactive Space and a Space that cannot be read', () => {
    expect(chatProjectRefusal({ temporary: false, role: 'owner', space: { ...room, lifecycle: 'archived' } })).toBe('not_eligible')
    expect(chatProjectRefusal({ temporary: false, role: 'owner', space: null })).toBe('not_eligible')
  })

  it('disables a project for temporary chat, which writes no thread', () => {
    expect(chatProjectRefusal({ temporary: true, role: 'owner', space: room })).toBe('temporary')
  })
})

describe('audienceConfirmRequired', () => {
  it('asks a shared room once per browser before the first send', () => {
    expect(audienceConfirmRequired({ kind: 'room', confirmed: false })).toBe(true)
    expect(audienceConfirmRequired({ kind: 'room', confirmed: true })).toBe(false)
  })

  it('never asks for a personal Space', () => {
    expect(audienceConfirmRequired({ kind: 'personal', confirmed: false })).toBe(false)
  })
})

describe('sandboxToolsRefusedNotice', () => {
  it('says so for a Do turn in a room project only', () => {
    expect(sandboxToolsRefusedNotice({ kind: 'room', doMode: true })).toBe(true)
    expect(sandboxToolsRefusedNotice({ kind: 'room', doMode: false })).toBe(false)
    expect(sandboxToolsRefusedNotice({ kind: 'personal', doMode: true })).toBe(false)
    expect(sandboxToolsRefusedNotice({ kind: undefined, doMode: true })).toBe(false)
  })
})

describe('projectTurnsFromTranscript', () => {
  it('keeps the user and assistant turns, in order, with positional ids', () => {
    const turns = projectTurnsFromTranscript('thread_1', [
      { role: 'user', content: 'Summarise the launch plan.', authorSubjectId: 'user_b' },
      { role: 'system', content: 'Room notice' },
      { role: 'assistant', content: 'Here is the plan.', agentName: 'Planner' },
      { role: 'tool', content: 'tool output' },
    ])

    expect(turns.map((turn) => [turn.id, turn.role, turn.content])).toEqual([
      ['thread_1:room:0', 'user', 'Summarise the launch plan.'],
      ['thread_1:room:1', 'assistant', 'Here is the plan.'],
    ])
    expect(turns.every((turn) => turn.streaming === false && turn.tools.length === 0 && turn.attachments.length === 0)).toBe(true)
  })

  it('leaves the timestamps to the caller, since a room transcript carries none', () => {
    expect(projectTurnsFromTranscript('thread_1', [{ role: 'user', content: 'Hi' }])[0]?.createdAt).toBe('')
  })
})
