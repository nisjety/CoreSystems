import { z } from 'zod'

const suggestedFieldsSchema = z.object({
  category: z.string().trim().min(1).max(80).optional(),
  intent: z.string().trim().min(1).max(120).optional(),
  work_type: z.enum(['customer_case', 'internal_work', 'incident']).optional(),
  priority: z.enum(['low', 'normal', 'high', 'urgent']).optional(),
  severity: z.enum(['low', 'medium', 'high', 'critical']).optional(),
  // AI may recommend an active-work state only. Closing, resolving, and
  // snoozing stay explicit operator decisions outside the proposal contract.
  status: z.enum(['open', 'waiting_customer', 'waiting_team', 'escalated']).optional(),
  team_id: z.string().trim().min(1).max(120).optional(),
  team_name: z.string().trim().min(1).max(160).optional(),
}).strict().refine((value) => Object.keys(value).length > 0, {
  message: 'At least one ticket field is required.',
}).refine((value) => Boolean(value.team_id) === Boolean(value.team_name), {
  message: 'A team proposal requires both team_id and team_name.',
})

const incidentProposalSchema = z.object({
  title: z.string().trim().min(1).max(300),
  customer_impact: z.string().trim().min(1).max(2_000),
}).strict()

const problemProposalSchema = z.object({
  title: z.string().trim().min(1).max(300),
  summary: z.string().trim().min(1).max(2_000),
  root_cause: z.string().trim().max(2_000).optional(),
}).strict()

const inboxTriageProposalSchema = z.object({
  confidence: z.number().finite().min(0).max(1),
  reason: z.string().trim().min(1).max(500),
  suggestedFields: suggestedFieldsSchema,
  incident: incidentProposalSchema.optional(),
  problem: problemProposalSchema.optional(),
}).strict().superRefine((value, context) => {
  const isIncident = value.suggestedFields.work_type === 'incident'
  if (isIncident && !value.incident) {
    context.addIssue({ code: z.ZodIssueCode.custom, message: 'Incident triage requires an incident proposal.', path: ['incident'] })
  }
  if (!isIncident && value.incident) {
    context.addIssue({ code: z.ZodIssueCode.custom, message: 'Only incident triage may contain an incident proposal.', path: ['incident'] })
  }
  if (!isIncident && value.problem) {
    context.addIssue({ code: z.ZodIssueCode.custom, message: 'Only incident triage may contain a Problem candidate.', path: ['problem'] })
  }
})

export type InboxTriageProposal = z.infer<typeof inboxTriageProposalSchema>

const resolutionPlanSchema = z.object({
  summary: z.string().trim().min(1).max(500),
  reply: z.string().trim().min(1).max(8_000).optional(),
  internal_note: z.string().trim().min(1).max(8_000).optional(),
  triage: inboxTriageProposalSchema.optional(),
}).strict().refine((value) => Boolean(value.reply || value.internal_note || value.triage), {
  message: 'A resolution plan needs at least one reviewable suggestion.',
})

export type InboxResolutionPlan = {
  summary: string
  reply?: string
  internalNote?: string
  triage?: InboxTriageProposal
}

export type InboxTriageTeam = { id: string; name: string }

function jsonCandidate(text: string): string {
  const trimmed = text.trim()
  const fenced = trimmed.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/i)
  return (fenced?.[1] ?? trimmed).trim()
}

/**
 * Models are asked for a compact JSON triage proposal. Accept only the exact,
 * bounded shape that the reviewed `tickets.classify_conversation` contract can
 * persist; prose, malformed JSON, and unsupported fields stay transient.
 */
export function parseInboxTriageProposal(
  text: string,
  options: { ticketTeams?: readonly InboxTriageTeam[] } = {},
): InboxTriageProposal | null {
  try {
    const candidate = JSON.parse(jsonCandidate(text)) as unknown
    const parsed = inboxTriageProposalSchema.safeParse(candidate)
    if (!parsed.success) return null
    const { team_id: teamId, team_name: teamName } = parsed.data.suggestedFields
    if (!teamId || !teamName) return parsed.data
    const validTeam = options.ticketTeams?.some((team) => team.id === teamId && team.name === teamName)
    return validTeam ? parsed.data : null
  } catch {
    return null
  }
}

/**
 * A resolution plan is a bounded collection of suggestions, never an action
 * request. Each suggested item is rendered separately and must enter its
 * existing review path only after a deliberate operator click.
 */
export function parseInboxResolutionPlan(
  text: string,
  options: { ticketTeams?: readonly InboxTriageTeam[] } = {},
): InboxResolutionPlan | null {
  try {
    const candidate = JSON.parse(jsonCandidate(text)) as unknown
    const parsed = resolutionPlanSchema.safeParse(candidate)
    if (!parsed.success) return null
    const triage = parsed.data.triage
    if (triage) {
      const { team_id: teamId, team_name: teamName } = triage.suggestedFields
      if (teamId && teamName && !options.ticketTeams?.some((team) => team.id === teamId && team.name === teamName)) {
        return null
      }
    }
    return {
      summary: parsed.data.summary,
      reply: parsed.data.reply,
      internalNote: parsed.data.internal_note,
      triage,
    }
  } catch {
    return null
  }
}
