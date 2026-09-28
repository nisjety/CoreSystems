/**
 * Verb/Object/Outcome rendering grammar for a Space's activity feed.
 *
 * Every item renders as one sentence — *the agent did [verb] to [object] →
 * [outcome]* — with detail pushed behind progressive disclosure. Adapted from
 * the render-class taxonomy documented in
 * `apps/VEREVON_UI_COWORK_RESEARCH_2026-08-13.md` §2.1.
 *
 * # This is presentation, not authority
 *
 * Nothing here computes a decision, resolves a permission, or asks the gateway
 * for anything new. The BFF stays a proxy: it forwards
 * `/api/v1/spaces/:space_ref/threads` from the owning plane, and this module
 * turns what came back into readable sentences. No new gateway endpoint, no
 * server-side classification, no activity semantics parked in the BFF.
 *
 * # Why the input is an event, not a thread
 *
 * `SpaceActivityItem` is deliberately NOT `SpaceThread`. Today the only Space
 * activity a plane publishes is a conversation with a latest-run status, so
 * `activityFromThread` is the one adapter that exists. When owner planes start
 * publishing correlated run receipts, approvals, and tool steps into a Space
 * projection — which `SpacePage` already tells the reader is coming — those get
 * their own adapters and the renderer is untouched. Binding the renderer to
 * today's single source would guarantee a rewrite then.
 *
 * # The rules this file implements
 *
 * - **Semantics over transport** — say what happened, never which API carried it.
 * - **Never go dark** — absent, idle, and unknown are rendered states, not gaps.
 * - **Failures rise; reads recede** — salience follows consequence, not recency.
 * - **Resolve references** — show a title, never a raw id.
 * - **Honesty over guessing** — an unrecognized status degrades to a truthful
 *   generic row rather than inventing a richer reading of it.
 */

/**
 * The render classes implemented today.
 *
 * The source taxonomy has twelve. Implementing all twelve against data that
 * cannot distinguish them would violate this grammar's own "honesty over
 * guessing" rule, so this is the honest subset for the data that actually
 * arrives, plus the generic fallback the rule requires.
 */
export type ActivityRenderClass =
  /** A conversation in the Space. The message spine. */
  | 'conversation'
  /** A turn/run lifecycle attached to a conversation. */
  | 'run'
  /** A standing schedule that will fire in this Space. */
  | 'schedule'
  /** A background process running in this Space's sandbox workspace. */
  | 'process'
  /** An owner-plane effect performed under this Space's authority. */
  | 'operation'
  /** A grant of an effect in this Space, or its withdrawal. */
  | 'authority'
  /** A decision a person still owes, or one they already made. */
  | 'approval'
  /** Recognized item whose state could not be read. Never dropped. */
  | 'unknown'

/**
 * How consequential an item is, which drives ordering and emphasis.
 *
 * "Failures rise; reads recede": a failed run outranks a completed one even if
 * the completed one is newer, because the supervisor's question is "what needs
 * me", not "what happened last".
 */
export type ActivitySalience = 'critical' | 'attention' | 'normal' | 'muted'

/** Terminality of the outcome, for tone rather than colour alone. */
export type ActivityTone = 'pending' | 'success' | 'failure' | 'neutral'

export interface ActivityOutcome {
  /** Short human phrase: "fullført", "venter på godkjenning". */
  readonly label: string
  readonly tone: ActivityTone
  /**
   * True while the item is still moving. "Mutate in place": a live row updates
   * itself rather than appending a new line per transition.
   */
  readonly live: boolean
}

