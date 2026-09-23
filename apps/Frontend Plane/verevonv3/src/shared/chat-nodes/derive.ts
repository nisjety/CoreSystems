/**
 * `ChatTurn` → ordered [`ConversationNode`] list.
 *
 * A pure function, deliberately. The conditions and ORDER here are the exact
 * ones `AssistantMessage` used to carry inline, and both are behaviour: a
 * reordered list is a visibly different answer. Keeping them in one readable
 * function means the sequence can be reviewed as a sequence instead of
 * reconstructed from fourteen nested `<Show>` blocks.
 *
 * See `derive.test.ts` for the acceptance gate: a turn built by a full thread
 * load, a paged prepend, and a live stream append must derive the same list.
 */

import {
  buildGeneratedImagePreviews,
  imageGenerationDisplayContent,
  isGeneratedImageFile,
  isImageArtifact,
} from '@/features/chat/components/chat-media-markdown'
import { LOW_CONFIDENCE_ANSWER_THRESHOLD } from '@/features/chat/components/chat-types'
import { deriveStreamActivity } from '@/features/chat/lib/stream-activity'
import type { AnswerState, ChatTurn, ConversationNode } from './types'

/**
 * The answer region's state.
 *
 * Mirrors the old nested `Show` exactly: `emptyWaiting` wins first (nothing to
 * show yet), then an error replaces the content entirely, and only otherwise is
 * there real content. Order matters — an errored turn keeps its message in
 * `content`, so testing for content first would render the error text as if it
 * were the answer.
 */
function answerState(turn: ChatTurn, displayContent: string): AnswerState {
  const waiting = turn.status === 'waiting'
  // `since` is the turn's createdAt, stamped locally when the request was sent,
  // so the elapsed-wait counter is anchored to the turn rather than to whichever
  // mount of the indicator happens to be current. See `AnswerState`.
  if (waiting && !turn.content && !turn.reasoning) {
    // Spread rather than `activity: … ?? undefined`: a turn that has reported
    // no activity derives a node with no such key at all, so "nothing known
    // yet" and "some activity" stay distinguishable by shape.
    const activity = deriveStreamActivity(turn.toolCalls)
    return { state: 'pending', since: turn.createdAt, ...(activity ? { activity } : {}) }
  }
  if (turn.status === 'error') {
    return { state: 'failed', message: turn.content || 'Stream error' }
  }
  return {
    state: 'content',
    content: displayContent,
    streaming: waiting,
    stopped: turn.status === 'stopped',
  }
}

/**
 * Live status of a plan approval in flight.
 *
 * Passed in rather than read off the turn: a click's pending/failed state is not
 * a property of the conversation, and storing it on the turn would put it in the
 * transcript snapshot where a reload would resurrect a spinner for a click that
 * finished minutes ago.
 */
export type PlanApprovalStatus = { pending: boolean; error?: string }

