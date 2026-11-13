import { describe, expect, it } from 'vitest'
import { streamFailurePresentation } from '@/features/chat/components/chat-normalizers'
import { translateApiError } from './errors'

// V1 verifier note: model-gateway refuses a Do run whose org agent policy cannot be read inside its bound with the
// typed code `org_policy_unavailable` (org_agent_policy.rs POLICY_UNAVAILABLE_CODE), on the turn's stream error
// frame and on a continue over HTTP (503, relayed by the BFF, listed in chat-run-continue-contract.ts). Its sibling
// `org_agents_paused` has Norwegian and English copy in errors.ts, a stream presentation, and a fixed sentence on
// the stream connection. `org_policy_unavailable` must be worded the same way: by its code, in the reader's
// language, never as the server's English operator text, and saying nothing ran.

const no = (noText: string) => noText
const en = (_noText: string, enText: string) => enText

const SERVER_TEXT =
  "The organisation's agent policy cannot be read right now, so this agent run cannot start. Try again shortly."

describe('org_policy_unavailable copy', () => {
  it('has its own Norwegian and English words over HTTP, not the generic unavailable copy', () => {
    const refusal = { code: 'org_policy_unavailable', status: 503, message: SERVER_TEXT }
    const generic = { code: 'some_unlisted_code', status: 503, message: SERVER_TEXT }
    const norwegian = translateApiError(refusal, no)
    const english = translateApiError(refusal, en)

    expect(norwegian).not.toBe(english)
    expect(norwegian).not.toBe(translateApiError(generic, no))
    expect(english).not.toBe(translateApiError(generic, en))
    expect(norwegian).not.toBe(translateApiError({ code: 'org_agents_paused', status: 403 }, no))
    expect(norwegian).not.toContain(SERVER_TEXT)
  })

  it('is worded by its code on the stream, in the reader language, and says nothing was saved', () => {
    const norwegian = streamFailurePresentation('org_policy_unavailable', SERVER_TEXT, no)
    const english = streamFailurePresentation('org_policy_unavailable', SERVER_TEXT, en)

    expect(norwegian.message).not.toBe(SERVER_TEXT)
    expect(norwegian.message).not.toBe(english.message)
    expect(norwegian.unsaved).toBe(true)
  })

  it('is worded like its sibling org_agents_paused, which already is', () => {
    const paused = streamFailurePresentation('org_agents_paused', 'raw server text', no)
    expect(paused.message).not.toBe('raw server text')
    expect(paused.unsaved).toBe(true)
    expect(translateApiError({ code: 'org_agents_paused', status: 403 }, no)).toMatch(/pause/)
  })
})
