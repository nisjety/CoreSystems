import { describe, expect, it } from 'vitest'

import { ERROR_COPY, describeApiError, translateApiError } from './errors'

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

// D18: the two Control codes of per-member room history that had no words, and the ones they travel with. The gateway
// relays Control's codes with their status and only code, message and scalar details; the SPA words them, never the text.

describe('D18 per-member room history error copy', () => {
  const genericNo = translateApiError({ code: 'a_code_nobody_registered' }, no)
  const genericEn = translateApiError({ code: 'a_code_nobody_registered' }, en)

  it('member_history_window_invalid has its own Norwegian and English sentence that says nothing was changed', () => {
    const norwegian = translateApiError({ code: 'member_history_window_invalid', status: 422 }, no)
    const english = translateApiError({ code: 'member_history_window_invalid', status: 422 }, en)
    expect(norwegian).not.toBe(genericNo)
    expect(english).not.toBe(genericEn)
    expect(norwegian).toBe('Historikkvalget er ikke gyldig. Ingenting er endret.')
    expect(english).toBe('The history choice is not valid. Nothing was changed.')
  })

  it('member_history_window_unavailable says per-member history is not turned on, and that nothing was changed', () => {
    const norwegian = translateApiError({ code: 'member_history_window_unavailable', status: 503 }, no)
    const english = translateApiError({ code: 'member_history_window_unavailable', status: 503 }, en)
    expect(norwegian).toBe('Historikk per medlem er ikke slått på ennå. Ingenting er endret.')
    expect(english).toBe('Per-member history is not turned on yet. Nothing was changed.')
  })

  it('words both as a server refusal, not as a bare server error, so the person knows what it is', () => {
    for (const code of ['member_history_window_invalid', 'member_history_window_unavailable']) {
      const described = describeApiError({ code, status: code.endsWith('invalid') ? 422 : 503 }, en)
      expect(described.code).toBe(code)
      expect(described.message).not.toBe(genericEn)
    }
  })

  it('has an entry in both languages for every code the member routes relay, apart from the one a caller words itself', () => {
    // room_admin_unavailable is worded by each caller for what it was doing (nothing was changed, nobody was added).
    for (const code of [
      'member_history_window_invalid',
      'member_history_window_unavailable',
      'room_member_changed',
      'room_roster_derived',
      'room_archived',
      'room_role_forbidden',
      'room_delegation_required',
      'room_not_found',
      'member_not_found',
      'member_required',
      'feature_disabled',
      'invalid_request',
      'upstream_unavailable',
    ]) {
      const copy = ERROR_COPY[code]
      expect(copy, code).toBeDefined()
      expect(copy?.no.trim().length, `${code} no`).toBeGreaterThan(0)
      expect(copy?.en.trim().length, `${code} en`).toBeGreaterThan(0)
      expect(copy?.no, code).not.toBe(copy?.en)
    }
    expect(ERROR_COPY.room_admin_unavailable).toBeUndefined()
  })

  it('never shows the server’s own text for a refusal', () => {
    const err = Object.assign(new Error('Control said: UPDATE space_member_history_windows failed'), {
      code: 'member_history_window_invalid',
      status: 422,
      details: { field: 'since_at' },
    })
    for (const tr of [no, en]) expect(translateApiError(err, tr)).not.toMatch(/UPDATE|Control said|space_member/)
  })
})