export interface SpaceActivityItem {
  /** Stable identity, so a live row can be updated rather than duplicated. */
  readonly id: string
  readonly renderClass: ActivityRenderClass
  /** The verb, already in Norwegian and already past/continuous tense. */
  readonly verb: string
  /** The resolved object — a title, never an id. */
  readonly object: string
  readonly outcome: ActivityOutcome
  readonly salience: ActivitySalience
  /** ISO timestamp if the source had one. */
  readonly at?: string
  /** Where the item leads when opened. */
  readonly href?: string
  /**
   * Secondary evidence, already phrased, shown under the sentence.
   *
   * Progressive disclosure without a second component: the sentence stays one
   * line and the proof a supervisor asks for next — token counts, the ticket an
   * effect produced, who granted an authority — sits directly beneath it. Never
   * carries anything the row's own source did not say.
   */
  readonly detail?: readonly string[]
}

/**
 * Every run status the backing services actually emit, mapped to how it reads.
 *
 * Enumerated from the real vocabulary in session-core rather than invented, so
 * an unmapped value genuinely means "new status we have not taught this
 * grammar yet" and correctly falls through to the honest generic row.
 */
const RUN_STATUS: Readonly<
  Record<string, { label: string; tone: ActivityTone; salience: ActivitySalience; live: boolean }>
> = {
  // Needs a human. The whole point of "failures rise".
  awaiting_approval: { label: 'venter på godkjenning', tone: 'pending', salience: 'critical', live: true },
  failed: { label: 'feilet', tone: 'failure', salience: 'critical', live: false },
  timed_out: { label: 'tidsavbrutt', tone: 'failure', salience: 'critical', live: false },
  denied: { label: 'avslått', tone: 'failure', salience: 'attention', live: false },
  rejected: { label: 'avvist', tone: 'failure', salience: 'attention', live: false },

  // In flight.
  running: { label: 'kjører', tone: 'pending', salience: 'attention', live: true },
  queued: { label: 'i kø', tone: 'pending', salience: 'normal', live: true },
  pending: { label: 'venter', tone: 'pending', salience: 'normal', live: true },

  // Done and unremarkable — these recede.
  completed: { label: 'fullført', tone: 'success', salience: 'muted', live: false },
  succeeded: { label: 'fullført', tone: 'success', salience: 'muted', live: false },
  approved: { label: 'godkjent', tone: 'success', salience: 'muted', live: false },
  cancelled: { label: 'avbrutt', tone: 'neutral', salience: 'muted', live: false },
}

const SALIENCE_ORDER: Readonly<Record<ActivitySalience, number>> = {
  critical: 0,
  attention: 1,
  normal: 2,
  muted: 3,
}

/**
 * The subset of a Space thread this grammar reads.
 *
 * Structural, not an import of `SpaceThread`: the adapter must keep working if
 * that type gains fields, and this module must not depend on a client owned by
 * another surface.
 */
export interface ThreadLikeActivitySource {
  readonly thread_id: string
  /** Owning Space reference. When present, activity must stay in that Space. */
  readonly space_id?: string
  readonly title?: string
  readonly preview?: string
  readonly updated_at?: string
  readonly latest_run_id?: string
  readonly latest_run_status?: string
  readonly latest_run_updated_at?: string
}

/** "Resolve references": a readable object, never a raw id. */
function resolveObject(source: ThreadLikeActivitySource): string {
  const title = source.title?.trim()
  if (title) return title
  const preview = source.preview?.trim()
  if (preview) return preview.length > 80 ? `${preview.slice(0, 80)}…` : preview
  return 'Uten tittel'
}

/**
 * Adapt one Space thread into activity items.
 *
 * A thread with a run yields two items — the conversation itself and its run —
 * because they answer different questions: "what is being discussed" and "what
 * does it need from me". Collapsing them would bury the second.
 */
