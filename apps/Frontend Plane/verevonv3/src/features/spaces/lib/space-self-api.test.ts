import { describe, expect, it } from 'vitest'

import { ApiError } from '@/shared/api/http'

import { selfApiErrorMessage, threadPinnedByAgent, threadRenamedByAgent } from './space-self-api'

const en = (_noText: string, enText: string) => enText

describe('WS18-S6 agent own-thread switch presentation', () => {
  it('selfApiErrorMessage words space_role_cannot_manage_agent in the switch\'s own sentence, gives invalid_self_api_request, agent_binding_not_active and space_registry_unavailable their copy, and takes the fallback for an unknown code', () => {
    // The shared copy for this code is the wake policy's ("who can wake the agent"); the switch words it itself.
    const ownSentence = selfApiErrorMessage(new ApiError('forbidden', 403, 'space_role_cannot_manage_agent'), en)
    expect(ownSentence).toBe(
      'Only an owner or manager can change whether the agent may act on its own thread. Nothing was changed.',
    )
    expect(ownSentence).not.toContain('wake')

    expect(selfApiErrorMessage(new ApiError('invalid', 400, 'invalid_self_api_request'), en)).toBe(
      "The setting for the agent's own actions is not valid. Nothing was changed.",
    )
    expect(selfApiErrorMessage(new ApiError('inactive', 409, 'agent_binding_not_active'), en)).toBe(
      'The agent is not active in this room right now, so this setting cannot be changed. Nothing was changed.',
    )
    expect(selfApiErrorMessage(new ApiError('unavailable', 503, 'space_registry_unavailable'), en)).toBe(
      'The setting could not be changed. Nothing was changed. Try again shortly.',
    )

    const fallback = 'The agent setting could not be changed. Nothing was changed.'
    expect(selfApiErrorMessage(new ApiError('unknown', 400, 'self_api_code_nobody_knows'), en)).toBe(fallback)
  })

  it('threadRenamedByAgent is true only for title_source agent', () => {
    expect(threadRenamedByAgent({ title_source: 'agent' })).toBe(true)
    expect(threadRenamedByAgent({ title_source: 'owner' })).toBe(false)
    // Absent is never an agent (an older gateway sends no title_source).
    expect(threadRenamedByAgent({})).toBe(false)
  })

  it('threadPinnedByAgent needs pinned true and pinned_source agent', () => {
    expect(threadPinnedByAgent({ pinned: true, pinned_source: 'agent' })).toBe(true)
    expect(threadPinnedByAgent({ pinned: true, pinned_source: 'owner' })).toBe(false)
    expect(threadPinnedByAgent({ pinned: false, pinned_source: 'agent' })).toBe(false)
    expect(threadPinnedByAgent({ pinned_source: 'agent' })).toBe(false)
    expect(threadPinnedByAgent({ pinned: true })).toBe(false)
  })
})
