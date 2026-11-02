import type { RoomRunStepClass, RoomRunStepState, SpaceRunWork } from '@/shared/api/spaces-client/run-work'
import { spaceThreadHref } from './space-thread-deep-link'
import { COPY, say, type CopyPair, type Tr } from './space-copy'
import type { SpaceActivityItem } from './activity-grammar'
import { elapsedSeconds, runningSteps, totalSteps, type SpaceRunView } from './space-run-view'

/**
 * Words for a live room run (WS09-S5), in both languages.
 *
 * A member sees which KIND of step the agent is taking, never what it was given or returned: the server
 * sends a closed class, so the sentence is chosen here from a fixed table. Nothing in this file takes
 * free text from a frame. `activity-grammar.ts` is not changed; `activityFromRunView` hands the live run
 * to it as an ordinary `SpaceActivityItem`, so Work and Activity say it in the same Verb / Object /
 * Outcome form as every other row.
 */

const p = (no: string, en: string): CopyPair => ({ no, en })

/** What the agent is doing, present tense, per step class. */
const DOING: Readonly<Record<RoomRunStepClass, CopyPair>> = {
  search_knowledge: p('Søker i kunnskapen', 'Searching the knowledge base'),
  web: p('Søker på nettet', 'Searching the web'),
  code: p('Kjører kode', 'Running code'),
  artifact: p('Arbeider med et dokument', 'Working on a document'),
  lookup: p('Slår opp', 'Looking something up'),
  effect: p('Utfører en handling', 'Carrying out an action'),
  memory: p('Bruker minnet', 'Using memory'),
  delegate: p('Ber en annen agent om hjelp', 'Asking another agent for help'),
  plan: p('Planlegger', 'Planning'),
  check: p('Kontrollerer', 'Checking'),
  tool: p('Bruker et verktøy', 'Using a tool'),
}

const STATE: Readonly<Record<RoomRunStepState, CopyPair>> = {
  running: p('pågår', 'in progress'),
  ok: p('ferdig', 'done'),
  error: p('feilet', 'failed'),
  denied: p('avslått', 'denied'),
}

export function stepDoing(stepClass: RoomRunStepClass, tr: Tr): string {
  return say(tr, DOING[stepClass])
}

export function stepStateLabel(state: RoomRunStepState, tr: Tr): string {
  return say(tr, STATE[state])
}

/** One step as a short line: "Searching the web, done". */
export function stepLine(step: { readonly class: RoomRunStepClass; readonly state: RoomRunStepState }, tr: Tr): string {
  return `${stepDoing(step.class, tr)}, ${stepStateLabel(step.state, tr)}`
}

/** The headline of a live run: what the newest running step is, else a plain "Working". */
export function runHeadline(view: Pick<SpaceRunView, 'steps'>, tr: Tr): string {
  const running = view.steps.filter((step) => step.state === 'running')
  const last = running[running.length - 1] ?? view.steps[view.steps.length - 1]
  return last ? stepDoing(last.class, tr) : tr('Jobber', 'Working')
}

/** "42 s" or "1 min 5 s". */
export function durationLabel(totalSeconds: number, tr: Tr): string {
  const seconds = Math.max(0, Math.floor(totalSeconds))
  if (seconds < 60) return tr(`${seconds} s`, `${seconds} s`)
  const minutes = Math.floor(seconds / 60)
  const rest = seconds % 60
  return rest === 0 ? tr(`${minutes} min`, `${minutes} min`) : tr(`${minutes} min ${rest} s`, `${minutes} min ${rest} s`)
}

/**
 * "Used 4 tools in 12 s" for the collapsed disclosure after a run. The time is left out when it is not
 * known; `steer` adds the count of messages the owner steered in.
 */
export function workSummaryLine(
  work: Pick<SpaceRunWork, 'steps' | 'steer'>,
  tr: Tr,
  elapsedSecondsTotal?: number,
): string {
  const n = work.steps.length
  const base =
    n === 0
      ? tr('Brukte ingen verktøy', 'Used no tools')
      : n === 1
        ? tr('Brukte 1 verktøy', 'Used 1 tool')
        : tr(`Brukte ${n} verktøy`, `Used ${n} tools`)
  const timed =
    elapsedSecondsTotal && elapsedSecondsTotal > 0
      ? tr(`${base} på ${durationLabel(elapsedSecondsTotal, tr)}`, `${base} in ${durationLabel(elapsedSecondsTotal, tr)}`)
      : base
  if (work.steer <= 0) return timed
  return work.steer === 1
    ? tr(`${timed}, 1 styring fra eieren`, `${timed}, 1 steer from the owner`)
    : tr(`${timed}, ${work.steer} styringer fra eieren`, `${timed}, ${work.steer} steers from the owner`)
}