export function activityFromThread(source: ThreadLikeActivitySource): SpaceActivityItem[] {
  const object = resolveObject(source)
  // Space activity is owned by the Space surface. Keep the legacy Chat
  // fallback only for older projections that predate `space_id`; current
  // SpaceThread responses always include it, so a foreign Space thread cannot
  // be adopted by Chat through a new activity link.
  const owner = source.space_id?.trim()
  // The comment above used to promise that "a foreign Space thread cannot be
  // adopted by Chat through a new activity link" while this very expression
  // handed out exactly that link whenever `space_id` was missing. The fallback
  // is gone: an activity item whose owning Space is unknown renders WITHOUT a
  // link rather than routing into Chat, which would adopt a thread Chat does
  // not own (and, since item 2's origin guard, could only show read-only).
  // `#chat` because that is where the conversation — and, when a run is paused,
  // the decision that unblocks it — actually lives. Landing the reader on
  // whichever tab the hash happened to hold would show them a count of work
  // needing them and no way to do it.
  const href = owner
    ? `/spaces/${encodeURIComponent(owner)}?thread_id=${encodeURIComponent(source.thread_id)}#chat`
    : undefined

  const conversation: SpaceActivityItem = {
    id: `thread:${source.thread_id}`,
    renderClass: 'conversation',
    verb: 'Samtale',
    object,
    outcome: { label: 'åpen', tone: 'neutral', live: false },
    salience: 'normal',
    ...(source.updated_at ? { at: source.updated_at } : {}),
    href,
  }

  const status = source.latest_run_status?.trim()
  if (!status) return [conversation]

  const known = RUN_STATUS[status]
  const run: SpaceActivityItem = known
    ? {
        id: `run:${source.latest_run_id ?? source.thread_id}`,
        renderClass: 'run',
        verb: 'Kjøring',
        object,
        outcome: { label: known.label, tone: known.tone, live: known.live },
        salience: known.salience,
        ...(source.latest_run_updated_at ? { at: source.latest_run_updated_at } : {}),
        href,
      }
    : {
        // "Honesty over guessing": name the status verbatim instead of
        // pretending to understand it. "Never go dark": still rendered.
        id: `run:${source.latest_run_id ?? source.thread_id}`,
        renderClass: 'unknown',
        verb: 'Kjøring',
        object,
        outcome: { label: status, tone: 'neutral', live: false },
        salience: 'normal',
        ...(source.latest_run_updated_at ? { at: source.latest_run_updated_at } : {}),
        href,
      }

  return [conversation, run]
}

/**
 * Order a feed: consequence first, then recency inside a band.
 *
 * Deliberately not newest-first. A run that failed an hour ago outranks a
 * conversation touched a minute ago, because only one of them is waiting on
 * someone. Stable for equal keys so live rows do not jitter between refreshes.
 */
export function orderActivity(items: readonly SpaceActivityItem[]): SpaceActivityItem[] {
  return [...items].sort((a, b) => {
    const bySalience = SALIENCE_ORDER[a.salience] - SALIENCE_ORDER[b.salience]
    if (bySalience !== 0) return bySalience
    const at = a.at ?? ''
    const bt = b.at ?? ''
    if (at === bt) return 0
    // Items with no timestamp sort last within their band rather than first.
    if (!at) return 1
    if (!bt) return -1
    return at < bt ? 1 : -1
  })
}

/** Build an ordered feed from Space threads. */
export function buildSpaceActivity(
  threads: readonly ThreadLikeActivitySource[],
): SpaceActivityItem[] {
  return orderActivity(threads.flatMap(activityFromThread))
}

/**
 * One run as the Work tab sees it.
 *
 * The Activity tab derives a run row from the THREAD's latest-run summary, which
 * is all the thread projection carries. Work reads the runs themselves, so it
 * gets each run's own goal and status rather than only the newest per thread —
 * the difference between "this conversation is working" and "these four things
 * are running".
 *
 * Deliberately the same `RUN_STATUS` vocabulary as the thread-derived row, so a
 * run that is waiting says the same thing in both tabs. An unmapped status
 * still renders, verbatim, as the generic class.
 */
