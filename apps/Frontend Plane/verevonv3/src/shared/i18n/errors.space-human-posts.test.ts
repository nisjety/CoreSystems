import { describe, expect, it } from 'vitest'

import { translateApiError } from './errors'

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

// The codes the WS01 human-post and thread routes answer with (errors.ts, WS01 banner).
const WS01_CODES = [
  'active_membership_required',
  'idempotency_key_conflict',
  'invalid_client_send_id',
  'invalid_post',
  'managed_run_already_started',
  'post_not_stored',
  'session_unavailable',
  'space_authority_stale',
  'space_decision_unavailable',
  'space_human_posts_disabled',
  'space_lifecycle_unavailable',
  'space_not_found',
  'space_post_not_authorized',
  'space_read_not_authorized',
  'thread_not_found',
  'thread_state_conflict',
] as const

describe('WS01 human-post error copy', () => {
  const genericNo = translateApiError({ code: 'a_code_nobody_registered' }, no)
  const genericEn = translateApiError({ code: 'a_code_nobody_registered' }, en)

  it.each(WS01_CODES)('%s has its own Norwegian and English copy', (code) => {
    const norwegian = translateApiError({ code }, no)
    const english = translateApiError({ code }, en)
    expect(norwegian.trim()).not.toBe('')
    expect(english.trim()).not.toBe('')
    // Never the generic fallback, and the two languages differ.
    expect(norwegian).not.toBe(genericNo)
    expect(english).not.toBe(genericEn)
    expect(norwegian).not.toBe(english)
  })

  it('tells the member what happened to the message for the retryable codes', () => {
    for (const code of ['session_unavailable', 'space_decision_unavailable', 'space_lifecycle_unavailable']) {
      expect(translateApiError({ code }, en)).toMatch(/kept/i)
    }
    expect(translateApiError({ code: 'post_not_stored' }, en)).toMatch(/only be sent once/i)
  })

  it('keeps the role copy for space_post_not_authorized when no typed reason rides along', () => {
    expect(translateApiError({ code: 'space_post_not_authorized' }, en)).toMatch(/role in this room does not allow posting/i)
  })

  it('keeps the typed denial copy when space_post_not_authorized carries a details.reason', () => {
    const text = translateApiError(
      { code: 'space_post_not_authorized', details: { reason: 'space_membership_required' } },
      en,
    )
    expect(text).toMatch(/do not have access to this Space/i)
    expect(text).not.toMatch(/role in this room/i)
  })
})
