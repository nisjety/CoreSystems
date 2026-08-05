import { getActionDescriptor, type ActionId } from '@/shared/actions/action-registry'
import { VEREVON_BALANCE_MODE_ID, type ChatAction } from '@/shared/api/chat-client'

// ── Preset agents ─────────────────────────────────────────────────────────────
// A small, curated set of ready-to-run tasks for the Agent Run Console. Each
// preset pins a goal template plus a FIXED subset of `action-registry` ids —
// the same ids the Model Plane already understands via `createSelectedAgentToolSpecs`
// (see `src/shared/actions/agent-tools.ts`). Selecting a preset must put those
// ids on the real `runTask()` request (`ChatInvokeRequest.actions`), not just
// drop text into the composer, so the model actually receives the tool
// contracts — with their existing `requiresApproval` gating intact. Presets
// never set `planMode: false`; every preset action still runs through the
// console's normal HITL/approval deck.

export type PresetAgentId =
  | 'draft-reply-with-sources'
  | 'summarize-new-inquiries'
  | 'refresh-knowledge-base'
  | 'triage-urgent-tickets'

/** Launch defaults a preset applies to the console's existing controls. */
export type PresetAgentDefaults = {
  /** One of `VEREVON_MODES`' ids (or a concrete model id). */
  modeId: string
  /** Wire `profile` sent to model-gateway; the console only ever needs 'chat'. */
  profile: string
  browseWeb: boolean
  /** Always true — presets must stay on the approval-gated agentic run path. */
  planMode: true
}

export type PresetAgent = {
  id: PresetAgentId
  label: string
  /** Short copy shown under the label in the picker. */
  description: string
  /** Seeds the composer's goal field; the user can still edit it before running. */
  goalTemplate: string
  /** Fixed subset of `action-registry` ids exposed to the model for this preset. */
  actionIds: readonly ActionId[]
  defaults: PresetAgentDefaults
}

const defaultLaunch = (browseWeb = false): PresetAgentDefaults => ({
  modeId: VEREVON_BALANCE_MODE_ID,
  profile: 'chat',
  browseWeb,
  planMode: true,
})

/** Presets relevant to a Norwegian AI-support-agent workspace. */
export const presetAgents: readonly PresetAgent[] = [
  {
    id: 'draft-reply-with-sources',
    label: 'Draft reply with sources',
    description: 'Answer the latest inbound conversation, grounded and cited from the knowledge base.',
    goalTemplate:
      "Draft a reply to the customer's most recent inbound message. Ground the answer in our knowledge base and cite the sources you used. If the conversation looks like it needs ongoing tracking, classify it as a ticket candidate — but do not send the reply or create anything without my approval.",
    actionIds: ['tickets.classify_conversation'],
    defaults: defaultLaunch(),
  },
  {
    id: 'summarize-new-inquiries',
    label: 'Summarize new inquiries',
    description: 'Digest unread and unassigned conversations so nothing slips through.',
    goalTemplate:
      'Summarize all unread and unassigned inbox conversations from the last 24 hours. Group them by topic, flag anything urgent, and suggest who or which team each one should be assigned to.',
    actionIds: ['tickets.assign'],
    defaults: defaultLaunch(),
  },
  {
    id: 'refresh-knowledge-base',
    label: 'Refresh knowledge base from website',
    description: "Trigger a Quarry re-crawl of the organization's known website.",
    goalTemplate:
      "Refresh our knowledge base by re-crawling the organization's known website. Report what changed once the crawl completes.",
    actionIds: ['knowledge.recrawl_source', 'knowledge.crawl_site'],
    defaults: defaultLaunch(),
  },
  {
    id: 'triage-urgent-tickets',
    label: 'Triage & escalate urgent tickets',
    description: 'Classify open conversations and route the urgent ones to the right team.',
    goalTemplate:
      'Review open support conversations, classify which ones should become tickets, and assign the urgent ones to the right team. Escalate anything that needs a human to look at it right away.',
    actionIds: ['tickets.classify_conversation', 'tickets.assign', 'tickets.update'],
    defaults: defaultLaunch(),
  },
] as const satisfies readonly PresetAgent[]

export function getPresetAgent(id: PresetAgentId): PresetAgent | undefined {
  return presetAgents.find((preset) => preset.id === id)
}

/**
 * Turn a preset's fixed action-id subset into `ChatAction[]` for
 * `ChatInvokeRequest.actions`. `createSelectedAgentToolSpecs` resolves each id
 * against the real `action-registry` entry (full input schema + approval
 * metadata) as long as the id matches exactly, so callers must not rename it.
 */
export function presetAgentChatActions(preset: PresetAgent): ChatAction[] {
  return preset.actionIds.map((id) => ({
    id,
    name: getActionDescriptor(id)?.label ?? id,
    kind: 'tool',
  }))
}
