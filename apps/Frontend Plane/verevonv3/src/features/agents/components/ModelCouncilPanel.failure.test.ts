// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import { councilFailureText } from './ModelCouncilPanel'

// A council names no thread today, but a typed refusal of a thread (DEC-V-R8) must still read as the council's own refusal:
// the side chat's words for two of those codes are not the council's, and the council never retries a thread refusal.

const english = { tr: (_norwegian: string, english: string) => english }

const COUNCIL_THREAD_REFUSAL = 'The model council is not available for this chat. The request was not sent to a model.'

describe('councilFailureText', () => {
  it('words every typed thread refusal in the council\'s own terms, and never in the side chat\'s', () => {
    for (const code of [
      'scope_not_available_in_space',
      'connection_ambiguous',
      'side_chat_model_required',
      'side_chat_thread_retention_refused',
    ]) {
      const status = code === 'side_chat_model_required' ? 400 : 409
      expect(councilFailureText(new ApiError('refused', status, code), english), code).toBe(COUNCIL_THREAD_REFUSAL)
    }
  })

  it('keeps the words of the refusals the council already had', () => {
    expect(councilFailureText(new ApiError('off', 404, 'model_council_disabled'), english)).toBe(
      'The model council is not turned on for this installation.',
    )
    expect(councilFailureText(new ApiError('auth', 503, 'delegated_auth_unavailable'), english)).toBe(
      'Model access is temporarily unavailable. Try again shortly.',
    )
  })

  it('gives an untyped failure the council\'s fallback', () => {
    expect(councilFailureText(new ApiError('boom', 500, 'some_unmapped_code'), english)).toBe(
      'The model council could not finish the question. Try again.',
    )
  })
})
