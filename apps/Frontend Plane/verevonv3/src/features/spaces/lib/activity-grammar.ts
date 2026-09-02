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
  const href = owner
    ? `/spaces/${encodeURIComponent(owner)}?thread_id=${encodeURIComponent(source.thread_id)}`
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
 * The one-line sentence for an item.
 *
 * Kept here rather than in the component so the wording is testable without
 * rendering, and so a second surface reusing this grammar cannot drift from it.
 */
export function activitySentence(item: SpaceActivityItem): string {
  return `${item.verb}: ${item.object} → ${item.outcome.label}`
}