export interface RunLikeActivitySource {
  readonly id?: string
  readonly run_id?: string
  readonly thread_id?: string
  readonly space_id?: string
  readonly goal?: string
  readonly status?: string
  readonly updated_at?: string | number
  readonly created_at?: string | number
  /**
   * Cost and effort evidence the run contract has always carried. Work reads
   * only goal and status; Activity is where "what did this cost" is asked, so
   * these feed `runCostDetail` rather than the sentence itself.
   */
  readonly input_tokens?: number
  readonly output_tokens?: number
  readonly steps_completed?: number
  readonly agent_id?: string
  readonly error?: string
}

function asIsoTime(value: string | number | undefined): string | undefined {
  if (value === undefined || value === null) return undefined
  if (typeof value === 'number') {
    // Session Core sends run timestamps as epoch SECONDS.
    const millis = value > 1e12 ? value : value * 1000
    const date = new Date(millis)
    return Number.isNaN(date.getTime()) ? undefined : date.toISOString()
  }
  const trimmed = value.trim()
  return trimmed ? trimmed : undefined
}

export function activityFromRun(source: RunLikeActivitySource): SpaceActivityItem | null {
  const runId = (source.id ?? source.run_id ?? '').trim()
  if (!runId) return null
  const object = source.goal?.trim() || 'Uten beskrevet mål'
  const owner = source.space_id?.trim()
  const threadId = source.thread_id?.trim()
  const href = owner && threadId
    ? `/spaces/${encodeURIComponent(owner)}?thread_id=${encodeURIComponent(threadId)}#chat`
    : undefined
  const status = source.status?.trim()
  const known = status ? RUN_STATUS[status] : undefined
  const at = asIsoTime(source.updated_at) ?? asIsoTime(source.created_at)
  return {
    id: `work-run:${runId}`,
    renderClass: known ? 'run' : 'unknown',
    verb: 'Kjøring',
    object,
    outcome: known
      ? { label: known.label, tone: known.tone, live: known.live }
      : // "Honesty over guessing": an unrecognized status is named as it came.
        { label: status || 'ukjent tilstand', tone: 'neutral', live: false },
    salience: known ? known.salience : 'normal',
    ...(at ? { at } : {}),
    ...(href ? { href } : {}),
  }
}

/**
 * One standing schedule as the Work tab sees it.
 *
 * A schedule is not a run: nothing has happened yet, so its outcome is what it
 * will do rather than what it did. Disabled is called disabled rather than
 * hidden — a schedule somebody switched off is part of the answer to "what is
 * set up here", and dropping it would make the room look emptier than it is.
 */
export interface ScheduleLikeActivitySource {
  readonly id?: string
  readonly name?: string
  readonly description?: string
  readonly schedule_expr?: string
  readonly enabled?: boolean
  readonly next_fire_at?: string | number | null
  readonly last_fire_at?: string | number | null
  readonly space_ref?: string
}

export function activityFromSchedule(
  source: ScheduleLikeActivitySource,
): SpaceActivityItem | null {
  const id = (source.id ?? '').trim()
  if (!id) return null
  const object = source.name?.trim() || source.description?.trim() || 'Uten navn'
  const expression = source.schedule_expr?.trim()
  const enabled = source.enabled !== false
  return {
    id: `work-schedule:${id}`,
    renderClass: 'schedule',
    verb: 'Planlagt',
    object,
    outcome: enabled
      ? {
          label: expression ? `kjører ${expression}` : 'aktiv',
          tone: 'pending',
          // Not "live": nothing is moving right now, and a pulsing row would
          // claim otherwise.
          live: false,
        }
      : { label: 'slått av', tone: 'neutral', live: false },
    // A schedule needs attention only when it is off; an active one is
    // background furniture and recedes, per "reads recede".
    salience: enabled ? 'muted' : 'normal',
    ...(asIsoTime(source.next_fire_at ?? undefined)
      ? { at: asIsoTime(source.next_fire_at ?? undefined) as string }
      : {}),
  }
}

