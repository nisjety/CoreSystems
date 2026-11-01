/**
 * Why a room turn answered without the organisation's knowledge (D1-242).
 *
 * Control declines the retrieval half of a thread's decision for a reason; the BFF turns it into a
 * code from a closed set and Model Plane states it on the turn as a `retrieval_notice` whose `code`
 * is `retrieval_declined_<reason>`. The gateway sends no sentence, so the wording is this file's.
 * A code outside the set shows nothing: the answer reads exactly as it did before the reason
 * existed. Only the code is ever read, and it is looked up, never displayed.
 */

export type SpaceRetrievalDeclineReason =
  | 'entitlement_off'
  | 'policy_not_provisioned'
  | 'membership_required'
  | 'audience_not_current'
  | 'role_below_floor'
  | 'zdr_forbids_effect'
  | 'space_not_active'
  | 'not_authorized'

const NOTICE_CODE_PREFIX = 'retrieval_declined_'

const COPY: Record<SpaceRetrievalDeclineReason, { no: string; en: string }> = {
  entitlement_off: {
    no: 'Svaret bruker ikke organisasjonens kunnskap: organisasjonen din har ikke slått på kunnskapssøk.',
    en: 'This answer does not use your organisation’s knowledge: your organisation has not turned knowledge search on.',
  },
  policy_not_provisioned: {
    no: 'Svaret bruker ikke organisasjonens kunnskap: kunnskapssøk er ikke satt opp for dette rommet ennå.',
    en: 'This answer does not use your organisation’s knowledge: knowledge search is not set up for this Space yet.',
  },
  membership_required: {
    no: 'Svaret bruker ikke organisasjonens kunnskap: du er ikke medlem av dette rommet.',
    en: 'This answer does not use your organisation’s knowledge: you are not a member of this Space.',
  },
  audience_not_current: {
    no: 'Svaret bruker ikke organisasjonens kunnskap: tilgangen din til rommet er under oppdatering. Prøv igjen om litt.',
    en: 'This answer does not use your organisation’s knowledge: your access to this Space is being updated. Try again shortly.',
  },
  role_below_floor: {
    no: 'Svaret bruker ikke organisasjonens kunnskap: rollen din i rommet tillater ikke kunnskapssøk.',
    en: 'This answer does not use your organisation’s knowledge: your role in this Space does not allow knowledge search.',
  },
  zdr_forbids_effect: {
    no: 'Svaret bruker ikke organisasjonens kunnskap: Zero Data Retention tillater ikke kunnskapssøk i rom.',
    en: 'This answer does not use your organisation’s knowledge: Zero Data Retention does not allow knowledge search in Spaces.',
  },
  space_not_active: {
    no: 'Svaret bruker ikke organisasjonens kunnskap: rommet er ikke aktivt.',
    en: 'This answer does not use your organisation’s knowledge: this Space is not active.',
  },
  not_authorized: {
    no: 'Svaret bruker ikke organisasjonens kunnskap: kunnskapssøk ble ikke godkjent for dette rommet.',
    en: 'This answer does not use your organisation’s knowledge: knowledge search was not authorised for this Space.',
  },
}

/**
 * The reason a `retrieval_notice` code states, or `undefined` for any other notice or an unknown reason.
 * The same code is what Model Plane stores on the answer (`retrievalDeclined`), so a reopened thread reads it
 * back through this function too.
 */
export function spaceRetrievalDeclineReason(code: string): SpaceRetrievalDeclineReason | undefined {
  if (!code.startsWith(NOTICE_CODE_PREFIX)) return undefined
  return asSpaceRetrievalDeclineReason(code.slice(NOTICE_CODE_PREFIX.length))
}

/** A bare reason (the form the room's own turn types carry), or `undefined` for a string outside the closed set. */
export function asSpaceRetrievalDeclineReason(value: string): SpaceRetrievalDeclineReason | undefined {
  return Object.prototype.hasOwnProperty.call(COPY, value) ? (value as SpaceRetrievalDeclineReason) : undefined
}

/** The one-line note for a reason, in the Space's two languages. */
export function spaceRetrievalDeclineCopy(reason: SpaceRetrievalDeclineReason): { no: string; en: string } {
  return COPY[reason]
}
