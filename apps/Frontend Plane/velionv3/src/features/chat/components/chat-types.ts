import {
  type DashboardComposerSubmitPayload,
} from '@/features/dashboard/home/DashboardComposer'
import {
  type ChatAction,
} from '@/shared/api/chat-client'
import {
  type Approval,
} from '@/shared/api/orchestration-client'
import {
  Briefcase,
  Code2,
  GraduationCap,
  Laptop,
  Palette,
  PenLine,
  Presentation,
  WandSparkles,
  type LucideProps,
} from 'lucide-solid'
import {
  type JSX,
} from 'solid-js'

export type ComposerToolId = DashboardComposerSubmitPayload['tools'][number]
export type ComposerAttachment = DashboardComposerSubmitPayload['attachments'][number]
export type ChatTab = 'chat' | 'sources' | 'artifacts' | 'steps'
export type TaskStepStatus = 'done' | 'active' | 'waiting' | 'error' | 'stopped'
export type IconComponent = (props: LucideProps) => JSX.Element

export type ChatArtifact = {
  id: string
  kind: string
  content: string
  title: string
  version: number
}

export type ChatToolCall = {
  id: string
  name: string
  args?: unknown
  status?: string
  output?: string
  error?: string
}

export type Citation = {
  id: string
  title: string
  url: string
  snippet: string
}

export type GeneratedFile = {
  id: string
  name: string
  mime: string
  size: number
  url: string
}

export type GeneratedImagePreview = {
  id: string
  title: string
  src: string
  downloadName: string
  size: number
  artifactId?: string
}

export type ArtifactPanelItem = {
  artifact: ChatArtifact
  file?: GeneratedFile
  turn: ChatTurn
}

export type ChatGroundingSource = {
  id: string
  kind: 'knowledge'
  title: string
  snippet: string
  provider: string
  sourceType: string
  documentId: string
  href: string
  score: number
}

export type ChatGroundingFact = {
  knowledgeId: string
  documentId: string
  text: string
  score: number
  sourceTitle: string
  sourceType: string
  provider: string
  chunkIndex: number
}

export type ChatGroundingGraphNode = {
  id: string
  label: string
  kind: string
}

export type ChatGroundingGraph = {
  traceId?: string
  communitySummaries: string[]
  edgeCount: number
  nodes: ChatGroundingGraphNode[]
}

export type ChatKnowledgeGrounding = {
  mode: 'retrieve' | 'hybrid'
  query: string
  traceId?: string
  lowConfidence: boolean
  factCount: number
  sourceCount: number
  facts: ChatGroundingFact[]
  sources: ChatGroundingSource[]
  graph?: ChatGroundingGraph
}

/**
 * Below this answer-confidence score, the chat UI must surface a visible
 * caveat on the message bubble itself rather than leaving it discoverable
 * only in the Reasoning/Steps popover.
 *
 * Reference point (Model Plane `confidence.rs`): a clean, ungrounded answer
 * scores exactly `BASE = 0.72` — confident-sounding text with no hedging and
 * no knowledge-base citations. A genuinely knowledge-base-grounded answer
 * gets `GROUNDED_BONUS = 0.15` on top, landing around `0.87`. Setting the
 * threshold at `0.75` — just above the ungrounded baseline — means any
 * answer that is fluent but NOT grounded in this org's knowledge base gets
 * flagged, while a real grounded answer does not.
 *
 * Incident (2026-07-20): "tell me about aquatiq what do they do and sell"
 * returned a confidently wrong, uncited answer scored exactly 0.72; that
 * score was computed but only ever visible by opening the Reasoning popover
 * and clicking into its "Oversikt" tab — never in the actual chat bubble.
 */
export const LOW_CONFIDENCE_ANSWER_THRESHOLD = 0.75