/**
 * One background process as the Work tab sees it (S4.2).
 *
 * A process is neither a run nor a schedule, and the difference matters to the
 * reader. A run belongs to a turn somebody took; a schedule is something that
 * will happen. A process is something that is happening NOW, started by a turn
 * that may be long over — which is exactly why it earns a row: it is the one
 * kind of work in a room that nobody is currently watching.
 *
 * `LOST` is `critical`, above a plain failure, for the same reason the
 * operation grammar makes `unknown` critical: it means the host that owned the
 * process went away and we cannot say what happened to it. A supervisor who
 * reads past that has been misled.
 *
 * The command is already redacted upstream — the host scrubs before it
 * registers — so there is no unredacted form to leak into the room.
 */
export interface ProcessLikeActivitySource {
  readonly process_id?: string
  readonly state?: string
  readonly exit_code?: number | null
  readonly end_reason?: string
  readonly command?: string | null
  readonly started_at?: string | number | null
  readonly expires_at?: string | number | null
  readonly space_ref?: string
}

const PROCESS_STATE: Readonly<
  Record<string, { label: string; tone: ActivityTone; salience: ActivitySalience; live: boolean }>
> = {
  STARTING: { label: 'starter', tone: 'pending', salience: 'normal', live: true },
  RUNNING: { label: 'kjører', tone: 'pending', salience: 'normal', live: true },
  KILLED: { label: 'stoppet', tone: 'failure', salience: 'attention', live: false },
  EXPIRED: { label: 'tidsavbrutt', tone: 'failure', salience: 'attention', live: false },
  LOST: { label: 'verten forsvant — utfall ukjent', tone: 'failure', salience: 'critical', live: false },
}

export function activityFromProcess(
  source: ProcessLikeActivitySource,
): SpaceActivityItem | null {
  const id = (source.process_id ?? '').trim()
  if (!id) return null
  const state = (source.state ?? '').trim().toUpperCase()
  const object = source.command?.trim() || 'Bakgrunnsprosess'

  // EXITED is the only state whose meaning depends on a second field, so it is
  // resolved here rather than in the table: exit 0 is a success and anything
  // else is a failure the reader should see, not a neutral "finished".
  let outcome = PROCESS_STATE[state]
  if (state === 'EXITED') {
    const code = source.exit_code ?? 0
    outcome =
      code === 0
        ? { label: 'ferdig', tone: 'success', salience: 'muted', live: false }
        : { label: `avsluttet med kode ${code}`, tone: 'failure', salience: 'attention', live: false }
  }

  return {
    id: `work-process:${id}`,
    renderClass: 'process',
    verb: 'Bakgrunnsprosess',
    object,
    outcome: outcome
      ? { label: outcome.label, tone: outcome.tone, live: outcome.live }
      : // Honesty over guessing, as everywhere else in this grammar: a state
        // this build has not been taught is named as it came rather than
        // flattened into "finished".
        { label: state.toLowerCase() || 'ukjent tilstand', tone: 'neutral', live: false },
    salience: outcome ? outcome.salience : 'normal',
    // A live process is shown by when it will STOP, not when it started: "what
    // needs me" is answered by the deadline, and a start time three hours ago
    // tells a supervisor nothing actionable.
    ...(asIsoTime(source.expires_at ?? undefined)
      ? { at: asIsoTime(source.expires_at ?? undefined) as string }
      : asIsoTime(source.started_at ?? undefined)
        ? { at: asIsoTime(source.started_at ?? undefined) as string }
        : {}),
  }
}

/**
 * The Work tab's feed: what is running, then what is scheduled.
 *
 * Ordered by the same consequence rule as Activity, so a failed run outranks a
 * healthy schedule regardless of which is newer.
 */
export function buildSpaceWork(
  runs: readonly RunLikeActivitySource[],
  schedules: readonly ScheduleLikeActivitySource[],
  processes: readonly ProcessLikeActivitySource[] = [],
): SpaceActivityItem[] {
  const items = [
    ...runs.map(activityFromRun),
    ...schedules.map(activityFromSchedule),
    ...processes.map(activityFromProcess),
  ].filter((item): item is SpaceActivityItem => item !== null)
  return orderActivity(items)
}

