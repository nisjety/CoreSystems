/**
 * Conversation nodes: an assistant turn's renderable content as an ordered list
 * of typed, data-only nodes.
 *
 * # Why
 *
 * `AssistantMessage` rendered a fixed sequence of ~15 `<Show>` blocks, each with
 * its own inline condition read off `ChatTurn`. Adding a node meant editing that
 * component; reordering meant reading all of it; and there was no way to ask
 * "what will this turn render?" without rendering it. Every new backend
 * capability (memory recall, truncation, reasoning) has arrived as one more
 * `<Show>` in the middle of that list.
 *
 * Separating *what a turn contains* (this module, pure) from *how each piece
 * looks* (the registry, in the feature layer) makes both testable on their own,
 * and makes the DeepSeek acceptance gate expressible at all: three different
 * paths build a `ChatTurn` — a full thread load, a paged prepend, and a live
 * stream append — and the node list they derive must be identical. That is a
 * property about data, and it cannot be asserted against JSX.
 *
 * # Nodes are data
 *
 * No node carries a callback or a component. Callbacks live in
 * [`ConversationNodeContext`], supplied once at render time. That is what lets
 * the derivation be a pure function of the turn, comparable with `toEqual`, and
 * serializable if a future surface ever needs to ship one over the wire.
 */

import type { AutonomyRung } from '@/shared/api/chat-client'
import type { JSX } from '@solidjs/web'
import type {
  ChatArtifact,
  ChatKnowledgeGrounding,
  ChatToolCall,
  ChatTurn,
  ComposerAttachment,
  ComposerToolId,
  GeneratedFile,
  GeneratedImagePreview,
  Citation,
} from '@/features/chat/components/chat-types'
import type { Approval, ApprovalDecision } from '@/shared/api/orchestration-client'
import type { RecalledMemory } from '@/shared/api/chat-client'


/**
 * The answer region's three mutually exclusive states.
 *
 * Kept as ONE node rather than three, because they share a DOM wrapper (the
 * streaming-class div) and are chosen by a nested `Show`/fallback pair. Splitting
 * them would let a future change render two at once, which the nesting made
 * impossible.
 */
export type AnswerState =
  /** Waiting with nothing to show yet — the thinking indicator. */
  | { state: 'pending' }
  /** The stream failed; the content field holds the error text. */
  | { state: 'failed'; message: string }
  /** Real content (possibly still streaming, possibly stopped early). */
  | { state: 'content'; content: string; streaming: boolean; stopped: boolean }

export type ConversationNode =
  | { kind: 'reasoning'; text: string; streaming: boolean }
  | { kind: 'answer'; answer: AnswerState }
  | { kind: 'grounding'; grounding: ChatKnowledgeGrounding }
  | { kind: 'low-confidence'; confidence: number; hasEvidence: boolean }
  | { kind: 'memory-recall'; count: number; memories: RecalledMemory[] }
  | { kind: 'truncated'; stopReason: string }
  | { kind: 'tool-chips'; tools: ComposerToolId[] }
  | { kind: 'attachments'; attachments: ComposerAttachment[] }
  | { kind: 'steps'; calls: ChatToolCall[] }
  | { kind: 'approvals'; approvals: Approval[] }
  /**
   * A plan-mode turn awaiting (or carrying) an autonomy grant.
   *
   * Derived rather than stored: whether a turn planned is a fact about the turn,
   * and whether it has been granted authority is a fact about its run. Recomputed
   * so a grant made after the turn rendered shows up without a migration.
   */
  | { kind: 'plan-approval'; grantedRung?: AutonomyRung; pending: boolean; error?: string }
  | { kind: 'image-previews'; previews: GeneratedImagePreview[] }
  | { kind: 'files'; files: GeneratedFile[] }
  | { kind: 'artifacts'; artifacts: ChatArtifact[] }
  | { kind: 'follow-ups'; suggestions: string[] }

export type ConversationNodeKind = ConversationNode['kind']

/** Callbacks a node's renderer may need. Supplied once, never inside a node. */
export type ConversationNodeContext = {
  onViewSteps: () => void
  /** Open the contextual attachment canvas without replacing the transcript. */
  onViewAttachments?: (attachmentId: string) => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
  /** Grant this turn's run the authority to execute its plan. */
  onApprovePlan: (rung: AutonomyRung, justification: string) => void
  onSelectFollowUp?: (text: string) => void
  onRegenerate: () => void
  /** Explicit `[n]` answer markers can resolve only against this turn's
   * validated citation list; absent entries stay as plain text. */
  citations?: readonly Citation[]
}

/** One node kind's renderer. */
export type ConversationNodeDefinition<K extends ConversationNodeKind = ConversationNodeKind> = {
  kind: K
  render: (node: Extract<ConversationNode, { kind: K }>, ctx: ConversationNodeContext) => JSX.Element
}

export type { ChatTurn }
