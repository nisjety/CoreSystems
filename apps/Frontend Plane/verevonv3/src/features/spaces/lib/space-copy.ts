/**
 * The Space copy catalogue: every status, verb, outcome and fallback the
 * activity grammar and the thread presentation say, as `{ no, en }` pairs.
 *
 * One wording per language per concept, so the Activity tab, the Work tab, the
 * room timeline and the pulse rail cannot drift apart. The Norwegian text is the
 * historical output, kept byte-for-byte; English follows `threadStatus`'s words
 * and is lower-cased inside a sentence.
 *
 * Builders take a trailing `tr: Tr = noTr`; the default keeps today's Norwegian
 * output, so callers that have not been taught a locale are unchanged. Raw
 * server tokens are never routed through here (honesty over guessing).
 */

export type Tr = (no: string, en: string) => string

/** The default translator: Norwegian, which is what every caller got before locale support. */
export const noTr: Tr = (no) => no

export interface CopyPair {
  readonly no: string
  readonly en: string
}

const p = (no: string, en: string): CopyPair => ({ no, en })

/** Read a pair through a translator. */
export function say(tr: Tr, pair: CopyPair): string {
  return tr(pair.no, pair.en)
}

export const COPY = {
  // Verbs (the "did what" of an activity sentence).
  verb: {
    conversation: p('Samtale', 'Conversation'),
    run: p('Kjøring', 'Run'),
    scheduled: p('Planlagt', 'Scheduled'),
    process: p('Bakgrunnsprosess', 'Background process'),
    approval: p('Godkjenning', 'Approval'),
    ticketCreated: p('Opprettet sak', 'Created ticket'),
    effect: p('Effekt', 'Effect'),
    authorityRevoked: p('Fjernet fullmakt', 'Removed authority'),
    authorityGranted: p('Ga fullmakt', 'Granted authority'),
  },

  // Run outcomes, lower-case because they sit inside a sentence.
  run: {
    awaitingApproval: p('venter på godkjenning', 'needs approval'),
    failed: p('feilet', 'failed'),
    timedOut: p('tidsavbrutt', 'timed out'),
    denied: p('avslått', 'denied'),
    rejected: p('avvist', 'rejected'),
    running: p('kjører', 'running'),
    queued: p('i kø', 'queued'),
    pending: p('venter', 'waiting'),
    completed: p('fullført', 'completed'),
    approved: p('godkjent', 'approved'),
    cancelled: p('avbrutt', 'cancelled'),
  },

  // Background process states.
  process: {
    starting: p('starter', 'starting'),
    running: p('kjører', 'running'),
    killed: p('stoppet', 'stopped'),
    expired: p('tidsavbrutt', 'timed out'),
    lost: p('verten forsvant — utfall ukjent', 'the host disappeared — outcome unknown'),
    done: p('ferdig', 'done'),
  },

  // Owner-plane effect (operation) states.
  operation: {
    unknown: p('utfall er ukjent', 'outcome unknown'),
    pendingControlCommit: p('venter på godkjent reservasjon', 'waiting for an approved reservation'),
    reserved: p('reservert, ikke utført', 'reserved, not carried out'),
    cancelled: p('avbrutt før effekt', 'cancelled before effect'),
    completed: p('utført', 'carried out'),
  },

  // Authority grants.
  authority: {
    noLongerAllowed: p('ikke lenger tillatt', 'no longer allowed'),
    allowedHere: p('tillatt her', 'allowed here'),
  },

  // Approval decisions.
  approval: {
    waiting: p('venter på en avgjørelse', 'waiting for a decision'),
    granted: p('godkjent', 'approved'),
    denied: p('avslått', 'denied'),
    expired: p('utløpt uten avgjørelse', 'expired without a decision'),
  },

  // Conversation / schedule outcomes.
  conversationOpen: p('åpen', 'open'),
  scheduleActive: p('aktiv', 'active'),
  scheduleOff: p('slått av', 'switched off'),

  // Fallbacks for an absent title, goal, name or state.
  fallback: {
    untitled: p('Uten tittel', 'Untitled'),
    noGoal: p('Uten beskrevet mål', 'No stated goal'),
    unnamed: p('Uten navn', 'Unnamed'),
    unknownActor: p('Ukjent utfører', 'Unknown actor'),
    unknownSubject: p('Ukjent subjekt', 'Unknown subject'),
    unnamedAction: p('Handling uten navngitt type', 'Action with no named type'),
    unknownState: p('ukjent tilstand', 'unknown state'),
  },

  // Feed-level text.
  feed: {
    empty: p('Ingen aktivitet i dette rommet ennå.', 'No activity in this Space yet.'),
  },

  // Thread-level status (capitalised, shared by the timeline, header and pulse rail).
  thread: {
    untitled: p('Samtale uten tittel', 'Untitled conversation'),
    open: p('Samtale åpen', 'Conversation open'),
    awaitingApproval: p('Venter på godkjenning', 'Needs approval'),
    running: p('Arbeider', 'Working'),
    queued: p('I kø', 'Queued'),
    completed: p('Fullført', 'Completed'),
    failed: p('Feilet', 'Failed'),
    stopped: p('Stoppet', 'Stopped'),
  },
} as const

/** "Sak T-1" / "Ticket T-1". */
export function ticketDetail(tr: Tr, ticketId: string): string {
  return `${tr('Sak', 'Ticket')} ${ticketId}`
}

export function revokedByDetail(tr: Tr, actor: string): string {
  return `${tr('Trukket av', 'Withdrawn by')} ${actor}`
}

export function grantedByDetail(tr: Tr, actor: string): string {
  return `${tr('Gitt av', 'Granted by')} ${actor}`
}

export function grantedAtDetail(tr: Tr, at: string): string {
  return `${tr('Gitt', 'Granted')} ${at}`
}

export function requestedByDetail(tr: Tr, who: string): string {
  return `${tr('Bedt om av', 'Requested by')} ${who}`
}

/** "5 steg" / "1 step", "5 steps". */
export function stepsDetail(tr: Tr, count: number): string {
  return `${count} ${count === 1 ? tr('steg', 'step') : tr('steg', 'steps')}`
}

export function runsScheduleOutcome(tr: Tr, expression: string): string {
  return `${tr('kjører', 'runs')} ${expression}`
}

export function exitedWithCode(tr: Tr, code: number): string {
  return `${tr('avsluttet med kode', 'exited with code')} ${code}`
}