/**
 * One owner-plane effect, as the room's record reads it.
 *
 * The status vocabulary is `conversation_ticket_operations`' own CHECK
 * constraint, not an invention, so an unmapped value genuinely means the
 * ledger grew a state this grammar has not been taught.
 *
 * `unknown` is deliberately `critical`. It is the only state that means "we
 * cannot tell whether this effect happened", and a supervisor who reads past it
 * has been misled by the ordering — it outranks a plain failure, which at least
 * resolved.
 */
const OPERATION_STATUS: Readonly<
  Record<string, { label: string; tone: ActivityTone; salience: ActivitySalience; live: boolean }>
> = {
  unknown: { label: 'utfall er ukjent', tone: 'failure', salience: 'critical', live: false },
  pending_control_commit: { label: 'venter på godkjent reservasjon', tone: 'pending', salience: 'attention', live: true },
  reserved: { label: 'reservert, ikke utført', tone: 'pending', salience: 'attention', live: true },
  cancelled: { label: 'avbrutt før effekt', tone: 'neutral', salience: 'normal', live: false },
  completed: { label: 'utført', tone: 'success', salience: 'muted', live: false },
}

/**
 * The action id as a phrase a person recognises.
 *
 * Falls back to the raw id: an unmapped action is still a real effect, and
 * `tickets.create` read verbatim is worse than a translation but far better
 * than a dropped row.
 */
function operationVerb(actionId: string): string {
  if (actionId === 'tickets.create') return 'Opprettet sak'
  return actionId.trim() || 'Effekt'
}

export interface OperationLikeActivitySource {
  readonly operation_id?: string
  readonly action_id?: string
  readonly status?: string
  readonly subject_id?: string
  readonly granted_by_user_id?: string
  readonly ticket_id?: string
  readonly terminal_reason?: string
  readonly created_at?: string
  readonly updated_at?: string
}

export function activityFromOperation(
  source: OperationLikeActivitySource,
): SpaceActivityItem | null {
  const operationId = source.operation_id?.trim()
  if (!operationId) return null
  const status = source.status?.trim()
  const known = status ? OPERATION_STATUS[status] : undefined
  const at = asIsoTime(source.updated_at) ?? asIsoTime(source.created_at)
  const detail: string[] = []
  // Only on `completed` does the ledger name what the effect produced, and its
  // own constraint guarantees that. An absent id here means the effect is not
  // claimed to have landed — never render it as one that did.
  if (source.ticket_id?.trim()) detail.push(`Sak ${source.ticket_id.trim()}`)
  if (source.terminal_reason?.trim()) detail.push(source.terminal_reason.trim())
  return {
    id: `operation:${operationId}`,
    renderClass: known ? 'operation' : 'unknown',
    verb: operationVerb(source.action_id?.trim() ?? ''),
    object: source.subject_id?.trim() || 'Ukjent utfører',
    outcome: known
      ? { label: known.label, tone: known.tone, live: known.live }
      : { label: status || 'ukjent tilstand', tone: 'neutral', live: false },
    salience: known ? known.salience : 'normal',
    ...(at ? { at } : {}),
    ...(detail.length ? { detail } : {}),
  }
}

export interface AuthorityLikeActivitySource {
  readonly grant_id?: string
  readonly action_id?: string
  readonly subject_id?: string
  readonly created_by_user_id?: string
  readonly created_at?: string
  readonly revoked_at?: string
  readonly revoked_by_user_id?: string
}

/**
 * One grant of an effect in this Space, read as its latest state.
 *
 * A revoked grant renders as the revocation, not the grant: the room's current
 * question is "may this agent still do that here", and the answer is no. The
 * grant's own time survives in the detail line, so the history is not lost —
 * it is just not the headline.
 *
 * Deliberately NOT two rows. The ledger stores one row per grant with an
 * optional revocation, so emitting a synthetic pair would invent an event
 * ordering the source never recorded.
 */
