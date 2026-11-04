import { describe, expect, it } from 'vitest'
import { describeApiError, LocalizedError, translateApiError } from './errors'

describe('translateApiError', () => {
  it('keeps an unknown provider-send outcome distinct from a normal retryable failure', () => {
    const tr = (no: string) => no

    expect(translateApiError({ code: 'delivery_unknown' }, tr)).toMatch(/ikke send på nytt automatisk/i)
    expect(translateApiError({ code: 'send_failed' }, tr)).toMatch(/ble ikke sendt/i)
    expect(translateApiError({ code: 'delivery_unavailable' }, tr)).toMatch(/ikke sendt eller lagret/i)
  })

  // A memory save refused in a shared Space or an agent run says why. The person's
  // own pause keeps the generic copy: that choice is theirs to change, these are not.
  it('says why memory was not saved in a shared Space or an agent run', () => {
    const no = (norwegian: string) => norwegian
    const en = (_norwegian: string, english: string) => english

    expect(translateApiError({ code: 'memory_not_enabled', details: { reason: 'space_scope' } }, no)).toMatch(
      /delte Spaces, så dette ble ikke lagret/,
    )
    expect(translateApiError({ code: 'memory_not_enabled', details: { reason: 'space_scope' } }, en)).toMatch(
      /shared Spaces, so this was not saved/,
    )
    expect(translateApiError({ code: 'memory_not_enabled', details: { reason: 'agent_run_scope' } }, en)).toMatch(
      /from an agent run, so this was not saved/,
    )
    const paused = translateApiError({ code: 'memory_not_enabled', details: { reason: 'user_paused' } }, en)
    expect(paused).not.toMatch(/Spaces|agent run/)
  })

  // Owner ruling (5): a personal Microsoft/Google connection's two ownership
  // failures must surface as their own specific copy, not the generic
  // fallback — this is the seam the connector-attach picker relies on.
  it('gives the personal-connection ownership codes their own specific copy', () => {
    const tr = (no: string) => no

    expect(translateApiError({ code: 'personal_connection_forbidden' }, tr)).toMatch(/tilhører en annen bruker/i)
    expect(translateApiError({ code: 'personal_connection_no_delegated_user' }, tr)).toMatch(/logget inn som eieren/i)
  })

  // The org Zero Data Retention switch is refused with these two codes, from the
  // settings page and from organization creation alike. Each needs its own
  // sentence: the generic fallback would tell an admin "something went wrong"
  // about a refusal that is deliberate and has a way forward.
  it('gives the org ZDR switch refusals their own copy, in both languages', () => {
    const no = (norwegian: string) => norwegian
    const en = (_norwegian: string, english: string) => english

    expect(translateApiError({ code: 'zdr_no_attested_provider' }, no)).toMatch(/ingen AI-leverandør har en registrert nullbevaringsgodkjenning/i)
    expect(translateApiError({ code: 'zdr_no_attested_provider' }, en)).toMatch(/no AI provider has a recorded zero-retention attestation/i)
    expect(translateApiError({ code: 'zdr_gate_unavailable' }, no)).toMatch(/Kunne ikke kontrollere/i)
    expect(translateApiError({ code: 'zdr_gate_unavailable' }, en)).toMatch(/could not check which providers/i)
    expect(translateApiError({ code: 'zdr_no_attested_provider' }, no)).not.toMatch(/Noe gikk galt/)
  })

  // A full crawl queue is the organization's own and clears by itself: it gets
  // its own copy rather than the generic fallback.
  it('explains a full crawl queue in both languages', () => {
    const no = (norwegian: string) => norwegian
    const en = (_norwegian: string, english: string) => english

    expect(translateApiError({ code: 'crawl_backlog_full' }, no)).toMatch(/for mange gjennomganger/i)
    expect(translateApiError({ code: 'crawl_backlog_full' }, en)).toMatch(/too many crawls are waiting/i)
    expect(translateApiError({ code: 'crawl_backlog_full' }, no)).not.toMatch(/Noe gikk galt/)
  })
})

describe('crawl allowance', () => {
  it('explains a spent crawl allowance in both languages', () => {
    const no = (norwegian: string) => norwegian
    const en = (_norwegian: string, english: string) => english

    expect(translateApiError({ code: 'crawl_allowance_exhausted' }, no)).toMatch(/brukt opp kvoten/i)
    expect(translateApiError({ code: 'crawl_allowance_exhausted' }, en)).toMatch(/used up its crawl allowance/i)
    expect(translateApiError({ code: 'crawl_allowance_exhausted' }, no)).not.toMatch(/Noe gikk galt/)
  })
})

describe('LocalizedError', () => {
  const refusal = new LocalizedError({
    no: 'Du har allerede et prosjekt med dette navnet.',
    en: 'A project with this name already exists.',
  })

  it('is shown as written, in the language of the caller, instead of the caller fallback', () => {
    const fallback = { no: 'Kunne ikke lagre.', en: 'Could not save.' }
    expect(translateApiError(refusal, (no) => no, fallback)).toBe('Du har allerede et prosjekt med dette navnet.')
    expect(translateApiError(refusal, (_no, en) => en, fallback)).toBe('A project with this name already exists.')
    expect(describeApiError(refusal, (no) => no, fallback).message).toBe('Du har allerede et prosjekt med dette navnet.')
  })

  it('keeps an English message for logs and tests, and is an Error', () => {
    expect(refusal).toBeInstanceOf(Error)
    expect(refusal.message).toBe('A project with this name already exists.')
  })

  it('does not widen what is shown: a plain Error or a gateway-style error still gets the fallback', () => {
    const fallback = { no: 'Kunne ikke lagre.', en: 'Could not save.' }
    expect(translateApiError(new Error('boom: internal detail'), (no) => no, fallback)).toBe('Kunne ikke lagre.')
    expect(translateApiError({ code: 'no_such_code', status: 400, message: 'English ops text' }, (no) => no, fallback)).toBe('Kunne ikke lagre.')
  })
})