/** Sum of the recorded step times in whole seconds, or 0 when none carry a time. */
export function summedStepSeconds(work: Pick<SpaceRunWork, 'steps'>): number {
  const ms = work.steps.reduce((total, step) => total + (step.ms ?? 0), 0)
  return Math.round(ms / 1000)
}

const UNAVAILABLE: Readonly<Record<string, CopyPair>> = {
  run_stream_expired: p('Live-visningen av denne kjøringen er avsluttet.', 'The live view of this run has ended.'),
  too_many_live_views: p('For mange live-visninger er åpne.', 'Too many live views are open.'),
  zdr_feature_not_supported: p(
    'Live-visning er ikke tilgjengelig med null datalagring.',
    'Live view is not available with zero data retention.',
  ),
  space_read_not_authorized: p('Du har ikke tilgang til å følge denne kjøringen.', 'You are not allowed to follow this run.'),
}

/** Why the live view is off, in words; undefined for a code that has none (the legacy row says nothing extra). */
export function unavailableNote(code: string | null, tr: Tr): string | undefined {
  const pair = code ? UNAVAILABLE[code] : undefined
  return pair ? say(tr, pair) : undefined
}

/**
 * A live run as one Work / Activity row. Its id is the one `activityFromRun` gives the same run
 * (`work-run:<id>`), so the feed shows the live row IN PLACE of the polled one rather than twice.
 */
export function activityFromRunView(view: SpaceRunView, spaceRef: string, tr: Tr, now: number = Date.now()): SpaceActivityItem {
  const href = view.threadId ? spaceThreadHref(spaceRef, view.threadId) : undefined
  const ended = view.finished
  const label = ended
    ? ended.outcome === 'completed'
      ? say(tr, COPY.run.completed)
      : ended.outcome === 'stopped'
        ? say(tr, COPY.run.cancelled)
        : say(tr, COPY.run.failed)
    : runHeadline(view, tr)
  const detail: string[] = []
  const steps = view.steps.slice(-3)
  for (const step of steps) detail.push(stepLine(step, tr))
  const rest = totalSteps(view) - steps.length
  if (rest > 0) detail.push(rest === 1 ? tr('1 steg til', '1 more step') : tr(`${rest} steg til`, `${rest} more steps`))
  const seconds = elapsedSeconds(view, now)
  if (!ended && seconds > 0) detail.push(durationLabel(seconds, tr))
  return {
    id: `work-run:${view.runId}`,
    renderClass: 'run',
    verb: say(tr, COPY.verb.run),
    object: view.agentName || tr('Agent', 'Agent'),
    outcome: {
      label,
      tone: ended ? (ended.outcome === 'failed' ? 'failure' : ended.outcome === 'stopped' ? 'neutral' : 'success') : 'pending',
      live: !ended,
    },
    salience: ended ? 'muted' : runningSteps(view).length > 0 ? 'attention' : 'normal',
    ...(view.startedAt ? { at: new Date(view.startedAt * 1000).toISOString() } : {}),
    ...(href ? { href } : {}),
    ...(detail.length > 0 ? { detail } : {}),
  }
}

/**
 * Merge live run rows into a feed's items: a live row replaces the polled row with the same id and the
 * rest keep their order. Only views that are `live` count; a run whose stream failed leaves the polled
 * row alone. `runIds` is the set of runs THIS Space's listing named: the run store is shared by every
 * room, and a view of a run the listing did not name is never drawn here.
 */
export function withLiveRunItems(
  items: readonly SpaceActivityItem[],
  views: readonly SpaceRunView[],
  runIds: ReadonlySet<string>,
  spaceRef: string,
  tr: Tr,
  now: number = Date.now(),
): SpaceActivityItem[] {
  const live = views.filter((view) => view.status === 'live' && !view.finished && runIds.has(view.runId))
  if (live.length === 0) return [...items]
  const liveItems = live.map((view) => activityFromRunView(view, spaceRef, tr, now))
  const liveIds = new Set(liveItems.map((item) => item.id))
  return [...liveItems, ...items.filter((item) => !liveIds.has(item.id))]
}