export function activityFromAuthority(
  source: AuthorityLikeActivitySource,
): SpaceActivityItem | null {
  const grantId = source.grant_id?.trim()
  if (!grantId) return null
  const revokedAt = asIsoTime(source.revoked_at)
  const grantedAt = asIsoTime(source.created_at)
  const subject = source.subject_id?.trim() || 'Ukjent subjekt'
  const detail: string[] = []
  const actor = revokedAt
    ? source.revoked_by_user_id?.trim()
    : source.created_by_user_id?.trim()
  if (actor) detail.push(revokedAt ? `Trukket av ${actor}` : `Gitt av ${actor}`)
  if (revokedAt && grantedAt) detail.push(`Gitt ${grantedAt}`)
  if (source.action_id?.trim()) detail.push(source.action_id.trim())
  return {
    id: `authority:${grantId}`,
    renderClass: 'authority',
    verb: revokedAt ? 'Fjernet fullmakt' : 'Ga fullmakt',
    object: subject,
    outcome: revokedAt
      ? { label: 'ikke lenger tillatt', tone: 'neutral', live: false }
      : { label: 'tillatt her', tone: 'success', live: false },
    // A withdrawn authority is the row that explains a later refusal, so it
    // does not recede the way a completed effect does.
    salience: revokedAt ? 'normal' : 'muted',
    ...(revokedAt ?? grantedAt ? { at: revokedAt ?? grantedAt } : {}),
    ...(detail.length ? { detail } : {}),
  }
}

/**
 * Approval states as the model gateway actually emits them.
 *
 * The wire value is the full protobuf enum name (`APPROVAL_STATE_REQUESTED`),
 * which is exactly the mismatch that once made every pending approval invisible
 * to this app — see `canonicalApprovalStatus` in the orchestration client. This
 * matches on substrings for the same reason: tolerate either convention rather
 * than silently rendering nothing.
 */
function approvalOutcome(
  raw: string | undefined,
): { label: string; tone: ActivityTone; salience: ActivitySalience; live: boolean } {
  const value = (raw ?? '').toUpperCase()
  if (value.includes('REQUESTED') || value.includes('PENDING')) {
    return { label: 'venter på en avgjørelse', tone: 'pending', salience: 'critical', live: true }
  }
  if (value.includes('GRANTED')) {
    return { label: 'godkjent', tone: 'success', salience: 'muted', live: false }
  }
  if (value.includes('DENIED')) {
    return { label: 'avslått', tone: 'failure', salience: 'attention', live: false }
  }
  if (value.includes('TIMED_OUT') || value.includes('EXPIRED')) {
    return { label: 'utløpt uten avgjørelse', tone: 'failure', salience: 'attention', live: false }
  }
  return { label: value.toLowerCase() || 'ukjent tilstand', tone: 'neutral', salience: 'normal', live: false }
}

export interface ApprovalLikeActivitySource {
  readonly id?: string
  readonly approval_id?: string
  readonly run_id?: string
  readonly kind?: string
  readonly status?: string
  readonly state?: string
  readonly detail?: string
  readonly summary?: string
  readonly requested_by?: string
  readonly created_at?: string
  readonly space_id?: string
}