export function deriveConversationNodes(
  turn: ChatTurn,
  planApproval?: PlanApprovalStatus,
): ConversationNode[] {
  const waiting = turn.status === 'waiting'
  const errored = turn.status === 'error'
  const files = turn.files ?? []
  const artifacts = turn.artifacts ?? []
  const previews = buildGeneratedImagePreviews(files, artifacts, turn.content)
  // An image-generation turn's prose is replaced by the previews; showing both
  // duplicated the answer.
  const displayContent =
    previews.length > 0 ? imageGenerationDisplayContent(turn.content) : turn.content
  const visibleFiles = files.filter((file) => !isGeneratedImageFile(file))
  const visibleArtifacts = artifacts.filter((artifact) => !isImageArtifact(artifact))

  const nodes: ConversationNode[] = []

  if (turn.reasoning) {
    nodes.push({ kind: 'reasoning', text: turn.reasoning, streaming: waiting })
  }
  nodes.push({ kind: 'answer', answer: answerState(turn, displayContent) })
  if (turn.grounding) {
    nodes.push({ kind: 'grounding', grounding: turn.grounding })
  }
  // Confidence is suppressed while waiting or errored: a partial answer has
  // no meaningful score yet, and an error's score describes nothing.
  //
  // Emitted for EVERY scored answer, not only low ones. While the node existed
  // solely below the threshold, the score appeared on some answers and silently
  // vanished on the next — the same answer quality reading as "no signal" —
  // and readers had to open Detaljer to find out whether a number existed at
  // all. `low` carries the threshold verdict so the renderer picks a caveat or
  // a quiet chip without re-deriving the rule.
  if (!waiting && !errored && turn.confidence != null) {
    nodes.push({
      kind: 'confidence',
      confidence: turn.confidence,
      low: turn.confidence < LOW_CONFIDENCE_ANSWER_THRESHOLD,
      verification: turn.verification,
      // Whether there is anything to check. The notice used to say "sjekk
      // kilder" on turns with no sources at all -- a verification path that
      // does not exist (audit item 23). The hedge itself stays ungated: the
      // 2026-07-20 incident it exists for was an uncited answer.
      //
      // A successful tool result IS evidence: a code-interpreter run that
      // printed the figures, a weather lookup, a web fetch. Counting only
      // citations produced "ingen kilder ble brukt" directly under an answer
      // whose numbers the sandbox had just verified (RUN-LOG finding 12) — a
      // caveat that contradicted the Arbeid panel one click away.
      hasEvidence: (turn.citations?.length ?? 0) > 0
        || turn.grounding != null
        || (turn.toolCalls ?? []).some((call) => !call.error && call.status !== 'error' && call.status !== 'failed'),
    })
  }
  // Falsy count is dropped, not rendered as "recalled 0": the backend emits the
  // event only when memory genuinely contributed.
  if (!waiting && turn.memoryRecallCount) {
    nodes.push({
      kind: 'memory-recall',
      count: turn.memoryRecallCount,
      // May be empty when the backend reported only a count (an older build, or
      // a turn whose entries were all unreadable). The notice degrades to the
      // count alone rather than disappearing.
      memories: turn.recalledMemories ?? [],
    })
  }
  if (!waiting && !errored && turn.stopReason) {
    nodes.push({ kind: 'truncated', stopReason: turn.stopReason })
  }
  // Chips render their own empty state, so they are unconditional — matching the
  // previous `<ToolChips>` / `<AttachmentChips>` calls, which had no `Show`.
  nodes.push({ kind: 'tool-chips', tools: turn.tools })
  nodes.push({ kind: 'attachments', attachments: turn.attachments })
  if ((turn.toolCalls?.length ?? 0) > 0) {
    nodes.push({ kind: 'steps', calls: turn.toolCalls ?? [] })
  }
  if ((turn.pendingApprovals?.length ?? 0) > 0) {
    nodes.push({ kind: 'approvals', approvals: turn.pendingApprovals ?? [] })
  }
  // Only a FINISHED plan-mode turn: the control asks a person to approve a plan,
  // and there is no plan to approve until the agent has finished describing it.
  if (turn.planMode === true && !turn.streaming) {
    nodes.push({
      kind: 'plan-approval',
      grantedRung: turn.grantedRung,
      pending: planApproval?.pending === true,
      error: planApproval?.error,
    })
  }
  if (previews.length > 0) {
    nodes.push({ kind: 'image-previews', previews })
  }
  if (visibleFiles.length > 0) {
    nodes.push({ kind: 'files', files: visibleFiles })
  }
  if (visibleArtifacts.length > 0) {
    nodes.push({ kind: 'artifacts', artifacts: visibleArtifacts })
  }
  // Follow-ups are deliberately NOT derived. VEREVON_CHAT_DESIGN.md section 5
  // lists "Perplexity's generic curiosity follow-up chips" under Explicitly
  // rejected, to be replaced with permission-scoped next actions -- and what
  // the model returns here is exactly a curiosity question ("Hva er
  // befolkningen i Oslo?" under the answer "Oslo.", live 2026-09-04, audit
  // item 22). The node kind, its renderer and `FollowUpChips` stay in place:
  // the replacement needs a server-side action-suggestion contract (scoped to
  // what this user may actually do), and inferring one from free text in the
  // browser would be the fabrication this codebase forbids. Restore the push
  // below when that contract exists, feeding it actions rather than questions.
  return nodes
}