export type ChatTurn = {
  id: string
  role: 'user' | 'assistant'
  content: string
  createdAt: string
  streaming: boolean
  tools: ComposerToolId[]
  attachments: ComposerAttachment[]
  status?: 'waiting' | 'stopped' | 'error'
  model?: string
  requestId?: string
  modelUsed?: string
  inputTokens?: number
  outputTokens?: number
  latencyMs?: number
  costUsd?: number
  confidence?: number
  reasoning?: string
  citations?: Citation[]
  toolCalls?: ChatToolCall[]
  artifacts?: ChatArtifact[]
  files?: GeneratedFile[]
  grounding?: ChatKnowledgeGrounding
  /** Orchestration run id (captured from a `paused` step) — drives approvals. */
  runId?: string
  /** Pending human-approval requests gating this agentic run's next tool. */
  pendingApprovals?: Approval[]
}

export type AgentTaskStep = {
  id: string
  title: string
  detail: string
  status: TaskStepStatus
  createdAt: string
  expandedDetail?: string
  evidence?: AgentTaskStepEvidence[]
  turnId?: string
  turnTitle?: string
}

export type AgentTaskStepEvidence = {
  id: string
  label: string
  value: string
  href?: string
}

export type AgentTaskStepSection = {
  id: string
  title: string
  createdAt: string
  steps: AgentTaskStep[]
}

export type ChatStatus = 'idle' | 'streaming' | 'error'

export type ChatState = {
  turns: ChatTurn[]
  taskSteps: AgentTaskStep[]
  status: ChatStatus
  error: string | null
  requestId: string | null
  threadId: string | null
  activeModel: string
  branchCount: number
}

export type StreamAttachment = {
  data_base64: string
  kind: 'image'
  mime_type: string
}

export type SendOptions = {
  actions?: ChatAction[]
  attachments?: StreamAttachment[]
  browseWeb?: boolean
  createdAt?: string
  displayAttachments?: ComposerAttachment[]
  generateImage?: boolean
  appendUser?: boolean
  tools?: ComposerToolId[]
}

export type EvidenceSource = (Citation & { kind: 'web' }) | ChatGroundingSource

export type MarkdownBlock =
  | { kind: 'code'; lang: string; text: string }
  | { kind: 'heading'; level: 1 | 2 | 3; text: string }
  | { kind: 'hr' }
  | { kind: 'list'; ordered: boolean; items: string[] }
  | { kind: 'paragraph'; text: string }
  | { kind: 'quote'; text: string }

// ── Prompt chips ──────────────────────────────────────────────────────────────

export const PRIMARY_PROMPTS = [
  { label: 'Create slides', prompt: 'Create a concise slide outline for a customer support leadership update.', icon: Presentation },
  { label: 'Build website', prompt: 'Build a focused website plan for a high-converting support automation page.', icon: Code2 },
  { label: 'Develop apps', prompt: 'Plan a desktop app workflow for agents managing customer conversations.', icon: Laptop },
  { label: 'Design', prompt: 'Design a refined support workflow with clear states, handoffs, and escalation paths.', icon: Palette },
  { label: 'Write', prompt: 'Write a polished customer reply that is concise, helpful, and on-brand.', icon: PenLine },
] as const

export const OVERFLOW_PROMPTS = [
  { label: 'Learn', prompt: 'Teach me the most important concepts behind customer support automation.', icon: GraduationCap },
  { label: 'Code', prompt: 'Help me implement a clean customer support automation feature with tests.', icon: Code2 },
  { label: 'Career chat', prompt: 'Help me prepare for a career conversation about customer experience leadership.', icon: Briefcase },
  { label: "Velion's choice", prompt: 'Choose the highest-impact next task for improving our support operations.', icon: WandSparkles },
] as const

export const TOOL_LABELS: Record<ComposerToolId, string> = {
  image: 'Create image',
  reason: 'Reason',
  research: 'Deep research',
  search: 'Search',
}

export const PROSE_ARTIFACT_KINDS = new Set(['markdown', 'md', 'doc', 'text', 'report', 'prose'])
export const CHAT_BROWSE_WEB_KEY = 'velion.chat.browseWeb.v1'
