import { getActionDescriptor, type ActionId } from '@/shared/actions/action-registry'
import { VEREVON_BALANCE_MODE_ID, type ChatAction } from '@/shared/api/chat-client'

// ── Preset agents ─────────────────────────────────────────────────────────────
// A small, curated set of ready-to-run tasks for the Agent Run Console. Each
// preset pins a goal template plus a fixed Model-eligible action subset.
// Browser registry actions are deliberately NOT treated as Model tools until a
// governed owner-operation adapter exists; the goal may request a proposal,
// but cannot claim an effect happened. Presets never set `planMode: false`.

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
  /** Fixed Model-eligible action subset; empty until owner adapters are live. */
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
    actionIds: [],
    defaults: defaultLaunch(),
  },
  {
    id: 'summarize-new-inquiries',
    label: 'Summarize new inquiries',
    description: 'Digest unread and unassigned conversations so nothing slips through.',
    goalTemplate:
      'Summarize all unread and unassigned inbox conversations from the last 24 hours. Group them by topic, flag anything urgent, and suggest who or which team each one should be assigned to.',
    actionIds: [],
    defaults: defaultLaunch(),
  },
  {
    id: 'refresh-knowledge-base',
    label: 'Plan a knowledge-base refresh',
    description: "Propose a Quarry re-crawl of the organization's known website.",
    goalTemplate:
      "Propose a safe refresh plan for our knowledge base from the organization's known website. Do not start a crawl; explain the evidence, expected scope, and approval needed.",
    actionIds: [],
    defaults: defaultLaunch(),
  },
  {
    id: 'triage-urgent-tickets',
    label: 'Triage & escalate urgent tickets',
    description: 'Classify open conversations and route the urgent ones to the right team.',
    goalTemplate:
      'Review open support conversations, classify which ones should become tickets, and assign the urgent ones to the right team. Escalate anything that needs a human to look at it right away.',
    actionIds: [],
    defaults: defaultLaunch(),
  },
] as const satisfies readonly PresetAgent[]

export function getPresetAgent(id: PresetAgentId): PresetAgent | undefined {
  return presetAgents.find((preset) => preset.id === id)
}

/**
 * Turn a preset's fixed action-id subset into `ChatAction[]` for
 * `ChatInvokeRequest.actions`. Empty sets are intentional until their owner
 * actions are explicitly Model-eligible and enforceable at execution time.
 */
export function presetAgentChatActions(preset: PresetAgent): ChatAction[] {
  return preset.actionIds.map((id) => ({
    id,
    name: getActionDescriptor(id)?.label ?? id,
    kind: 'tool',
  }))
}