export function activityFromApproval(
  source: ApprovalLikeActivitySource,
): SpaceActivityItem | null {
  const approvalId = (source.id ?? source.approval_id ?? '').trim()
  if (!approvalId) return null
  const outcome = approvalOutcome(source.status ?? source.state)
  const runId = source.run_id?.trim()
  const owner = source.space_id?.trim()
  const detail: string[] = []
  if (source.detail?.trim()) detail.push(source.detail.trim())
  else if (source.summary?.trim()) detail.push(source.summary.trim())
  if (source.requested_by?.trim()) detail.push(`Bedt om av ${source.requested_by.trim()}`)
  return {
    id: `approval:${approvalId}`,
    renderClass: 'approval',
    verb: 'Godkjenning',
    object: source.kind?.trim() || 'Handling uten navngitt type',
    outcome: { label: outcome.label, tone: outcome.tone, live: outcome.live },
    salience: outcome.salience,
    ...(asIsoTime(source.created_at) ? { at: asIsoTime(source.created_at) } : {}),
    ...(owner && runId
      ? { href: `/spaces/${encodeURIComponent(owner)}?run_id=${encodeURIComponent(runId)}#arbeid` }
      : {}),
    ...(detail.length ? { detail } : {}),
  }
}

/**
 * A run's own effort, and its cost when a real figure exists, as a detail line
 * under its sentence.
 *
 * `RunDetail` carries `input_tokens`, `output_tokens` and `steps_completed`.
 * Only the steps are real today: session-core does not track token usage and
 * reports both token fields as a literal `0` ("owned by the inference path",
 * `run_service_grpc.rs`). The actual per-run cost lives in cost-core's
 * `cost_entries.run_id` and is not joined into the run listing by any plane.
 * So a zero total is treated as "no figure", never printed — `0 tokens` under
 * every run was a false statement this adapter made until 2026-09-08. When the
 * fields are non-zero they are printed as tokens, not currency: pricing is a
 * Control-owned concern and a browser-side multiplication would be an invented
 * number.
 */
function runCostDetail(source: RunLikeActivitySource): string[] {
  const detail: string[] = []
  const input = typeof source.input_tokens === 'number' ? source.input_tokens : 0
  const output = typeof source.output_tokens === 'number' ? source.output_tokens : 0
  if (input + output > 0) {
    detail.push(`${input + output} tokens`)
  }
  if (typeof source.steps_completed === 'number' && source.steps_completed > 0) {
    detail.push(`${source.steps_completed} steg`)
  }
  if (source.agent_id?.trim()) detail.push(source.agent_id.trim())
  // An error message is the single most useful thing on a failed row, so it
  // goes last where the eye lands after the outcome.
  if (source.error?.trim()) detail.push(source.error.trim())
  return detail
}

/**
 * Build the room's record from every plane that could answer.
 *
 * Threads are the message spine; runs add each run's own outcome and cost;
 * approvals add the decisions a person owes or made; operations add the effects
 * that actually left the system; authority adds who was allowed to cause them.
 * All five go through one salience ordering, so "what needs me" is the top of
 * the list regardless of which plane it came from.
 */
export function buildSpaceRecord(input: {
  readonly threads?: readonly ThreadLikeActivitySource[]
  readonly runs?: readonly RunLikeActivitySource[]
  readonly approvals?: readonly ApprovalLikeActivitySource[]
  readonly operations?: readonly OperationLikeActivitySource[]
  readonly authority?: readonly AuthorityLikeActivitySource[]
}): SpaceActivityItem[] {
  const items = [
    ...(input.threads ?? []).flatMap(activityFromThread),
    ...(input.runs ?? []).map((run) => {
      const item = activityFromRun(run)
      if (!item) return null
      const detail = runCostDetail(run)
      return detail.length ? { ...item, detail } : item
    }),
    ...(input.approvals ?? []).map(activityFromApproval),
    ...(input.operations ?? []).map(activityFromOperation),
    ...(input.authority ?? []).map(activityFromAuthority),
  ].filter((item): item is SpaceActivityItem => item !== null)
  return orderActivity(items)
}

/**
 * The one-line sentence for an item.
 *
 * Kept here rather than in the component so the wording is testable without
 * rendering, and so a second surface reusing this grammar cannot drift from it.
 */
export function activitySentence(item: SpaceActivityItem): string {
  return `${item.verb}: ${item.object} → ${item.outcome.label}`
}
