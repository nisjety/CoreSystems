import { describe, expect, it } from 'vitest'
import { presentSpaceDenialText } from '@/shared/api/space-denials'
import { describeApiError, ERROR_COPY, LocalizedError, translateApiError } from './errors'

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

// The copy the owed Space slices added: a code that is renamed or dropped fails here, instead of falling back silently
// to the generic sentence. Each describe names the slice whose owed line it answers.
const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

function expectCopyInBothLanguages(codes: readonly string[]) {
  for (const code of codes) {
    expect(ERROR_COPY[code]?.no.trim(), `${code} has Norwegian copy`).toBeTruthy()
    expect(ERROR_COPY[code]?.en.trim(), `${code} has English copy`).toBeTruthy()
  }
}

describe('WS01-S6 send copy', () => {
  it('every new WS01 code has no and en copy', () => {
    expectCopyInBothLanguages([
      'active_membership_required',
      'idempotency_key_conflict',
      'invalid_client_send_id',
      'invalid_post',
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
      // WS01-S8: the send on the agent path, answered when the same message is already received.
      'managed_run_already_started',
    ])
  })

  it('space_post_not_authorized with a typed details.reason keeps the denial copy', () => {
    const refusal = { code: 'space_post_not_authorized', status: 403, details: { reason: 'space_role_below_floor' } }
    expect(translateApiError(refusal, no)).toMatch(/^Rollen din i dette rommet er ikke høy nok til dette\./)
    expect(translateApiError(refusal, en)).toMatch(/^Your role in this Space is not high enough for this\./)
  })
})

describe('WS03-S7 translateApiError and the Space denial presenter', () => {
  it('translateApiError delegates a typed Space denial to the presenter and leaves other codes unchanged', () => {
    const denial = { code: 'space_role_below_floor', status: 403, details: { reason: 'space_role_below_floor' } }
    expect(translateApiError(denial, en)).toBe(presentSpaceDenialText(denial, en))
    expect(translateApiError({ code: 'space_not_found', status: 404 }, en)).toBe(ERROR_COPY.space_not_found?.en)
    expect(translateApiError({ code: 'upstream_unavailable', status: 503 }, en)).toBe(ERROR_COPY.upstream_unavailable?.en)
  })
})

describe('WS04-S8.spa room directory copy', () => {
  it('directory_disabled has no and en copy', () => {
    expectCopyInBothLanguages(['directory_disabled'])
  })

  it('directory_unavailable copy change does not break a pinned test', () => {
    // SpaceMemberControls.test.tsx pins this Norwegian sentence for a 503 from the directory read; the copy now says
    // "the directory" because the rooms route answers the same code.
    expect(translateApiError({ code: 'directory_unavailable', status: 503 }, no)).toBe('Katalogen er utilgjengelig akkurat nå. Prøv igjen om litt.')
    expect(translateApiError({ code: 'directory_unavailable', status: 503 }, en)).not.toMatch(/People search/)
  })
})

describe('WS08 wake and mention copy', () => {
  it('every new WS08 code has no and en copy', () => {
    expectCopyInBothLanguages([
      'agent_not_bound',
      'agent_respond_to_denied',
      'invalid_mentions',
      'mention_text_missing',
      'mention_unavailable',
      'person_not_in_room',
      'space_addressed_wake_disabled',
      'wake_not_authorized',
      'wake_owner_required',
      'wake_requires_single_agent',
      'wake_requires_thread',
      'wake_trigger_mismatch',
      // D18: a Do wake under the member's history window is refused before its run exists.
      'wake_history_window_do_unsupported',
    ])
  })

  it('policy_conflict, invalid_policy, space_role_cannot_manage_agent and agent_binding_not_found have no and en copy', () => {
    expectCopyInBothLanguages(['policy_conflict', 'invalid_policy', 'space_role_cannot_manage_agent', 'agent_binding_not_found'])
  })
})

describe('WS11-S4 attention copy', () => {
  it('the three WS11 codes have no and en copy', () => {
    expectCopyInBothLanguages(['space_attention_disabled', 'invalid_level', 'space_index_unavailable'])
  })
})

describe('WS13-S4b permalink copy', () => {
  it('entry_not_found has no and en copy', () => {
    expectCopyInBothLanguages(['entry_not_found'])
  })
})

describe('WS17-S5 agent_not_addable', () => {
  it('agent_not_addable with details.reason resolves the reason sentence, not the Space denial line', () => {
    const refusal = { code: 'agent_not_addable', status: 409, details: { reason: 'add_policy_nobody' } }
    expect(translateApiError(refusal, no)).toBe(ERROR_COPY.add_policy_nobody?.no)
    expect(translateApiError(refusal, en)).toBe(ERROR_COPY.add_policy_nobody?.en)
    expect(translateApiError(refusal, en)).not.toBe(presentSpaceDenialText(refusal, en))
    // A reason this build does not know keeps the code's own sentence.
    const unknown = { code: 'agent_not_addable', status: 409, details: { reason: 'not_a_reason_yet' } }
    expect(translateApiError(unknown, en)).toBe(ERROR_COPY.agent_not_addable?.en)
  })
})

describe('W3-FIX-13 memory refusals keep their own sentence', () => {
  it('memory_declassify_not_authorized and memory_retract_not_authorized carrying details.reason keep their own copy', () => {
    for (const code of ['memory_declassify_not_authorized', 'memory_retract_not_authorized']) {
      const refusal = { code, status: 403, details: { reason: 'space_role_below_floor' } }
      expect(translateApiError(refusal, no), code).toBe(ERROR_COPY[code]?.no)
      expect(translateApiError(refusal, en), code).toBe(ERROR_COPY[code]?.en)
      expect(translateApiError(refusal, en), code).not.toBe(presentSpaceDenialText(refusal, en))
    }
  })
})

describe('W3-FIX-14 private panel and send refusal copy', () => {
  it('private_panel_unavailable has copy with no Nothing was changed claim', () => {
    expectCopyInBothLanguages(['private_panel_unavailable'])
    expect(ERROR_COPY.private_panel_unavailable?.en).not.toMatch(/Nothing was changed/)
    expect(ERROR_COPY.private_panel_unavailable?.no).not.toMatch(/Ingenting er endret/)
  })

  it('private_panel_creation_unavailable has no entry', () => {
    expect(ERROR_COPY.private_panel_creation_unavailable).toBeUndefined()
  })

  it('space_decision_unavailable copy does not say the message is kept', () => {
    expectCopyInBothLanguages(['space_decision_unavailable'])
    expect(ERROR_COPY.space_decision_unavailable?.en).not.toMatch(/kept/)
    expect(ERROR_COPY.space_decision_unavailable?.no).not.toMatch(/beholdt/)
  })
})
