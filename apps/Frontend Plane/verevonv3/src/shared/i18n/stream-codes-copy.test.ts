import { describe, expect, it } from 'vitest'
import { streamFailurePresentation } from '@/features/chat/components/chat-normalizers'
import { REFUSAL_TEXT } from '@/shared/api/zdr-refusals'
import { INTERNAL_FAILURE_COPY, MODE_LOCKDOWN_COPY, ERROR_COPY, translateApiError } from './errors'

// Two codes the chat stream can carry had no words of their own: `mode_lockdown` (model-gateway refuses an agentic turn in a
// chat whose agent mode allows no run, sse.rs) and `internal` (the stream error the SSE golden pins). Each reads in the
// reader's language, never as the server's English text and never as "Stream error". The three zero-retention refusals the
// gateway relays keep the catalogue's words in ERROR_COPY too, so a direct lookup reads the same sentence.

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english
const SERVER_TEXT = 'This chat is locked to plain answers, so it cannot start an agent run.'

describe('the chat stream words mode_lockdown', () => {
  it('says the agent mode allows no run, that nothing was run, and offers no retry', () => {
    const shown = streamFailurePresentation('mode_lockdown', SERVER_TEXT, en)

    expect(shown.message).toBe(MODE_LOCKDOWN_COPY.en)
    expect(shown).toMatchObject({ unsaved: true, unavailable: false, action: 'none' })
  })

  it('shows the Norwegian words to a Norwegian reader, and never the server text', () => {
    const shown = streamFailurePresentation('mode_lockdown', SERVER_TEXT, no)

    expect(shown.message).toBe(MODE_LOCKDOWN_COPY.no)
    expect(shown.message).not.toContain('locked')
  })

  it('is the same sentence over HTTP as on the stream', () => {
    expect(translateApiError({ code: 'mode_lockdown', status: 403 }, en)).toBe(MODE_LOCKDOWN_COPY.en)
    expect(translateApiError({ code: 'mode_lockdown', status: 403 }, no)).toBe(MODE_LOCKDOWN_COPY.no)
  })
})

describe('the chat stream words internal', () => {
  it('says the request could not be completed, claims nothing about what was saved, and offers a retry', () => {
    const shown = streamFailurePresentation('internal', '', en)

    expect(shown.message).toBe(INTERNAL_FAILURE_COPY.en)
    expect(shown).toMatchObject({ unsaved: false, unavailable: false, action: 'retry' })
  })

  it('is not the English fallback "Stream error" for a Norwegian reader', () => {
    expect(streamFailurePresentation('internal', '', no).message).toBe(INTERNAL_FAILURE_COPY.no)
    expect(streamFailurePresentation('internal', '', no).message).not.toBe('Stream error')
  })

  it('has no ERROR_COPY entry, so an HTTP 500 keeps the caller\'s own sentence (the status class answers it)', () => {
    expect(ERROR_COPY.internal).toBeUndefined()
    expect(translateApiError({ code: 'internal', status: 500 }, no, { no: 'Kunne ikke laste.', en: 'Could not load.' })).toBe(
      'Kunne ikke laste.',
    )
  })
})

describe('the zero-retention refusals the gateway relays have the catalogue words in ERROR_COPY', () => {
  it.each(['zdr_connection_not_acknowledged', 'zdr_posture_mismatch', 'zdr_waiver_route_mismatch'] as const)(
    '%s has the catalogue text, in both languages',
    (code) => {
      expect(ERROR_COPY[code]).toEqual({ no: REFUSAL_TEXT[code].no, en: REFUSAL_TEXT[code].en })
    },
  )
})
