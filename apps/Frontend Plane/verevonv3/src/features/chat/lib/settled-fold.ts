/**
 * Settled-turn folding (EP-14 S1): the work a settled agent turn did folds into
 * one row, and its answer stays in view.
 *
 * Pure. The page passes the thread and the turn a run is still on, and gets back
 * the fold of each turn that has one. Nothing here is stored or sent, and the
 * folded state is per view (see `SettledTurnFold`). Behind
 * `VITE_CHAT_SETTLED_FOLD`, which the page reads: off, no plan is built and the
 * transcript renders as it did.
 */

import { formatElapsedWait, isGeneratedImageFile } from '../components/chat-media-markdown'
import type { ChatTurn } from '../components/chat-types'
import type { ConversationNodeKind } from '@/shared/chat-nodes/types'
import type { Translator } from './stream-activity'

/** Only the literal `true` enables the fold, as for the other chat switches. */
export function isSettledFoldEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/** Fixed per build, like every `VITE_` switch: read once, at load. */
export const settledFoldEnabled = isSettledFoldEnabled(import.meta.env.VITE_CHAT_SETTLED_FOLD)

/** The fields of a turn the fold reads. */
export type FoldTurn = Pick<
  ChatTurn,
  'id' | 'role' | 'streaming' | 'status' | 'reasoning' | 'toolCalls' | 'files' | 'timing'
>

/** What the transcript needs to draw one settled turn's fold row. */
export type SettledFold = {
  /** Starts folded: an older turn, and not the last one settled. */
  defaultFolded: boolean
  /** Tool calls the turn ran. */
  steps: number
  /** Files the turn produced. Generated images are left out: they show as previews. */
  files: number
  /** The gateway's own span for the turn, in ms. Absent when the turn reported none. */
  durationMs?: number
}

/**
 * Where each node kind renders. A fold body holds only the process trail:
 * reasoning, tool chips and tool steps. The answer, every decision, every notice,
 * every error and every deliverable stays in view. The record is exhaustive, so a
 * new kind fails the build here until someone decides where it goes, and until
 * then it renders in view.
 */
const FOLDED_KINDS: Record<ConversationNodeKind, boolean> = {
  reasoning: true,
  'tool-chips': true,
  steps: true,
  answer: false,
  grounding: false,
  confidence: false,
  'memory-recall': false,
  'source-scope': false,
  'search-provenance': false,
  // A lifecycle notice (a retry, a reduced answer) can matter after the turn settles,
  // and the notice type carries no error marker that would tell the two kinds apart.
  'lifecycle-notices': false,
  truncated: false,
  attachments: false,
  approvals: false,
  'plan-approval': false,
  'image-previews': false,
  files: false,
  artifacts: false,
  'follow-ups': false,
}

/**
 * A settled turn: an assistant turn whose outcome is recorded (done, error or
 * stopped) and that the run is not still on. A streaming or waiting turn is not.
 */
export function isSettled(turn: FoldTurn, activeTurnId?: string | null): boolean {
  return turn.role === 'assistant' && !turn.streaming && turn.status !== 'waiting' && turn.id !== activeTurnId
}

/**
 * The span the gateway measured for the turn, from admission to its end. It runs
 * on the gateway's clock, so no browser time enters it, and a restored turn keeps
 * it with the other timing totals. A turn that reported none gets no time at all,
 * not an estimate.
 */
function turnDurationMs(turn: FoldTurn): number | undefined {
  const ms = turn.timing?.serverMs
  return ms !== undefined && Number.isFinite(ms) && ms >= 0 ? ms : undefined
}

/**
 * The fold of each settled turn that ran something to fold, keyed by turn id. A
 * turn that ran no reasoning and no tool step has no row.
 *
 * The default keeps the reader's place: a turn older than the last two is folded,
 * except the last settled turn, which stays open.
 */
export function foldPlan(
  turns: readonly FoldTurn[],
  options: { activeTurnId?: string | null } = {},
): Map<string, SettledFold> {
  const { activeTurnId } = options
  let lastSettled = -1
  turns.forEach((turn, index) => {
    if (isSettled(turn, activeTurnId)) lastSettled = index
  })
  const plan = new Map<string, SettledFold>()
  turns.forEach((turn, index) => {
    if (!isSettled(turn, activeTurnId)) return
    const steps = turn.toolCalls?.length ?? 0
    if (steps === 0 && !turn.reasoning) return
    plan.set(turn.id, {
      defaultFolded: index < turns.length - 2 && index !== lastSettled,
      steps,
      files: (turn.files ?? []).filter((file) => !isGeneratedImageFile(file)).length,
      durationMs: turnDurationMs(turn),
    })
  })
  return plan
}

/**
 * A turn's node entries, split around its fold: the entries the fold body holds,
 * and the entries that stay in view. Both keep the order the turn derived them in.
 */
export function splitForFold<T extends { node: { kind: ConversationNodeKind } }>(
  entries: readonly T[],
): { folded: T[]; visible: T[] } {
  const folded: T[] = []
  const visible: T[] = []
  for (const entry of entries) {
    if (FOLDED_KINDS[entry.node.kind]) folded.push(entry)
    else visible.push(entry)
  }
  return { folded, visible }
}

/**
 * The row's label, for example "Worked for 4 min 12 s · 9 steps · 2 files". The
 * time is left out when the turn reported none, and so is a zero count.
 */
export function settledFoldSummary(fold: SettledFold, i18n: Translator): string {
  const { durationMs, steps, files } = fold
  const parts = [
    durationMs === undefined
      ? i18n.tr('Arbeidet', 'Worked')
      : i18n.tr(`Arbeidet i ${formatElapsedWait(durationMs)}`, `Worked for ${formatElapsedWait(durationMs)}`),
  ]
  if (steps > 0) parts.push(steps === 1 ? i18n.tr('ett steg', '1 step') : i18n.tr(`${steps} steg`, `${steps} steps`))
  if (files > 0) parts.push(files === 1 ? i18n.tr('én fil', '1 file') : i18n.tr(`${files} filer`, `${files} files`))
  return parts.join(' · ')
}
