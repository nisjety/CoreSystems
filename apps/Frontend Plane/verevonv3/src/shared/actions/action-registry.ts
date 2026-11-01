import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

const sourceInput = z.object({
  sourceId: z.string().min(1),
})

const ticketCreateInput = z.object({
  conversationId: z.string().min(1),
  workType: z.enum(['customer_case', 'internal_work', 'incident']).default('customer_case'),
  priority: z.enum(['low', 'normal', 'high', 'urgent']).default('normal'),
  severity: z.enum(['low', 'medium', 'high', 'critical']).default('medium'),
  category: z.string().trim().max(80).optional(),
  intent: z.string().trim().max(120).optional(),
})

const ticketClassifyInput = z.object({
  conversationId: z.string().min(1),
  confidence: z.number().min(0).max(1),
  reason: z.string().trim().min(1).max(500),
  suggestedFields: z.record(z.string(), z.unknown()).optional(),
  evidenceMessageIds: z.array(z.string().min(1)).default([]),
})

const ticketUpdateInput = z.object({
  ticketId: z.string().min(1),
  workType: z.enum(['customer_case', 'internal_work', 'incident']).optional(),
  status: z.enum(['suggested', 'open', 'waiting_customer', 'waiting_team', 'snoozed', 'escalated', 'resolved', 'closed']).optional(),
  priority: z.enum(['low', 'normal', 'high', 'urgent']).optional(),
  severity: z.enum(['low', 'medium', 'high', 'critical']).optional(),
  category: z.string().trim().max(80).optional(),
  intent: z.string().trim().max(120).optional(),
  dueAt: z.string().datetime().nullable().optional(),
  followUpAt: z.string().datetime().nullable().optional(),
  snoozedUntil: z.string().datetime().nullable().optional(),
})

const ticketAssignInput = z.object({
  ticketId: z.string().min(1),
  assigneeUserId: z.string().trim().optional(),
  assigneeName: z.string().trim().optional(),
  teamId: z.string().trim().optional(),
  teamName: z.string().trim().optional(),
})

const ticketLinkResourceInput = z.object({
  ticketId: z.string().min(1),
  linkType: z.enum(['normal', 'parent', 'child', 'related', 'external']).optional(),
  resourceKind: z.enum(['conversation_source', 'social_post', 'campaign', 'order', 'document', 'ticket', 'external']),
  resourceId: z.string().trim().optional(),
  resourceUrl: z.string().url().refine((value) => {
    try {
      const protocol = new URL(value).protocol
      return protocol === 'http:' || protocol === 'https:'
    } catch {
      return false
    }
  }, 'resourceUrl must use http or https').optional(),
  label: z.string().trim().optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
})

const ticketResolveInput = z.object({
  ticketId: z.string().min(1),
  resolution: z.string().trim().max(500).optional(),
})

const ticketRunMacroInput = z.object({
  ticketId: z.string().min(1),
  macroId: z.string().min(1),
  expectedMacroUpdatedAt: z.string().datetime().optional(),
})

const ticketCreateMacroInput = z.object({
  name: z.string().trim().min(1).max(160),
  description: z.string().trim().max(500).optional(),
  status: z.enum(['waiting_customer', 'waiting_team', 'resolved']).default('waiting_team'),
  visibility: z.enum(['personal', 'team', 'org']).default('team'),
})

const ticketCreateChecklistInput = z.object({
  ticketId: z.string().min(1),
  name: z.string().trim().min(1).max(160),
  templateId: z.string().trim().min(1).optional(),
  items: z.array(z.string().trim().min(1).max(240)).max(25).default([]),
})

const ticketUpdateChecklistItemInput = z.object({
  ticketId: z.string().min(1),
  checklistId: z.string().min(1),
  itemId: z.string().min(1),
  completed: z.boolean(),
})

const ticketCreateSideConversationInput = z.object({
  ticketId: z.string().min(1),
  subject: z.string().trim().min(1).max(160).refine((value) => !/[\r\n]/.test(value), 'subject must be one line'),
  bodyText: z.string().trim().min(1).max(4000),
})

const ticketAddSideConversationMessageInput = z.object({
  ticketId: z.string().min(1),
  sideConversationId: z.string().min(1),
  bodyText: z.string().trim().min(1).max(4000),
})

const ticketUpdateSideConversationInput = z.object({
  ticketId: z.string().min(1),
  sideConversationId: z.string().min(1),
  status: z.enum(['open', 'closed']),
})

const ticketChatHandoffInput = z.object({
  ticketId: z.string().min(1),
})

// The support-ops schemas below (incidents/problems/SLA/automation/teams/
// views/macro-update) all route through domains::tickets in the gateway
// (apps/gateway/src/domains/tickets.rs), confirmed mounted at
// apps/gateway/src/main.rs and forwarding to conversation-core — not left as
// unverified frontend calls the way security.check_url_reputation and
// security.investigate_url turned out to be (see action-registry.test.ts).
const ticketCreateIncidentInput = z.object({
  title: z.string().trim().min(1),
  status: z.enum(['declared', 'investigating', 'monitoring', 'resolved']).optional(),
  severity: z.enum(['low', 'medium', 'high', 'critical']).optional(),
  ownerUserId: z.string().trim().optional(),
  ownerName: z.string().trim().optional(),
  customerImpact: z.string().trim().optional(),
  problemId: z.string().trim().optional(),
})

const ticketUpdateIncidentInput = z.object({
  incidentId: z.string().min(1),
  title: z.string().trim().min(1).optional(),
  status: z.enum(['declared', 'investigating', 'monitoring', 'resolved']).optional(),
  severity: z.enum(['low', 'medium', 'high', 'critical']).optional(),
  ownerUserId: z.string().trim().optional(),
  ownerName: z.string().trim().optional(),
  customerImpact: z.string().trim().optional(),
  problemId: z.string().trim().optional(),
})

const ticketLinkIncidentInput = z.object({
  incidentId: z.string().min(1),
  ticketId: z.string().min(1),
  relationship: z.enum(['affected', 'root_cause', 'related']),
})

const ticketCreateProblemInput = z.object({
  title: z.string().trim().min(1),
  status: z.enum(['investigating', 'known_error', 'resolved']).optional(),
  ownerUserId: z.string().trim().optional(),
  ownerName: z.string().trim().optional(),
  summary: z.string().trim().optional(),
  rootCause: z.string().trim().optional(),
})

const ticketUpdateProblemInput = z.object({
  problemId: z.string().min(1),
  title: z.string().trim().min(1).optional(),
  status: z.enum(['investigating', 'known_error', 'resolved']).optional(),
  ownerUserId: z.string().trim().optional(),
  ownerName: z.string().trim().optional(),
  summary: z.string().trim().optional(),
  rootCause: z.string().trim().optional(),
})

const ticketCreateSlaPolicyInput = z.object({
  name: z.string().trim().optional(),
  active: z.boolean().optional(),
  conditions: z.record(z.string(), z.unknown()).optional(),
  calendarRef: z.string().trim().optional(),
  firstResponseMinutes: z.number().int().nonnegative().optional(),
  nextResponseMinutes: z.number().int().nonnegative().optional(),
  resolutionMinutes: z.number().int().nonnegative().optional(),
})

const ticketUpdateSlaPolicyInput = z.object({
  policyId: z.string().min(1),
  name: z.string().trim().optional(),
  active: z.boolean().optional(),
  conditions: z.record(z.string(), z.unknown()).optional(),
  calendarRef: z.string().trim().optional(),
  firstResponseMinutes: z.number().int().nonnegative().optional(),
  nextResponseMinutes: z.number().int().nonnegative().optional(),
  resolutionMinutes: z.number().int().nonnegative().optional(),
})

const ticketCreateAutomationRuleInput = z.object({
  name: z.string().trim().optional(),
  eventName: z.string().trim().optional(),
  active: z.boolean().optional(),
  conditions: z.record(z.string(), z.unknown()).optional(),
  actions: z.record(z.string(), z.unknown()).optional(),
})

const ticketUpdateAutomationRuleInput = z.object({
  ruleId: z.string().min(1),
  name: z.string().trim().optional(),
  eventName: z.string().trim().optional(),
  active: z.boolean().optional(),
  conditions: z.record(z.string(), z.unknown()).optional(),
  actions: z.record(z.string(), z.unknown()).optional(),
})

const ticketCreateTeamInput = z.object({
  name: z.string().trim().min(1),
  description: z.string().trim().optional(),
  active: z.boolean().optional(),
})

const ticketUpdateTeamInput = z.object({
  teamId: z.string().min(1),
  name: z.string().trim().optional(),
  description: z.string().trim().optional(),
  active: z.boolean().optional(),
})

const ticketCreateViewInput = z.object({
  name: z.string().trim().optional(),
  scope: z.enum(['org', 'user', 'team']).optional(),
  ownerUserId: z.string().trim().optional(),
  teamId: z.string().trim().optional(),
  visibility: z.enum(['sidebar', 'hidden']).optional(),
  filter: z.record(z.string(), z.unknown()).optional(),
  sort: z.record(z.string(), z.unknown()).optional(),
  groupBy: z.string().trim().optional(),
  sidebarOrder: z.number().int().optional(),
})

const ticketUpdateViewInput = z.object({
  viewId: z.string().min(1),
  name: z.string().trim().optional(),
  scope: z.enum(['org', 'user', 'team']).optional(),
  ownerUserId: z.string().trim().optional(),
  teamId: z.string().trim().optional(),
  visibility: z.enum(['sidebar', 'hidden']).optional(),
  filter: z.record(z.string(), z.unknown()).optional(),
  sort: z.record(z.string(), z.unknown()).optional(),
  groupBy: z.string().trim().optional(),
  sidebarOrder: z.number().int().optional(),
})

// Mirrors tickets.create_macro's posture: this generic action surface only
// exposes a status-transition action, not arbitrary actions/conditions
// editing. The dedicated macro-builder UI calls updateTicketMacro directly
// (outside action execution) for that richer editing.
const ticketUpdateMacroInput = z.object({
  macroId: z.string().min(1),
  name: z.string().trim().max(160).optional(),
  description: z.string().trim().max(500).optional(),
  status: z.enum(['waiting_customer', 'waiting_team', 'resolved']).optional(),
  visibility: z.enum(['personal', 'team', 'org']).optional(),
  active: z.boolean().optional(),
})

// The spaces.* schemas below all forward to the same domains::spaces route
// handlers the direct REST routes use (apps/gateway/src/domains/spaces.rs,
// exposed to the action dispatcher via pub(crate) rather than proxied a
// second time), so per-room grant checks and the create/bind-then-confirm
// sequence stay in exactly one place.
const spaceCreatePersonalInput = z.object({
  name: z.string().trim().min(1).optional(),
})

const spaceEnsureOrganizationRoomInput = z.object({
  name: z.string().trim().min(1).optional(),
})

const spaceUpdateInstructionsInput = z.object({
  spaceRef: z.string().min(1),
  instructions: z.string().trim().max(4000).optional(),
})

const spaceCreateAgentInput = z.object({
  spaceRef: z.string().min(1),
  name: z.string().trim().min(2).max(60),
  instructions: z.string().trim().max(4000).optional(),
  avatarColor: z
    .string()
    .trim()
    .regex(/^#[0-9a-fA-F]{6}$/, 'avatarColor must be a 6-digit hex color')
    .optional(),
})

const spaceBindAgentInput = z.object({
  spaceRef: z.string().min(1),
  agentRef: z.string().min(1),
})

const spaceRequestPersonalDeletionInput = z.object({
  spaceRef: z.string().min(1),
  idempotencyKey: z.string().trim().min(1).max(160),
})

const spaceUpdateThreadPresentationInput = z
  .object({
    spaceRef: z.string().min(1),
    threadId: z.string().min(1),
    title: z.string().trim().max(200).optional(),
    pinned: z.boolean().optional(),
  })
  .refine((value) => value.title !== undefined || value.pinned !== undefined, {
    message: "Provide either 'title' or 'pinned'",
  })

const spaceMarkReadInput = z.object({
  spaceRef: z.string().min(1),
})

const spaceRecordPresenceInput = z.object({
  spaceRef: z.string().min(1),
  status: z.enum(['online', 'typing', 'offline']).optional(),
})

const spaceSetAgentStateInput = z.object({
  spaceRef: z.string().min(1),
  bindingRef: z.string().min(1),
  status: z.enum(['active', 'paused']),
})

const spaceRevokeAgentInput = z.object({
  spaceRef: z.string().min(1),
  bindingRef: z.string().min(1),
})

// A distinct id from spaces.create_personal_space even though both post to
// the same route and handler (domains/spaces.rs's create_personal_space,
// which dispatches on the body's `kind`): the registered personal-space
// schema has no `kind` field, so it could not express a room create, and a
// room needs a non-empty `name` where a personal Space's is optional.
const spaceCreateRoomInput = z.object({
  kind: z.literal('room'),
  name: z.string().trim().min(1),
})

const spaceAddMemberInput = z.object({
  spaceRef: z.string().min(1),
  memberId: z.string().min(1),
})

const spaceRemoveMemberInput = z.object({
  spaceRef: z.string().min(1),
  memberId: z.string().min(1),
})

const inboxFollowConversationInput = z.object({
  conversationId: z.string().min(1),
  following: z.boolean(),
})

const inboxSetCSATPreferenceInput = z.object({
  conversationId: z.string().min(1),
  optedIn: z.boolean(),
})

const inboxAddTagInput = z.object({
  conversationId: z.string().min(1),
  tag: z.string().trim().min(1),
})

const inboxRemoveTagInput = z.object({
  conversationId: z.string().min(1),
  tag: z.string().trim().min(1),
})

const inboxClaimDraftLeaseInput = z.object({
  conversationId: z.string().min(1),
})

const inboxReleaseDraftLeaseInput = z.object({
  conversationId: z.string().min(1),
})

const inboxSaveDraftInput = z.object({
  conversationId: z.string().min(1),
  bodyText: z.string().trim(),
  internal: z.boolean(),
})

const inboxDeleteDraftInput = z.object({
  conversationId: z.string().min(1),
})

const inboxSetStatusInput = z.object({
  conversationId: z.string().min(1),
  status: z.string().trim().min(1),
})

const inboxSetAssignmentInput = z.object({
  conversationId: z.string().min(1),
  assigneeUserId: z.string().trim().min(1),
  assigneeName: z.string().trim().min(1),
})

// social.publish_post's posture, not tickets.update's: a reply, once sent to
// a real customer, cannot be recalled the way a status transition can.
const inboxSendReplyInput = z.object({
  conversationId: z.string().min(1),
  body: z.string().trim().min(1),
  internal: z.boolean(),
  idempotencyKey: z.string().trim().min(1),
})

const inboxSubmitFeedbackInput = z.object({
  bodyText: z.string().trim().min(1),
  fromName: z.string().trim().optional(),
  fromEmail: z.string().trim().optional(),
  pageUrl: z.string().trim().optional(),
})

// Mirrors AiActionFieldEdits (inbox-client.ts) field-for-field rather than a
// raw record, since conversation-core's ReviewAIAction only ever applies
// these named fields.
const aiActionFieldEdits = z.object({
  body_text: z.string().optional(),
  category: z.string().optional(),
  intent: z.string().optional(),
  work_type: z.enum(['customer_case', 'internal_work', 'incident']).optional(),
  status: z.enum(['open', 'waiting_customer', 'waiting_team', 'escalated']).optional(),
  priority: z.string().optional(),
  severity: z.string().optional(),
  team_id: z.string().optional(),
  team_name: z.string().optional(),
  title: z.string().optional(),
  customer_impact: z.string().optional(),
  summary: z.string().optional(),
  root_cause: z.string().optional(),
})

// This IS the human review checkpoint social.decide_approval and
// tickets.classify_conversation's proposals depend on, not an action that
// itself needs a further approval gate ahead of it.
const inboxReviewAiActionInput = z.object({
  aiActionId: z.string().min(1),
  decision: z.enum(['approve', 'reject']),
  comment: z.string().trim().optional(),
  editedFields: aiActionFieldEdits.optional(),
})

const inboxCreateDraftReplyProposalInput = z.object({
  conversationId: z.string().min(1),
  bodyText: z.string().trim().min(1),
  proposalGroupId: z.string().trim().optional(),
})

const inboxCreateInternalNoteProposalInput = z.object({
  conversationId: z.string().min(1),
  bodyText: z.string().trim().min(1),
  proposalGroupId: z.string().trim().optional(),
})

const inboxCreateTicketUpdateProposalInput = z.object({
  conversationId: z.string().min(1),
  ticketId: z.string().min(1),
  confidence: z.number().min(0).max(1),
  reason: z.string().trim().min(1),
  evidenceMessageIds: z.array(z.string().min(1)),
  proposalGroupId: z.string().trim().optional(),
  suggestedFields: aiActionFieldEdits.pick({
    category: true,
    intent: true,
    work_type: true,
    priority: true,
    severity: true,
    status: true,
    team_id: true,
    team_name: true,
  }),
})

const inboxCreateIncidentProposalInput = z.object({
  conversationId: z.string().min(1),
  ticketId: z.string().min(1),
  title: z.string().trim().min(1),
  severity: z.enum(['low', 'medium', 'high', 'critical']),
  customerImpact: z.string().trim().min(1),
  confidence: z.number().min(0).max(1),
  reason: z.string().trim().min(1),
  evidenceMessageIds: z.array(z.string().min(1)),
  proposalGroupId: z.string().trim().optional(),
})

const inboxCreateProblemProposalInput = z.object({
  conversationId: z.string().min(1),
  title: z.string().trim().min(1),
  summary: z.string().trim().min(1),
  rootCause: z.string().trim().optional(),
  confidence: z.number().min(0).max(1),
  reason: z.string().trim().min(1),
  evidenceMessageIds: z.array(z.string().min(1)),
  proposalGroupId: z.string().trim().optional(),
})

const ticketCSATOutcomeInput = z.object({
  ticketId: z.string().min(1),
  score: z.number().int().min(1).max(5),
})

const workflowInput = z.object({
  workflowId: z.string().min(1),
  enabled: z.boolean(),
})

const socialPlatform = z.enum(['linkedin', 'x', 'instagram', 'tiktok'])

const socialDraftInput = z.object({
  title: z.string().trim().min(1).max(140),
  body: z.string().trim().min(1).max(2800),
  platforms: z.array(socialPlatform).min(1),
  sourceKind: z.enum(['manual', 'inbox', 'knowledge', 'campaign']).default('manual'),
  sourceId: z.string().trim().min(1).optional(),
})

const socialScheduleInput = z.object({
  postId: z.string().trim().min(1),
  scheduledAt: z.string().trim().datetime(),
  platforms: z.array(socialPlatform).min(1),
})

const socialPublishInput = z.object({
  postId: z.string().trim().min(1),
  platforms: z.array(socialPlatform).min(1),
  approvalId: z.string().trim().min(1),
})

// The client's own SocialProviderKey (facebook, snapchat included) is wider
// than socialPlatform above; social.create_campaign's gateway dispatcher
// forwards platforms as free-form strings with no enum check of its own, so
// this schema matches the real client type rather than perpetuating
// socialPlatform's narrower, pre-existing set into a new action.
const socialProviderKey = z.enum(['linkedin', 'x', 'instagram', 'facebook', 'tiktok', 'snapchat'])

const socialCreateCampaignInput = z.object({
  name: z.string().trim().min(1).max(140),
  brief: z.string().trim().optional(),
  goal: z.string().trim().optional(),
  status: z.enum(['draft', 'active', 'completed', 'archived']).optional(),
  platforms: z.array(socialProviderKey).optional(),
  startsAt: z.string().datetime().nullable().optional(),
  endsAt: z.string().datetime().nullable().optional(),
})

const socialCreateDraftFromInboxInput = z.object({
  ticketId: z.string().min(1),
  ticketTitle: z.string().trim().min(1),
  supportTicketId: z.string().trim().optional(),
  conversationId: z.string().trim().optional(),
  customerName: z.string().trim().optional(),
  channel: z.string().trim().optional(),
  excerpt: z.string().trim().optional(),
})

// decide_approval IS the human checkpoint social.publish_post's approvalId
// depends on, not an action that itself needs a further approval gate ahead
// of it — requiresApproval stays false for that reason, not by oversight.
const socialDecideApprovalInput = z.object({
  approvalId: z.string().min(1),
  decision: z.enum(['approved', 'rejected']),
  reason: z.string().trim().optional(),
})

const scrapeUrlInput = z.object({
  url: z.string().min(1),
  // WS13.7: screenshot and PDF captures. Actions are not exposed in v1.
  capture: z
    .object({
      screenshot: z
        .union([
          z.boolean(),
          z.object({ fullPage: z.boolean().optional(), quality: z.number().int().min(1).max(100).optional() }),
        ])
        .optional(),
      pdf: z.boolean().optional(),
    })
    .optional(),
})

const crawlSiteInput = z.object({
  url: z.string().min(1),
  maxPages: z.number().int().positive().optional(),
  // WS14.S6: the Model-visible scope subset. No external-links or robots key exists here
  // (a test in the shared actions folder asserts it); the gateway drops them too.
  maxDepth: z.number().int().positive().max(10).optional(),
  includePaths: z.array(z.string().min(1).max(512)).max(100).optional(),
  excludePaths: z.array(z.string().min(1).max(512)).max(100).optional(),
  allowSubdomains: z.boolean().optional(),
})

const importSourceInput = z.object({
  url: z.string().min(1),
  name: z.string().optional(),
  kind: z.string().optional(),
})

const uploadFilesInput = z.object({
  fileNames: z.array(z.string().min(1)).min(1),
})

const connectSourceInput = z.object({
  sourceType: z.enum(['notion', 'crm', 'erp', 'cms', 'pim', 'hubspot', 'salesforce', 'odoo']),
  connection: z.record(z.string(), z.unknown()).optional(),
  options: z.record(z.string(), z.unknown()).optional(),
  /** A browser selection only; the BFF resolves all import authority. */
  spaceRef: z.string().trim().min(1).max(256).optional(),
})

const knowledgeRegisterSharePointSourceInput = z.object({
  kind: z.enum(['drive', 'site_pages']).optional(),
  siteId: z.string().trim().min(1),
  siteWebUrl: z.string().trim().optional(),
  driveId: z.string().trim().optional(),
  driveName: z.string().trim().optional(),
  driveType: z.string().trim().optional(),
  tenantId: z.string().trim().optional(),
  folderId: z.string().trim().optional(),
  folderPath: z.string().trim().optional(),
})

const knowledgeCreateDocumentInput = z.object({
  content: z.string().trim().min(1),
  sourceUrl: z.string().url().optional(),
  title: z.string().trim().optional(),
  type: z.string().trim().optional(),
})

const knowledgeExtractProductsInput = z.object({
  url: z.string().url(),
  prompt: z.string().trim().optional(),
})

const knowledgeProductInput = z.object({
  name: z.string(),
  price: z.string().optional(),
  currency: z.string().optional(),
  image: z.string().optional(),
  url: z.string().optional(),
  specs: z.array(z.string()).optional(),
  description: z.string().optional(),
})

const knowledgeSummarizeProductsInput = z.object({
  products: z.array(knowledgeProductInput).min(1),
  prompt: z.string().trim().optional(),
})

const operatingMapGenerateInput = z.object({
  generatedFrom: z.record(z.string(), z.unknown()).optional(),
})

const operatingMapReviewInput = z.object({
  proposalId: z.string().trim().min(1),
  decision: z.enum(['accept', 'reject']),
})

const operatingMapBlueprintInput = z.object({
  versionId: z.string().trim().min(1),
  blueprintId: z.string().trim().min(1),
  role: z.enum(['service', 'sales', 'ecommerce', 'chatbot', 'workflow']),
  sourceWorkflowId: z.string().trim().optional(),
  name: z.string().trim().min(1),
  payload: z.record(z.string(), z.unknown()).optional(),
})

const brregLookupInput = z.object({
  q: z.string().trim().min(1).max(160),
  size: z.number().int().min(1).max(20).default(8),
})

const shippingAddressInput = z.object({
  name: z.string().trim().min(1),
  postal_code: z.string().trim().min(1),
  city: z.string().trim().min(1),
  country: z.string().trim().length(2).describe('ISO 3166-1 alpha-2, e.g. "NO"'),
})

const shippingQuoteInput = z.object({
  from: shippingAddressInput,
  to: shippingAddressInput,
  package: z.object({
    weight_kg: z.number().positive(),
    length_cm: z.number().positive(),
    width_cm: z.number().positive(),
    height_cm: z.number().positive(),
  }),
  segment: z.enum(['b2b', 'b2c']).default('b2b'),
})

const runOutput = z.object({
  runId: z.string(),
  status: z.enum(['queued', 'planning', 'waiting_approval', 'executing', 'completed', 'failed']),
})

const notificationMarkReadInput = z.object({
  id: z.string().min(1),
})

const notificationMarkAllReadInput = z.object({})

const notificationDeleteInput = z.object({
  id: z.string().min(1),
})

const notificationUpdatePreferenceInput = z.object({
  eventType: z.string().trim().min(1),
  channel: z.string().trim().min(1),
  enabled: z.boolean(),
})

const navbarSaveThemeInput = z.object({
  theme: z.enum(['light', 'dark', 'system']),
  colorScheme: z.string().trim().nullable().optional(),
})

const navbarMarkNotificationReadInput = z.object({
  notificationId: z.string().min(1),
})

const navbarCreateCalendarEventInput = z.object({
  start: z.string().trim().min(1),
  end: z.string().trim().min(1),
  title: z.string().trim().min(1),
  type: z.string().trim().min(1),
})

const navbarCreateCalendarNoteInput = z.object({
  date: z.string().trim().min(1),
  text: z.string().trim().min(1),
})

const navbarSubmitSupportRequestInput = z.object({
  context: z.string().trim().min(1),
  message: z.string().trim().min(1),
  subject: z.string().trim().min(1),
})

const inboxSetConversationPinnedInput = z.object({
  conversationId: z.string().min(1),
  enabled: z.boolean(),
})

const inboxSetConversationReadInput = z.object({
  conversationId: z.string().min(1),
  enabled: z.boolean(),
})

const ownershipShareDocumentInput = z.object({
  docId: z.string().min(1),
  subjectId: z.string().min(1),
})

const ownershipRevokeDocumentShareInput = z.object({
  docId: z.string().min(1),
  subjectId: z.string().min(1),
})

// Destructive and non-recoverable per memory-client.ts's own doc comment:
// session-core removes the durable record immediately and best-effort purges
// the semantic backend copy.
const memoryDeleteInput = z.object({
  memoryId: z.string().min(1),
})

// The caller's own memory state; whose memory it is comes from the session.
const memorySetStateInput = z.object({
  state: z.enum(['on', 'paused', 'off']),
})

// Erases every memory about the caller. The literal `true` is the user's
// confirmation: an absent or false value never reaches the owner.
const memoryResetInput = z.object({
  confirm: z.literal(true),
})

// `threadId` only proves the caller owns a thread (IndexMemory resolves the
// user from it); the saved fact is not scoped to it.
const memoryRememberInput = z.object({
  threadId: z.string().min(1),
  content: z.string().trim().min(1),
})

// A learned-skill proposal, by the skill's id. Only its owner may decide it, and
// session-core checks that; the caller's own identity never comes from here.
const learningSkillDecisionInput = z.object({
  skillId: z.string().min(1),
})

// One ledger entry of the caller's own, by its id; a learned memory is not undone here.
const learningUndoInput = z.object({
  entryId: z.string().min(1),
})

const settingsUpdateMeInput = z.object({
  name: z.string().trim().optional(),
  displayName: z.string().trim().optional(),
  avatar: z.string().trim().optional(),
  firstName: z.string().trim().optional(),
  lastName: z.string().trim().optional(),
  phoneNumber: z.string().trim().optional(),
  officeLocation: z.string().trim().optional(),
  timezone: z.string().trim().optional(),
  position: z.string().trim().optional(),
  department: z.string().trim().optional(),
  status: z.string().trim().optional(),
})

const settingsUpdatePreferencesInput = z.object({
  theme: z.string().trim().optional(),
  language: z.string().trim().optional(),
  timezone: z.string().trim().optional(),
  notifications: z.record(z.string(), z.boolean()).optional(),
  crawlIngestMode: z.enum(['auto', 'never', 'prompt']).optional(),
})

const settingsUpdateSettingInput = z.object({
  key: z.string().trim().min(1),
  value: z.unknown(),
})

// The response carries a raw, one-time API key secret auth-core never
// returns again — real credential material, not a routine settings change.
const settingsCreateApiKeyInput = z.object({
  name: z.string().trim().min(1),
  expiresAt: z.string().datetime().optional(),
})

const settingsDeleteApiKeyInput = z.object({
  id: z.string().min(1),
})

// registerMcpServer/connectMcpServer are deliberately unregistered: both gate
// on an org-admin-only check plus the same delegated model-gateway
// capability-token exchange cron-client.ts's operations use — the same depth
// this registry already declined to rush for cron.
const mcpDeleteServerInput = z.object({
  serverId: z.string().min(1),
})

const mcpShareServerInput = z.object({
  serverId: z.string().min(1),
  userIds: z.array(z.string().min(1)),
})

// A tool is approved by naming the digest of the definition the person reviewed,
// never "whatever is observed now": the owner pins only a digest that is still the
// observed one, so a definition that changed after the person looked is not pinned
// unseen. The org is never an input; it comes from the verified session.
const mcpApproveToolInput = z
  .object({
    serverId: z.string().min(1),
    tool: z.string().min(1),
    approvedDigest: z.string().regex(/^[0-9a-f]{64}$/),
  })
  .strict()

// A per-tool policy. `allow` is the only decision that relaxes an approval, so it
// alone names the digest of the tool definition the person reviewed (the owner
// pins it and reads an allow for a changed definition as an ask); `ask` and
// `never` only tighten and take none. `scope` defaults to the caller's own; an
// org-wide policy needs an org admin and is checked by the owner, not here.
const toolsSetPolicyInput = z
  .object({
    capabilityId: z.string().min(1),
    scope: z.enum(['user', 'org']).optional(),
    decision: z.enum(['allow', 'ask', 'never']),
    pinnedDigest: z.string().regex(/^[0-9a-f]{64}$/).optional(),
    reason: z.string().max(500).optional(),
  })
  .strict()
  .refine((input) => (input.decision === 'allow') === (input.pinnedDigest !== undefined), {
    message: 'pinnedDigest is required for an allow and refused for ask and never',
    path: ['pinnedDigest'],
  })

const monitoringCheckNowInput = z.object({
  url: z.string().url(),
})

// The client always sends confirm: true itself and never lets the caller
// override it (see eraseMyAccount); no input fields are meaningful here.
const privacyEraseMyAccountInput = z.object({})

const leadCompanyInput = z.object({
  organisasjonsnummer: z.string(),
  navn: z.string(),
  organisasjonsform: z.string().optional(),
  naeringskode: z.string().optional(),
  naering_beskrivelse: z.string().optional(),
  kommunenummer: z.string().optional(),
  poststed: z.string().optional(),
  antall_ansatte: z.number().optional(),
  registreringsdato: z.string().optional(),
  hjemmeside: z.string().optional(),
  konkurs: z.boolean().optional(),
  under_avvikling: z.boolean().optional(),
})

const leadsCreateListInput = z.object({
  name: z.string().trim().min(1),
  companies: z.array(leadCompanyInput),
})

const leadsDeleteListInput = z.object({
  id: z.string().min(1),
})

const finetuneCreateJobInput = z.object({
  agent_id: z.string().min(1),
  base_model: z.string().min(1),
  azure_file_id: z.string().min(1),
  hyperparameters: z.record(z.string(), z.unknown()).optional(),
  deployment_tier: z.enum(['developer', 'production']).optional(),
})

const finetuneCancelJobInput = z.object({
  jobId: z.string().min(1),
})

const finetuneDeployJobInput = z.object({
  jobId: z.string().min(1),
  tier: z.enum(['developer', 'production']).default('production'),
})

const studioBlockInput = z.object({
  id: z.string().min(1),
  kind: z.enum(['profile', 'image', 'text', 'brand', 'video', 'link', 'social']),
  title: z.string(),
  body: z.string().nullable().optional(),
  imageUrl: z.string().nullable().optional(),
  x: z.number(),
  y: z.number(),
  width: z.number(),
  height: z.number(),
})

const studioCreateProjectInput = z.object({
  title: z.string().trim().optional(),
  blocks: z.array(studioBlockInput).optional(),
  selectedBlockId: z.string().nullable().optional(),
})

const studioSaveProjectInput = z.object({
  projectId: z.string().min(1),
  title: z.string().trim().optional(),
  blocks: z.array(studioBlockInput).optional(),
  selectedBlockId: z.string().nullable().optional(),
})

// The owner's real validation accepts either 'cron' or 'scheduleAt' (see
// create_schedule in schedules.rs); the client's own ScheduleCreateRequest
// type declares cron as required, which is narrower than what the owner
// actually does, so this schema follows the owner rather than the client type.
const ingestionsCreateRunInput = z
  .object({
    kind: z.string().min(1),
    urls: z.array(z.string().url()).optional(),
    url: z.string().url().optional(),
    prompt: z.string().trim().optional(),
  })
  .refine((value) => (value.kind === 'batch' ? (value.urls?.length ?? 0) > 0 : !!value.url), {
    message: "kind 'batch' requires a non-empty 'urls' array; any other kind requires 'url'",
  })

const ingestionsCreateScheduleInput = z
  .object({
    name: z.string().trim().min(1),
    kind: z.string().min(1),
    targetUrl: z.string().url(),
    cron: z.string().trim().optional(),
    scheduleAt: z.string().datetime().optional(),
  })
  .refine((value) => !!value.cron || !!value.scheduleAt, {
    message: "Provide either 'cron' or 'scheduleAt'",
  })

const ingestionsRunScheduleActionInput = z
  .object({
    action: z.enum([
      'pause_schedule',
      'unpause_schedule',
      'trigger_schedule',
      'backfill_schedule',
      'delete_schedule',
    ]),
    scheduleId: z.string().min(1),
    startAt: z.string().datetime().optional(),
    endAt: z.string().datetime().optional(),
  })
  .refine((value) => value.action !== 'backfill_schedule' || (!!value.startAt && !!value.endAt), {
    message: "backfill_schedule requires 'startAt' and 'endAt'",
  })

const ingestionsCreateSourceInput = z.object({
  name: z.string().trim().min(1),
  url: z.string().url(),
  kind: z.string().min(1),
  monitor: z.boolean().optional(),
  preset: z.string().optional(),
  config: z.record(z.string(), z.unknown()).optional(),
})

const ingestionsDeleteSourceInput = z.object({
  id: z.string().min(1),
})

const integrationsStartConnectSessionInput = z.object({
  provider: z.string().trim().min(1),
  body: z.record(z.string(), z.unknown()).optional(),
})

const integrationsDisconnectInput = z.object({
  id: z.string().min(1),
})

const integrationsDisconnectChatGptSubscriptionInput = z.object({
  connectionId: z.string().min(1),
})

const integrationsTriggerSyncInput = z.object({
  id: z.string().min(1),
})

const integrationsTriggerInboxSyncInput = z.object({
  id: z.string().min(1),
  channel: z.enum(['email', 'teams', 'slack']),
})

const integrationsExtendInboxHistoryInput = z.object({
  id: z.string().min(1),
})

// Connected-app reads for the composer's picker (WS23 S6). The operation is a closed enum, so a
// client names a listing or a content read and never a write; integration-corev2 still checks that
// the caller owns the connection. Exported so the browser client validates its calls against them.
export const integrationsBrowseConnectionInput = z
  .object({
    connectionId: z.string().min(1),
    operation: z.enum(['mail.messages', 'gmail.messages', 'drive.files', 'calendar.events']),
    params: z.record(z.string(), z.unknown()).optional(),
  })
  .strict()

export const integrationsFetchConnectionContentInput = z
  .object({
    connectionId: z.string().min(1),
    operation: z.enum([
      'drive.file.content',
      'microsoft.drive.file.content',
      'google.drive.file.content',
      'mail.messages.get',
      'microsoft.mail.messages.get',
      'google.gmail.messages.get',
      'gmail.messages.get',
    ]),
    params: z.record(z.string(), z.unknown()),
  })
  .strict()

const integrationsConnectorActionOutput = z.object({
  providerKey: z.string(),
  operation: z.string(),
  result: z.unknown(),
})

// The connected accounts one chat turn may read (WS23 S6): the chip selection the composer sends as
// `connector_scope`. The bounds are the gateway's own (model-gateway `connector_scope`): at most 8
// distinct ids of 1-128 characters from [A-Za-z0-9_-]. An empty list clears the choice.
const chatSetConnectorScopeInput = z
  .object({
    connectionIds: z
      .array(z.string().regex(/^[A-Za-z0-9_-]{1,128}$/))
      .max(8)
      .refine((ids) => new Set(ids).size === ids.length, { message: 'connection ids must be distinct' }),
  })
  .strict()

// The agent mode one chat turn runs under (WS25-S6): the four ids of `api/contracts/agent-modes.json`,
// read by the gateway as `agent_mode`. A closed list, so an id outside it is refused.
const chatSetAgentModeInput = z
  .object({
    mode: z.enum(['lockdown', 'read_only', 'ask_first', 'auto']),
  })
  .strict()

// The org-admin schemas below all describe operations whose owner requires
// require_org_admin (or, for the two self-service deletion checkpoints,
// require_active_org) — enforced server-side, not by this schema. Getting the
// risk/approval posture wrong here has real consequences, so each was traced
// against its real handler in orgs/*.rs rather than assumed from the name.
const orgSoftDeleteInput = z.object({
  orgId: z.string().min(1),
  confirm: z.literal(true),
  orgName: z.string().trim().min(1),
})

const orgRestoreInput = z.object({
  orgId: z.string().min(1),
})

const orgMarkExportedInput = z.object({
  orgId: z.string().min(1),
})

const orgAcknowledgeDeletionInput = z.object({
  orgId: z.string().min(1),
})

const orgSetQuotaInput = z.object({
  orgId: z.string().min(1),
  key: z.string().min(1),
  limit: z.number().int().nonnegative(),
  reset_period: z.enum(['daily', 'monthly', 'none']).optional(),
})

// One layer of an org model policy (WS10-S5). The limits mirror org-core's; org-core stays the
// owner of every semantic rule (known efforts and tiers, defaults against the policy they sit in).
// Strict, so a misspelled field is refused instead of being dropped and read as set.
const modelPolicyLayer = z.object({
  allow_mode: z.enum(['all', 'listed']),
  allow: z.array(z.string().max(128)).max(500).optional(),
  deny: z.array(z.string().max(128)).max(500).optional(),
  max_effort: z.string().optional(),
  min_privacy_tier: z.string().optional(),
  defaults: z.record(
    z.string(),
    z.object({ model: z.string().optional(), effort: z.string().optional() }).strict(),
  ).optional(),
}).strict()

const orgSetModelEntitlementInput = z.object({
  orgId: z.string().min(1),
  expected_revision: z.number().int().nonnegative(),
  org_default: modelPolicyLayer.nullable().optional(),
  roles: z.record(z.string().min(1), modelPolicyLayer).optional(),
}).strict()

const orgUpdateInstructionsInput = z.object({
  orgId: z.string().min(1),
  instructions: z.string().trim().optional(),
})

const orgUpdateZdrInput = z.object({
  orgId: z.string().min(1),
  zeroDataRetention: z.boolean(),
})

const orgUpdateSupportAiModeInput = z.object({
  orgId: z.string().min(1),
  supportAiMode: z.enum(['off', 'assist', 'review']),
})

const orgUpdateAgentLearningPolicyInput = z.object({
  orgId: z.string().min(1),
  memory: z.enum(['allow', 'disabled']).optional(),
  learningReview: z.enum(['disabled', 'proposals']).optional(),
})

// The organization agent policy and its hook rules (WS25-S10). Inputs name no organization: the session
// supplies it. The settings keys are the closed set of api/contracts/org-agent-policy-schema.json; reserved
// keys are not writable from here. Inputs are camelCase like the rest of the registry; the REST bodies and
// the outputs are capability-core's wire form.
const orgAgentPolicyAllowListInput = z
  .object({
    mode: z.enum(['none', 'approved_only', 'any']),
    ids: z.array(z.string().min(1).max(200)).max(64),
  })
  .strict()
  .refine((input) => input.ids.length === 0 || input.mode === 'approved_only', {
    message: 'ids are only allowed with mode approved_only',
    path: ['ids'],
  })

const orgSetAgentPolicyInput = z
  .object({
    revision: z.number().int().min(0),
    settings: z
      .object({
        mode_cap: z.enum(['lockdown', 'read_only', 'ask_first', 'auto']).optional(),
        rung_cap: z.enum(['read_only', 'workspace_write', 'danger_full_access']).optional(),
        lockdown: z.boolean().optional(),
        'ext.mcp_servers': orgAgentPolicyAllowListInput.optional(),
        mcp_tool_approval: z.enum(['approved_only', 'trust_annotations']).optional(),
      })
      .strict(),
  })
  .strict()

const orgPreviewEffectivePolicyInput = z.object({}).strict()

// A tool matcher is empty or "*" for any tool, a tool name, or a name ending in one "*" (hook/mod.rs).
const orgHookRuleFields = z.object({
  event: z.enum(['pre_tool', 'post_tool']),
  toolMatcher: z
    .string()
    .max(200)
    .regex(/^(\*|[A-Za-z0-9][A-Za-z0-9_.:-]{0,199}\*?)?$/)
    .optional(),
  decision: z.enum(['deny', 'ask']),
  reason: z.string().trim().min(1).max(500),
  priority: z.number().int().min(-2147483648).max(2147483647).optional(),
  enabled: z.boolean().optional(),
})

// A post_tool rule may only deny: the tool has already acted by the time its result is seen (WS25-D7).
const postToolRuleMustDeny = (input: { event: string; decision: string }) =>
  input.event !== 'post_tool' || input.decision === 'deny'
const postToolRuleMustDenyIssue = { message: 'a post_tool rule may only deny', path: ['decision'] }

const orgCreateHookInput = orgHookRuleFields
  .strict()
  .refine(postToolRuleMustDeny, postToolRuleMustDenyIssue)

const orgUpdateHookInput = orgHookRuleFields
  .extend({ hookId: z.string().min(1).max(200) })
  .strict()
  .refine(postToolRuleMustDeny, postToolRuleMustDenyIssue)

const orgDeleteHookInput = z.object({ hookId: z.string().min(1).max(200) }).strict()

const orgAgentPolicyOutput = z
  .object({
    org_id: z.string(),
    revision: z.number().int().min(0),
    digest: z.string(),
    settings: z.record(z.string(), z.unknown()),
  })
  .passthrough()

const orgEffectivePolicyOutput = z
  .object({
    org_id: z.string(),
    revision: z.number().int().min(0),
    digest: z.string(),
    settings: z.array(
      z
        .object({
          key: z.string(),
          kind: z.enum(['enum', 'bool', 'allow_list']),
          status: z.enum(['enforced', 'reserved']),
          value: z.unknown(),
          source: z.enum(['platform_default', 'org_policy']),
          fail_closed_value: z.unknown(),
        })
        .passthrough(),
    ),
  })
  .passthrough()

const orgHookRuleOutput = z
  .object({
    id: z.string(),
    event: z.enum(['pre_tool', 'post_tool']),
    tool_matcher: z.string(),
    decision: z.enum(['deny', 'ask']),
    reason: z.string(),
    priority: z.number().int(),
    enabled: z.boolean(),
    revision: z.number().int().min(0),
  })
  .passthrough()

const orgHookRevokeOutput = z
  .object({
    id: z.string(),
    revoked: z.literal(true),
    revision: z.number().int().min(0),
  })
  .passthrough()

const membershipInviteMemberInput = z.object({
  orgId: z.string().min(1),
  email: z.string().trim().email().max(320),
  role: z.enum(['member', 'admin']),
})

const membershipRemoveMemberInput = z.object({
  orgId: z.string().min(1),
  userId: z.string().min(1),
})

const membershipUpdateMemberRoleInput = z.object({
  orgId: z.string().min(1),
  userId: z.string().min(1),
  role: z.enum(['member', 'admin']),
})

const organizationSwitchActiveInput = z.object({
  organizationId: z.string().min(1),
})

// Grants a running model invocation an autonomy rung up to
// danger_full_access. The owner (model-gateway) checks run ownership
// server-side; nothing here or in the gateway re-derives that check.
//
// `planId` names the structured plan the person approved and `editedSteps` are
// the edits they made to its steps; both are optional, so the rung-only grant
// is unchanged. A step with no `stepId` is a new one, and `removed` marks one
// the person dropped (it is kept, as skipped, rather than deleted).
const chatApprovePlanInput = z.object({
  runId: z.string().min(1),
  grantedRung: z.enum(['read_only', 'workspace_write', 'danger_full_access']),
  justification: z.string().trim().min(1),
  planId: z.string().min(1).optional(),
  editedSteps: z
    .array(
      z.object({
        stepId: z.string().optional(),
        title: z.string().min(1),
        operation: z.string().optional(),
        removed: z.boolean().optional(),
      }),
    )
    .optional(),
})

// Starts the turn that runs an approved plan. Strict on purpose: the rung is
// held by the server (the grant recorded when the person approved the plan) and
// is never a client input, so a supplied `grantedRung` must fail to parse. The
// controller spends the grant on connect: one approval is one execute turn.
const chatExecuteApprovedPlanInput = z
  .object({
    threadId: z.string().min(1),
    planId: z.string().min(1).optional(),
  })
  .strict()

const chatCancelInvocationInput = z.object({
  requestId: z.string().min(1),
})

// `clientMessageId` is the send's receipt, minted by the caller: the owner stores the message under it
// before delivering it and names it in the delivery, so a repeat of the same send (a retry after a lost
// reply, or the message sent again as a new turn because the run never took it) is one transcript row.
// The shape is the one the owner stores (it checks again). Omitted for a zero-retention thread.
const chatQueueInvocationInput = z.object({
  requestId: z.string().min(1),
  content: z.string().trim().min(1),
  threadId: z.string().optional(),
  spaceRef: z.string().optional(),
  clientMessageId: z
    .string()
    .regex(/^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/)
    .optional(),
})

// The granted approval named here is the gate for the continuation, so the entry
// is not approval-gated a second time: the claim token proves the caller holds
// the pause it resumes. Not model-executable (see model-eligibility.ts).
//
// approvalId is absent for the park form (WS03 S8): the continue of a run the executor
// parked, which has no approval. A present approvalId is never blank; the empty approval
// id on the wire is the client's to write for the park form, never a caller's input.
const chatRunContinueInput = z.object({
  runId: z.string().min(1),
  approvalId: z.string().min(1).optional(),
  claimToken: z.string().min(1),
})

// The person's answer to a question a run put to them (build plan WS14 S3), or a skip. A
// question is its own store with its own owner-only answer, never an approval: an answer is
// information and carries no authority, so the entry is low risk and shares nothing with the
// approval decisions. Strict, so a client cannot name an org, a user or the message that
// carries the answer: those are the server's. The bounds mirror the shared contract
// (`mp_contracts::user_question`): 4 items, 6 options, 2000 characters.
const chatAnswerQuestionAnswer = z
  .object({
    itemId: z.string().min(1).max(64),
    optionIds: z.array(z.string().min(1).max(64)).max(6).optional(),
    text: z.string().max(2000).optional(),
  })
  .strict()

const chatAnswerQuestionInput = z
  .object({
    questionId: z.string().min(1),
    answers: z.array(chatAnswerQuestionAnswer).max(4).optional(),
    skip: z.boolean().optional(),
  })
  .strict()
  .refine((input) => (input.skip === true ? (input.answers ?? []).length === 0 : (input.answers ?? []).length > 0), {
    message: 'A skip carries no answers, and an answer carries at least one.',
  })

// The decision on a pending approval (build plan WS13 S2). The decider is the verified credential's,
// never an input, so a field naming one, a decision source or a grant is refused rather than ignored.
// `scope` is how far the decision reaches; only `once` exists until standing grants do.
const approvalsDecideInput = z
  .object({
    approvalId: z.string().min(1),
    decision: z.enum(['approve', 'reject']),
    reason: z.string().max(500).optional(),
    scope: z.literal('once').optional(),
  })
  .strict()

const chatClearThreadsInput = z.object({})

const chatDeleteThreadInput = z.object({
  threadId: z.string().min(1),
})

const chatMarkThreadSeenInput = z.object({
  threadId: z.string().trim().min(1),
})

const chatSaveThreadSnapshotInput = z.object({
  threadId: z.string().min(1),
  title: z.string().trim().optional(),
  pinned: z.boolean().optional(),
  preview: z.string().trim().optional(),
})

const chatUploadDocumentInput = z.object({
  title: z.string().trim().min(1),
  content: z.string().trim().min(1),
})

const audioFormat = z.enum(['webm', 'ogg', 'wav', 'mp3'])

const audioTranscribeInput = z.object({
  audioBase64: z.string().min(1),
  format: audioFormat,
  language: z.string().trim().min(1),
})

// Dictation also takes Safari's AAC-in-mp4 recordings (labelled m4a), and a
// Zero Data Retention flag the gateway forwards (and raises to the org's
// standing posture).
const audioDictateFormat = z.enum(['webm', 'ogg', 'wav', 'mp3', 'm4a'])

const audioDictateInput = z.object({
  audioBase64: z.string().min(1),
  format: audioDictateFormat,
  language: z.string().trim().min(1),
  context: z.string().trim().optional(),
  zdr: z.boolean().optional(),
})

/**
 * Upper bound on the decision `reason`. On a reject it can carry redirect text
 * (the person telling the agent what to do instead). The owner does not read
 * the text yet on every path; see the description on `orchestration.decide_approval`.
 */
export const ORCHESTRATION_APPROVAL_REASON_MAX = 1000

export const orchestrationDecideApprovalInput = z.object({
  approvalId: z.string().min(1),
  decision: z.enum(['approve', 'reject']),
  reason: z.string().trim().max(ORCHESTRATION_APPROVAL_REASON_MAX).optional(),
})

const orchestrationResumeRunInput = z.object({
  runId: z.string().min(1),
})

const orchestrationCancelRunInput = z.object({
  runId: z.string().min(1),
})

// requireApproval is deliberately absent: the owner (browser.rs's
// StartAiRunBody) documents it as accepted for API back-compat only and never
// read — it would arm a confirmed-broken legacy gate in execution-core.
// Offering it here would describe a safety control that does not exist.
const browserRunStartInput = z.object({
  sessionId: z.string().min(1),
  goal: z.string().trim().min(1),
  allowedDomains: z.array(z.string()).optional(),
  maxSteps: z.number().int().positive().optional(),
  maxRuntimeSeconds: z.number().int().positive().optional(),
  stopCriteria: z.string().trim().optional(),
  maxCostUsd: z.number().nonnegative().optional(),
})

const browserRunControlInput = z.object({
  runId: z.string().min(1),
  action: z.enum(['pause', 'resume', 'stop']),
})

const browserOmniboxSearchInput = z.object({
  query: z.string().trim().min(1).max(500),
  limit: z.number().int().min(1).max(10).optional(),
})

const browserOmniboxSuggestInput = z.object({
  query: z.string().trim().min(1).max(200),
})

const browserEvidenceCompileProcedureInput = z.object({
  sessionId: z.string().trim().min(1).max(128),
  procedureId: z.string().trim().min(1).max(64).optional(),
})

const browserProfileScopeEnum = z.enum(['user_private', 'org_shared', 'run_scoped'])

const browserCreateSessionInput = z.object({
  url: z.string().trim().min(1),
  profileId: z.string().trim().min(1).optional(),
  persistentProfile: z.boolean().optional(),
  viewport: z.object({
    width: z.number().int().positive(),
    height: z.number().int().positive(),
  }).optional(),
  zdr: z.boolean().optional(),
  scope: z.enum(['ephemeral', 'user_private', 'org_shared', 'run_scoped']).optional(),
})

const browserCloseSessionInput = z.object({
  sessionId: z.string().min(1),
})

const browserKeepAliveSessionInput = z.object({
  sessionId: z.string().min(1),
})

// `actor` is who is asking, as on `browser_action.run`: `agent` marks a tab call the
// model proposed, which the owner refuses while the person holds the browser. Absent
// is the person; it can only tighten.
const browserCreateTabInput = z.object({
  sessionId: z.string().min(1),
  url: z.string().trim().min(1).optional(),
  actor: z.enum(['human', 'agent']),
})

const browserSelectTabInput = z.object({
  sessionId: z.string().min(1),
  tabId: z.string().min(1),
  actor: z.enum(['human', 'agent']),
})

const browserCloseTabInput = z.object({
  sessionId: z.string().min(1),
  tabId: z.string().min(1),
  actor: z.enum(['human', 'agent']),
})

// `action` is passed through as a raw object: `sanitize_action` in browser.rs
// is the real enforcement point (blocks private navigation, denies script
// eval, normalizes URLs, rejects unbounded coordinate takeover), so modeling
// the full BrowserAction union here would only be a second, driftable copy
// of that validation.
const browserRunActionInput = z.object({
  sessionId: z.string().min(1),
  action: z.record(z.string(), z.unknown()),
  actor: z.enum(['human', 'agent']).optional(),
})

const browserSetControlModeInput = z.object({
  sessionId: z.string().min(1),
  mode: z.enum(['agent_control', 'human_takeover']),
  /** WS9.I: the control epoch the caller saw; a stale one is refused with a typed 409. */
  expectedEpoch: z.number().int().nonnegative().optional(),
})

const browserSuggestActionInput = z.object({
  sessionId: z.string().min(1),
  goal: z.string().trim().optional(),
  includeScreenshot: z.boolean().optional(),
})

const browserCreateProfileInput = z.object({
  name: z.string().trim().min(1).optional(),
  scope: browserProfileScopeEnum,
})

const browserRenameProfileInput = z.object({
  profileId: z.string().min(1),
  name: z.string().trim().min(1).optional(),
  scope: browserProfileScopeEnum.optional(),
})

const browserDeleteProfileInput = z.object({
  profileId: z.string().min(1),
})

const browserProbeProfileRestoreInput = z.object({
  profileId: z.string().min(1),
  url: z.string().trim().min(1),
})

// `runId` is the AI run on the session, when there is one. Nothing about the page
// travels: the owner needs only which session to close and which run to stop.
const browserBlockedTryOtherSourceInput = z.object({
  sessionId: z.string().min(1),
  runId: z.string().min(1).optional(),
})

const routerPolicyModeTableInput = z.object({
  simple: z.string().min(1),
  moderate: z.string().min(1),
  complex: z.string().min(1),
})

// The owner enforces no admin gate on this write today — only org-scoping.
// See dispatch_router_policy_update's own comment: the high risk/
// requires-approval posture here compensates for that at the registry level,
// since this is a full-document PUT of the org's entire model-routing table.
const routerPolicyUpdateInput = z.object({
  enabled: z.boolean(),
  budget_cap_usd: z.number().nonnegative(),
  constrained_fraction: z.number().min(0).max(1),
  cheap_fallback: z.string().min(1),
  complexity: z.object({
    large_total_chars: z.number(),
    large_total_chars_score: z.number(),
    medium_total_chars: z.number(),
    medium_total_chars_score: z.number(),
    long_user_turn_chars: z.number(),
    long_user_turn_score: z.number(),
    code_fence_score: z.number(),
    keyword_score: z.number(),
    tool_use_score: z.number(),
    deep_conversation_turns: z.number(),
    deep_conversation_score: z.number(),
    moderate_threshold: z.number(),
    complex_threshold: z.number(),
    keywords: z.array(z.string()),
  }),
  table: z.object({
    budget: routerPolicyModeTableInput,
    balance: routerPolicyModeTableInput,
    genius: routerPolicyModeTableInput,
  }),
})

const chatSubmitFeedbackInput = z.object({
  requestId: z.string().min(1),
  rating: z.enum(['positive', 'negative']),
  note: z.string().trim().optional(),
  runId: z.string().optional(),
})

const studioExportSocialDraftInput = z.object({
  projectId: z.string().min(1),
  title: z.string().trim().optional(),
  body: z.string().trim().optional(),
  platforms: z.array(socialProviderKey).optional(),
  scheduledAt: z.string().datetime().optional(),
})

const jobOutput = z.object({
  id: z.string(),
  status: z.string(),
})

const brregLookupOutput = z.object({
  query: z.string(),
  count: z.number(),
  sourceUrl: z.string(),
  results: z.array(z.record(z.string(), z.unknown())),
})

const shippingQuoteOutput = z.object({
  quotes: z.array(z.record(z.string(), z.unknown())),
  errors: z.array(z.record(z.string(), z.unknown())).optional(),
})

const socialPostOutput = z.object({
  postId: z.string(),
  status: z.enum(['draft', 'pending_approval', 'scheduled', 'publishing', 'published', 'failed', 'blocked']),
})

const ticketOutput = z.object({
  ticketId: z.string().optional(),
  status: z.string(),
})

const scrapeOutput = z.object({
  title: z.string().optional(),
  url: z.string().optional(),
})

const incidentOutput = z.object({
  id: z.string(),
  status: z.string(),
})

const problemOutput = z.object({
  id: z.string(),
  status: z.string(),
})

const ticketConfigOutput = z.object({
  id: z.string(),
})

// Shared by any action whose dispatcher doesn't promote a domain-specific
// field to the top of the execution envelope the way tickets.* promotes
// ticketId — only the always-present envelope status is asserted here,
// matching the same minimal-claim posture as ticketOutput.
// Audit compliance (WS06-S6). Owner/admin only at the gateway and again in
// audit-core / org-core; no org id here, the session names the org. None of
// these is model-eligible (model-eligibility.ts stays an allowlist): an agent
// never reads or administers the audit log.
const auditVerifyChainInput = z.object({
  fromSeq: z.number().int().min(0).optional(),
  fromHash: z.string().regex(/^[0-9a-f]{64}$/).optional(),
  maxRows: z.number().int().min(1).max(1_000_000).optional(),
})

const auditExportLogInput = z.object({
  event: z.string().trim().max(128).optional(),
  userId: z.string().trim().max(128).optional(),
  plane: z.string().trim().max(128).optional(),
  outcome: z.string().trim().max(128).optional(),
  since: z.string().datetime().optional(),
  until: z.string().datetime().optional(),
})

const auditSiemEventFilter = z.object({
  events: z.array(z.string().trim().min(1).max(128)).max(128).optional(),
  planes: z.array(z.string().trim().min(1).max(128)).max(128).optional(),
})

const auditCreateSiemDestinationInput = z.object({
  kind: z.enum(['ndjson_webhook', 'splunk_hec']),
  url: z.string().url().max(2048).refine((value) => {
    try {
      return new URL(value).protocol === 'https:'
    } catch {
      return false
    }
  }, 'url must use https'),
  secret: z.string().min(1).max(512),
  enabled: z.boolean().optional(),
  eventFilter: auditSiemEventFilter.optional(),
})

const auditUpdateSiemDestinationInput = z.object({
  destinationId: z.string().min(1).max(64),
  expectedRevision: z.number().int().min(0),
  url: z.string().url().max(2048).optional(),
  secret: z.string().min(1).max(512).optional(),
  enabled: z.boolean().optional(),
  eventFilter: auditSiemEventFilter.optional(),
})

const auditDeleteSiemDestinationInput = z.object({
  destinationId: z.string().min(1).max(64),
})

const envelopeStatusOutput = z.object({
  status: z.string(),
})

// Registry entries are the product contract shared by humans and the Model Plane.
export const actionRegistry = [
  {
    id: 'knowledge.recrawl_source',
    label: 'Recrawl source',
    description: 'Refresh website or integration evidence through Ingestion Plane.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: sourceInput,
    outputSchema: runOutput,
  },
  {
    id: 'knowledge.scrape_url',
    label: 'Scrape page',
    description: 'Fetch a single web page via Quarry v2 and index it into the knowledge base.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: scrapeUrlInput,
    outputSchema: scrapeOutput,
  },
  {
    id: 'knowledge.crawl_site',
    label: 'Crawl site',
    description: 'Crawl a website with Quarry v2 and ingest discovered pages into Data Plane v2.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: crawlSiteInput,
    outputSchema: jobOutput,
  },
  {
    id: 'knowledge.import_source',
    label: 'Import source',
    description: 'Add a single web URL to the knowledge base via Quarry v2 (fetch + index).',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: importSourceInput,
    outputSchema: jobOutput,
  },
  {
    id: 'knowledge.upload_files',
    label: 'Upload files',
    description: 'Upload documents and ingest them into the knowledge base via imports-core.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: uploadFilesInput,
    outputSchema: jobOutput,
  },
  {
    id: 'knowledge.create_document',
    label: 'Create document',
    description: 'Add a document directly to the knowledge base via Data Plane\'s documents-api, without a fetch step.',
    ownerPlane: 'data',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: knowledgeCreateDocumentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'knowledge.extract_products',
    label: 'Extract products from page',
    description: 'Fetch a page and ask Model Plane to extract its product listings into structured data. Nothing is persisted by this action alone.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: knowledgeExtractProductsInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'knowledge.summarize_products',
    label: 'Summarize products',
    description: 'Ask Model Plane to summarize a chosen set of products into a markdown brief. Nothing is persisted by this action alone.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: knowledgeSummarizeProductsInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'knowledge.connect_source',
    label: 'Connect source',
    description: 'Import documents from a connected SaaS source (Notion, HubSpot, Salesforce, …) through imports-core.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: connectSourceInput,
    outputSchema: jobOutput,
  },
  {
    id: 'knowledge.register_sharepoint_source',
    label: 'Register SharePoint source',
    description: 'Register a SharePoint or OneDrive library as a finspo-core source and start its first sync.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: knowledgeRegisterSharePointSourceInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'operating_map.generate',
    label: 'Generate Operating Map',
    description: 'Ask Model Plane to synthesize an evidence-grounded AI Operating Map proposal from Verevon Knowledge.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: operatingMapGenerateInput,
    outputSchema: runOutput,
  },
  {
    id: 'operating_map.refresh',
    label: 'Refresh Operating Map',
    description: 'Create a new durable Operating Map proposal from the latest Knowledge evidence.',
    ownerPlane: 'data',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: operatingMapGenerateInput,
    outputSchema: runOutput,
  },
  {
    id: 'operating_map.review_proposal',
    label: 'Review Operating Map proposal',
    description: 'Accept or reject a generated Operating Map proposal. Accepted versions are published into the wiki knowledge path.',
    ownerPlane: 'data',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: operatingMapReviewInput,
    outputSchema: runOutput,
  },
  {
    id: 'operating_map.create_agent_blueprint',
    label: 'Create agent blueprint',
    description: 'Queue an agent blueprint suggestion from an approved Operating Map workflow.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: true,
    reversible: true,
    inputSchema: operatingMapBlueprintInput,
    outputSchema: runOutput,
  },
  {
    id: 'brreg.lookup_organization',
    label: 'Lookup Brreg organization',
    description: 'Look up Norwegian organizations by name or 9-digit organization number in Brreg Enhetsregisteret. Use this for registered names, organization numbers, legal form, address, industry code, and employee count.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: brregLookupInput,
    outputSchema: brregLookupOutput,
  },
  {
    id: 'shipping.get_quotes',
    label: 'Get shipping quotes',
    description: 'Compare live shipping quotes across the connected carrier fleet (Bring, DHL, UPS, FedEx, plus demo carriers) for a given origin, destination, and package. Returns cheapest-first pricing and transit days, plus a per-carrier error when a specific carrier could not be reached.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: shippingQuoteInput,
    outputSchema: shippingQuoteOutput,
  },
  {
    id: 'inbox.follow_conversation',
    label: 'Follow conversation',
    description: 'Save or remove the current operator’s personal follow preference for a support conversation.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxFollowConversationInput,
    outputSchema: runOutput,
  },
  {
    id: 'inbox.set_csat_preference',
    label: 'Set customer feedback preference',
    description: 'Record the support contact’s explicit preference for a future post-resolution satisfaction survey.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxSetCSATPreferenceInput,
    outputSchema: runOutput,
  },
  {
    id: 'inbox.add_tag',
    label: 'Add conversation tag',
    description: 'Add a label tag to a conversation.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxAddTagInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.remove_tag',
    label: 'Remove conversation tag',
    description: 'Remove a label tag from a conversation.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxRemoveTagInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.claim_draft_lease',
    label: 'Claim draft lease',
    description: 'Claim the exclusive lease to draft a reply on a conversation, preventing a concurrent editor from overwriting it.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxClaimDraftLeaseInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.release_draft_lease',
    label: 'Release draft lease',
    description: 'Release a previously claimed draft lease.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxReleaseDraftLeaseInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.save_draft',
    label: 'Save conversation draft',
    description: 'Save (or overwrite) the in-progress reply or internal note draft for a conversation. Requires an active organization.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxSaveDraftInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.delete_draft',
    label: 'Delete conversation draft',
    description: 'Discard the in-progress draft for a conversation. Requires an active organization.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxDeleteDraftInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.set_status',
    label: 'Set conversation status',
    description: 'Transition a conversation to a new support status.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxSetStatusInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.set_assignment',
    label: 'Set conversation assignment',
    description: 'Assign a conversation to a specific support agent.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxSetAssignmentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.send_reply',
    label: 'Send conversation reply',
    description: 'Send a customer-facing reply or an internal note on a conversation. A sent reply cannot be recalled.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: inboxSendReplyInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.submit_feedback',
    label: 'Submit pilot feedback',
    description: 'Submit a one-line friction report, landed as a new conversation in the org\'s own Inbox.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxSubmitFeedbackInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.review_ai_action',
    label: 'Review AI-proposed action',
    description: 'Approve or reject a model-proposed action (draft reply, internal note, ticket update, incident, or problem). Approving is what executes the proposal\'s real effect; this is the human checkpoint, not an action requiring a further one.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: inboxReviewAiActionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.create_draft_reply_proposal',
    label: 'Propose draft reply',
    description: 'Queue a model-drafted reply for human review via inbox.review_ai_action. Never sends anything on its own.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: true,
    reversible: true,
    inputSchema: inboxCreateDraftReplyProposalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.create_internal_note_proposal',
    label: 'Propose internal note',
    description: 'Queue a model-drafted internal note for human review via inbox.review_ai_action.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: true,
    reversible: true,
    inputSchema: inboxCreateInternalNoteProposalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.create_ticket_update_proposal',
    label: 'Propose ticket update',
    description: 'Queue a bounded field-level ticket update suggestion for human review. Resolution, closure, snooze, and ownership stay explicit human decisions and cannot be proposed this way.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: true,
    reversible: true,
    inputSchema: inboxCreateTicketUpdateProposalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.create_incident_proposal',
    label: 'Propose incident declaration',
    description: 'Queue a strictly review-gated incident declaration linked to an existing ticket, for human review.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: true,
    reversible: true,
    inputSchema: inboxCreateIncidentProposalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.create_problem_proposal',
    label: 'Propose problem record',
    description: 'Queue a proposed root-cause Problem record for human review. Cannot itself name an incident, owner, or lifecycle state.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: true,
    reversible: true,
    inputSchema: inboxCreateProblemProposalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'tickets.record_csat_outcome',
    label: 'Record customer satisfaction outcome',
    description: 'Record a 1–5 outcome received from a consented customer after the linked ticket is resolved. This does not send a survey.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCSATOutcomeInput,
    outputSchema: runOutput,
  },
  {
    id: 'tickets.create',
    label: 'Create ticket',
    description: 'Create a durable support ticket from a conversation.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCreateInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.classify_conversation',
    label: 'Classify conversation',
    description: 'Classify whether a conversation should become a ticket using evidence and policy.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: true,
    reversible: true,
    inputSchema: ticketClassifyInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.update',
    label: 'Update ticket',
    description: 'Update ticket work type, status, priority, severity, category, intent, SLA due date, or team-visible follow-up.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketUpdateInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.assign',
    label: 'Assign ticket',
    description: 'Assign a ticket to an owner or team.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketAssignInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.link_resource',
    label: 'Link resource',
    description: 'Link a ticket to a social post, campaign, order, document, or external record.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketLinkResourceInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.resolve',
    label: 'Resolve ticket',
    description: 'Mark a ticket as resolved after the customer issue or follow-up is complete.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketResolveInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.run_macro',
    label: 'Run ticket macro',
    description: 'Apply an active, preconfigured ticket macro to one durable ticket.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketRunMacroInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.create_macro',
    label: 'Create ticket macro',
    description: 'Create a bounded, reusable Ticketing macro with one reviewed ticket-status outcome.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCreateMacroInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.create_checklist',
    label: 'Create ticket checklist',
    description: 'Create a structured resolution checklist on one durable ticket.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCreateChecklistInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.update_checklist_item',
    label: 'Update ticket checklist item',
    description: 'Record completion state for one item in a ticket checklist.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketUpdateChecklistItemInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.create_side_conversation',
    label: 'Start ticket side conversation',
    description: 'Start an internal coordination thread on a ticket. It does not send to the customer or use Verevon Chat.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCreateSideConversationInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.add_side_conversation_message',
    label: 'Reply in ticket side conversation',
    description: 'Add an internal coordination reply to an open ticket side conversation.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketAddSideConversationMessageInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.update_side_conversation',
    label: 'Update ticket side conversation',
    description: 'Open or close an internal coordination thread on a ticket.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketUpdateSideConversationInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.record_chat_handoff',
    label: 'Record ticket Chat handoff request',
    description: 'Record that an operator requested a case-aware Chat handoff. It does not claim that Chat acted or that a customer message was sent.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketChatHandoffInput,
    outputSchema: ticketOutput,
  },
  {
    id: 'tickets.create_incident',
    label: 'Declare incident',
    description: 'Declare a support incident, independent of any single ticket.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCreateIncidentInput,
    outputSchema: incidentOutput,
  },
  {
    id: 'tickets.update_incident',
    label: 'Update incident',
    description: 'Update an incident\'s status, severity, or ownership.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketUpdateIncidentInput,
    outputSchema: incidentOutput,
  },
  {
    id: 'tickets.link_incident_ticket',
    label: 'Link ticket to incident',
    description: 'Attach a ticket to an incident with a stated relationship (affected, root cause, or related).',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketLinkIncidentInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'tickets.create_problem',
    label: 'Open problem record',
    description: 'Open a problem record to track a recurring or underlying root cause across incidents.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCreateProblemInput,
    outputSchema: problemOutput,
  },
  {
    id: 'tickets.update_problem',
    label: 'Update problem record',
    description: 'Update a problem record\'s status, ownership, or root cause.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketUpdateProblemInput,
    outputSchema: problemOutput,
  },
  {
    id: 'tickets.create_sla_policy',
    label: 'Create SLA policy',
    description: 'Create an SLA policy that sets response and resolution targets for tickets matching its conditions.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCreateSlaPolicyInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'tickets.update_sla_policy',
    label: 'Update SLA policy',
    description: 'Update an SLA policy\'s targets, conditions, or active state.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketUpdateSlaPolicyInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'tickets.create_automation_rule',
    label: 'Create ticket automation rule',
    description: 'Create a rule that fires stated actions on tickets when its trigger event and conditions match. Every matching ticket going forward is affected, so this stays approval-gated like other workflow-policy changes.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: ticketCreateAutomationRuleInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'tickets.update_automation_rule',
    label: 'Update ticket automation rule',
    description: 'Update an automation rule\'s trigger, conditions, actions, or active state.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: ticketUpdateAutomationRuleInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'tickets.create_team',
    label: 'Create ticket team',
    description: 'Create a team that tickets can be assigned to.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCreateTeamInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'tickets.update_team',
    label: 'Update ticket team',
    description: 'Rename, redescribe, or (de)activate a ticket team.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketUpdateTeamInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'tickets.create_view',
    label: 'Create ticket view',
    description: 'Create a saved ticket view (a named filter/sort/grouping) scoped to the org, a team, or the caller.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketCreateViewInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'tickets.update_view',
    label: 'Update ticket view',
    description: 'Update a saved ticket view\'s filter, sort, grouping, or visibility.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketUpdateViewInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'tickets.update_macro',
    label: 'Update ticket macro',
    description: 'Update a macro\'s stated actions, conditions, visibility, or active state. Matches tickets.create_macro\'s posture: not approval-gated on its own, because the macro only takes effect when tickets.run_macro is separately invoked.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ticketUpdateMacroInput,
    outputSchema: ticketConfigOutput,
  },
  {
    id: 'spaces.create_personal_space',
    label: 'Create personal Space',
    description: 'Create (or idempotently resolve) the caller\'s own personal Space for the active organization.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceCreatePersonalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.ensure_organization_room',
    label: 'Ensure organization room',
    description: 'Idempotently provision the organization\'s single shared room, the org-wide channel every member lands in.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceEnsureOrganizationRoomInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.update_space_instructions',
    label: 'Update Space instructions',
    description: 'Update the standing instructions agents in a Space follow. Requires the editor, manager, or owner room role.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceUpdateInstructionsInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.create_space_agent',
    label: 'Create Space agent',
    description: 'Create a new agent definition and grant it a place in this Space. No removal path exists yet, so this is not reversible.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceCreateAgentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.bind_space_agent',
    label: 'Bind existing agent to Space',
    description: 'Grant an org\'s existing agent definition a place in this Space. No removal path exists yet, so this is not reversible.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceBindAgentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.request_personal_space_deletion',
    label: 'Request personal Space deletion',
    description: 'Request deletion of the caller\'s own personal Space. Irreversible once processed.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: spaceRequestPersonalDeletionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.update_thread_presentation',
    label: 'Update Space thread presentation',
    description: 'Retitle or pin a post in a room. Only the member who started the post may change it.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceUpdateThreadPresentationInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.mark_read',
    label: 'Mark Space read',
    description: 'Advance the caller\'s own read marker for a Space to now.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceMarkReadInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.record_presence',
    label: 'Record Space presence',
    description: '"I am here" — and, in the same answer, who else is. A heartbeat the room already polls on.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceRecordPresenceInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.set_agent_state',
    label: 'Pause or resume Space agent',
    description: 'Pause or resume one bound agent in this room only. Gated to the room\'s owner or manager. Use spaces.revoke_agent to remove it instead.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceSetAgentStateInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.revoke_agent',
    label: 'Revoke Space agent',
    description: 'Revoke a bound agent from this room. The binding record survives for audit history; reinstating means binding again through the grant flow.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceRevokeAgentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.create_room',
    label: 'Create room',
    description: 'Create a named shared room. It starts with its creator and grows by explicit grant; it does not inherit the organization\'s roster.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceCreateRoomInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.add_member',
    label: 'Add Space member',
    description: 'Add one person to a named room. Gated to the room\'s owner or manager; the person must already be in the organization.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceAddMemberInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.remove_member',
    label: 'Remove Space member',
    description: 'Remove one person from a named room. The room\'s registered owner cannot be removed this way.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceRemoveMemberInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'social.create_draft',
    label: 'Create social draft',
    description: 'Create a social post draft from manual, inbox, knowledge, or campaign context.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: socialDraftInput,
    outputSchema: socialPostOutput,
  },
  {
    id: 'social.schedule_post',
    label: 'Schedule social post',
    description: 'Reserve a calendar slot for an approved social post draft.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: true,
    reversible: true,
    inputSchema: socialScheduleInput,
    outputSchema: socialPostOutput,
  },
  {
    id: 'social.publish_post',
    label: 'Publish social post',
    description: 'Publish an approved social post through the provider-specific social adapter workflow.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: socialPublishInput,
    outputSchema: socialPostOutput,
  },
  {
    id: 'social.create_campaign',
    label: 'Create social campaign',
    description: 'Create a social campaign shell to organize drafts and posts under a shared brief and goal.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: socialCreateCampaignInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'social.create_draft_from_inbox',
    label: 'Create social draft from ticket',
    description: 'Draft a public follow-up post from an Inbox ticket\'s context. Composes real draft content from ticket fields; produces a draft only, never publishes.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: socialCreateDraftFromInboxInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'social.decide_approval',
    label: 'Decide social approval',
    description: 'Approve or reject a pending social post. This is the human review gate social.publish_post\'s approvalId depends on.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: socialDecideApprovalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'workflows.toggle_policy',
    label: 'Toggle workflow policy',
    description: 'Enable or pause a workflow with audit and rollback tracking.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: workflowInput,
    outputSchema: runOutput,
  },
  {
    id: 'notifications.mark_read',
    label: 'Mark notification read',
    description: 'Mark a single notification as read.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: notificationMarkReadInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'notifications.mark_all_read',
    label: 'Mark all notifications read',
    description: 'Mark every notification as read.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: notificationMarkAllReadInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'notifications.delete',
    label: 'Delete notification',
    description: 'Delete a single notification.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: notificationDeleteInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'notifications.update_preference',
    label: 'Update notification preference',
    description: 'Enable or disable one notification channel for one event type.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: notificationUpdatePreferenceInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'navbar.save_theme',
    label: 'Save appearance theme',
    description: 'Save the caller\'s light/dark/system theme and accent color preference.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: navbarSaveThemeInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'navbar.mark_notification_read',
    label: 'Mark navbar notification read',
    description: 'Mark a single navbar notification as read. A separate surface from notifications.mark_read, backed by the same conversation-core operation.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: navbarMarkNotificationReadInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'navbar.create_calendar_event',
    label: 'Create calendar event',
    description: 'Create a scheduled calendar event.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: navbarCreateCalendarEventInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'navbar.create_calendar_note',
    label: 'Create calendar note',
    description: 'Create a dated calendar note.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: navbarCreateCalendarNoteInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'navbar.submit_support_request',
    label: 'Submit support request',
    description: 'Submit a support request from the navbar help control.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: navbarSubmitSupportRequestInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.set_conversation_pinned',
    label: 'Set conversation pinned',
    description: 'Pin or unpin a conversation in the caller\'s own inbox workspace view.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxSetConversationPinnedInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'inbox.set_conversation_read',
    label: 'Set conversation read',
    description: 'Mark a conversation read or unread in the caller\'s own inbox workspace view.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: inboxSetConversationReadInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ownership.share_document',
    label: 'Share document',
    description: 'Grant another org member access to a document.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ownershipShareDocumentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ownership.revoke_document_share',
    label: 'Revoke document share',
    description: 'Revoke a previously granted document share.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ownershipRevokeDocumentShareInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'memory.delete',
    label: 'Delete memory',
    description: 'Delete a single durable memory entry about the caller. Destructive and non-recoverable.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: memoryDeleteInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'memory.set_state',
    label: 'Set memory state',
    description: 'Turn the caller\'s own memory on, pause it, or turn it off. Paused keeps what is stored but neither recalls nor learns; off also hides it.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: memorySetStateInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'memory.reset',
    label: 'Reset memory',
    description: 'Erase everything Verevon remembers about the caller. Destructive and non-recoverable.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: memoryResetInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'memory.remember',
    label: 'Remember this',
    description: 'Save one fact the caller chose to a durable personal memory. Refused while memory is paused, off, or disabled by the organization.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: memoryRememberInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'learning.accept_skill',
    label: 'Accept learned skill',
    description: 'Accept a skill Verevon proposed from the caller\'s own conversation, so it is used in their chats. Only the owner of the proposal can decide it.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: learningSkillDecisionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'learning.reject_skill',
    label: 'Reject learned skill',
    description: 'Reject a skill Verevon proposed, so it is never used. Only the owner of the proposal can decide it.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: learningSkillDecisionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'learning.undo',
    label: 'Undo learning',
    description: 'Undo the newest change to one of the caller\'s learned skills by restoring the snapshot taken before it, or removing the skill if the change created it. Only the owner can undo.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: learningUndoInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'settings.update_me',
    label: 'Update profile',
    description: 'Update the caller\'s own profile fields (name, avatar, timezone, and similar).',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: settingsUpdateMeInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'settings.update_preferences',
    label: 'Update preferences',
    description: 'Update the caller\'s own preferences (theme, language, timezone, notification and crawl-ingest settings).',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: settingsUpdatePreferencesInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'settings.update_setting',
    label: 'Update setting',
    description: 'Set a single named user setting to a new value.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: settingsUpdateSettingInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'settings.create_api_key',
    label: 'Create API key',
    description: 'Mint a new API key for the caller\'s organization. The response carries the raw secret exactly once.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: settingsCreateApiKeyInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'settings.delete_api_key',
    label: 'Delete API key',
    description: 'Revoke an existing API key.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: settingsDeleteApiKeyInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'mcp.delete_server',
    label: 'Delete MCP server',
    description: 'Delete a connected MCP server.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: mcpDeleteServerInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'mcp.share_server',
    label: 'Share MCP server',
    description: 'Replace a user-owned MCP server\'s share set with the given users. Owner-only.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: mcpShareServerInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'mcp.approve_tool',
    label: 'Approve MCP tool',
    description: 'Accept the definition of one tool of an MCP server that the person reviewed, so a tool whose definition changed can be used again. The body names the digest the person saw (approvedDigest); it is pinned only if it is still the observed one, otherwise the call is refused 409 and the person reviews the tool again. Allowed for the owner of a user-scoped server and for an admin of a shared one, derived by the owner from the verified credential. Refused typed under zero data retention. Never model-executable: a model does not approve the tools it is offered.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: mcpApproveToolInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'tools.set_policy',
    label: 'Set tool policy',
    description: 'Set an allow, ask or never policy on the tools of a capability, for the caller or, for an org admin, for the whole org. never and ask tighten and apply to every tool; allow relaxes and is accepted only for an approved MCP tool the scope may relax, never for a built-in tool, an owner effect or an unclassified tool, and it must carry the digest of the definition the person reviewed: a changed definition is asked about again. The strictest policy in force wins, whoever set it. Refused typed under zero data retention, and a policy only takes effect where the deployment has switched policies on. Never model-executable: a model does not set the rules it runs under.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: toolsSetPolicyInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.set_agent_policy',
    label: 'Set organization agent policy',
    description: 'Replace the organization-wide agent policy: the highest mode agents may run in, the highest rung they may be granted, lockdown, the MCP servers they may use and how their tools are approved. Takes the revision it was read at and is refused if the policy has changed since. Takes effect only where the deployment has switched policy enforcement on. An org admin only. Refused typed under zero data retention. Never model-executable: a model does not set the rules it runs under.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgSetAgentPolicyInput,
    outputSchema: orgAgentPolicyOutput,
  },
  {
    id: 'org.preview_effective_policy',
    label: 'Preview effective agent policy',
    description: 'Read the organization agent policy in force, setting by setting: the value each takes, whether it is enforced or reserved, and whether it comes from the organization or the platform default. Any member of the organization may read it. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgPreviewEffectivePolicyInput,
    outputSchema: orgEffectivePolicyOutput,
  },
  {
    id: 'org.create_hook',
    label: 'Add organization hook rule',
    description: 'Add a rule that denies, or asks first about, a tool call in the agent runs of the organization. A rule only narrows a run, and a post-tool rule may only deny, because the tool has already acted by then. Takes effect only where the deployment has switched durable hook rules on. An org admin only. Refused typed under zero data retention. Never model-executable: a model does not set the rules it runs under.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgCreateHookInput,
    outputSchema: orgHookRuleOutput,
  },
  {
    id: 'org.update_hook',
    label: 'Update organization hook rule',
    description: 'Replace the content of an active organization hook rule, under the same rules as adding one. An org admin only. Refused typed under zero data retention. Never model-executable.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgUpdateHookInput,
    outputSchema: orgHookRuleOutput,
  },
  {
    id: 'org.delete_hook',
    label: 'Retire organization hook rule',
    description: 'Retire an active organization hook rule. The rule stays in the history and can be added again. An org admin only. Refused typed under zero data retention. Never model-executable.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgDeleteHookInput,
    outputSchema: orgHookRevokeOutput,
  },
  {
    id: 'monitoring.check_now',
    label: 'Check page now',
    description: 'Fetch a monitored URL, fingerprint it, and compare against the org\'s latest baseline.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: monitoringCheckNowInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'privacy.erase_my_account',
    label: 'Erase my account',
    description: 'Irreversibly erase the caller\'s own account (GDPR Art. 17). Requires a client-side typed-confirm and step-up re-authentication that this contract cannot itself enforce.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: privacyEraseMyAccountInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'leads.create_list',
    label: 'Create lead list',
    description: 'Save a named list of company leads.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: leadsCreateListInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'leads.delete_list',
    label: 'Delete lead list',
    description: 'Delete a saved lead list.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: leadsDeleteListInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'finetune.create_job',
    label: 'Create fine-tune job',
    description: 'Start a fine-tuning job for an agent against a base model and training file.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: finetuneCreateJobInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'finetune.cancel_job',
    label: 'Cancel fine-tune job',
    description: 'Cancel a running or queued fine-tune job.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: finetuneCancelJobInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'finetune.deploy_job',
    label: 'Deploy fine-tune job',
    description: 'Promote a completed fine-tune job to a deployment tier (developer or production).',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: finetuneDeployJobInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'studio.create_project',
    label: 'Create Studio project',
    description: 'Create a new Studio canvas project. Studio\'s store is RAM-only and explicitly ephemeral.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: studioCreateProjectInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'studio.save_project',
    label: 'Save Studio project',
    description: 'Save changes to an existing Studio canvas project.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: studioSaveProjectInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'studio.export_social_draft',
    label: 'Export Studio project to social draft',
    description: 'Export a Studio canvas project into a social post draft. Produces a draft only, never publishes.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: studioExportSocialDraftInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.create_run',
    label: 'Create ingestion run',
    description: 'Start a one-off ingestion run: a batch of URLs, or a single-URL crawl/scrape by kind.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ingestionsCreateRunInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.create_schedule',
    label: 'Create ingestion schedule',
    description: 'Create a recurring or one-time-scheduled ingestion job for a target URL.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ingestionsCreateScheduleInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.run_schedule_action',
    label: 'Run schedule action',
    description: 'Pause, unpause, trigger, backfill, or delete an existing ingestion schedule.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ingestionsRunScheduleActionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.create_source',
    label: 'Create ingestion source',
    description: 'Register a new knowledge source to ingest from a URL.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ingestionsCreateSourceInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.delete_source',
    label: 'Delete ingestion source',
    description: 'Remove a registered ingestion source.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: ingestionsDeleteSourceInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'integrations.start_connect_session',
    label: 'Start integration connect session',
    description: 'Start an OAuth-style connect session for a third-party provider.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: integrationsStartConnectSessionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'integrations.disconnect',
    label: 'Disconnect integration',
    description: 'Disconnect a connected third-party integration.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: integrationsDisconnectInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'integrations.start_chatgpt_subscription',
    label: 'Start ChatGPT subscription connection',
    description: 'Start the official ChatGPT device-code connection without collecting an API key.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: z.object({}),
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'integrations.disconnect_chatgpt_subscription',
    label: 'Disconnect ChatGPT subscription',
    description: 'Disconnect the workspace from an official ChatGPT subscription connection.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: integrationsDisconnectChatGptSubscriptionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'integrations.trigger_sync',
    label: 'Trigger integration sync',
    description: 'Trigger an on-demand sync for a connected integration.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: integrationsTriggerSyncInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'integrations.trigger_inbox_sync',
    label: 'Trigger inbox sync',
    description: 'Trigger an on-demand inbox sync for one channel of a connected integration.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: integrationsTriggerInboxSyncInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'integrations.extend_inbox_history',
    label: 'Extend inbox history',
    description: 'Queue a bounded extension of provider inbox history for a connected integration.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: integrationsExtendInboxHistoryInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'integrations.browse_connection',
    label: 'Browse connected app',
    description: 'List mail, calendar or file items in one of the person\'s own connected Microsoft or Google accounts. Read-only: the owner checks that the caller owns the connection, nothing is written and no approval is needed. Only the four listings in the operation enum are accepted.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: integrationsBrowseConnectionInput,
    outputSchema: integrationsConnectorActionOutput,
  },
  {
    id: 'integrations.fetch_connection_content',
    label: 'Read connected app content',
    description: 'Read one mail message or file from one of the person\'s own connected Microsoft or Google accounts. Read-only, with no approval and no write; the owner applies the same ownership check as the listing.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: integrationsFetchConnectionContentInput,
    outputSchema: integrationsConnectorActionOutput,
  },
  {
    id: 'org.soft_delete',
    label: 'Soft-delete organization',
    description: 'Open the organization\'s 30-day GDPR deletion grace window. Reversible via org.restore only inside that window.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgSoftDeleteInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.restore',
    label: 'Restore organization',
    description: 'Cancel a pending organization deletion inside its grace window.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgRestoreInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.mark_exported',
    label: 'Mark data exported',
    description: 'Record that the caller received their personal-data export ahead of a scheduled organization purge.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgMarkExportedInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.acknowledge_deletion',
    label: 'Acknowledge deletion notice',
    description: 'Record that the caller acknowledged the organization\'s pending-deletion notice.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgAcknowledgeDeletionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.set_quota',
    label: 'Set organization quota',
    description: 'Set the ceiling for one enforced organization quota (e.g. max cost or tokens per run).',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgSetQuotaInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.set_model_entitlement',
    label: 'Set organization model access',
    description: 'Replace which chat models the organization and each role may use. Takes effect for every member and is refused when it would leave a Zero Data Retention organization with no attested model.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgSetModelEntitlementInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.update_instructions',
    label: 'Update organization instructions',
    description: 'Update the organization-wide standing instructions agents follow.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgUpdateInstructionsInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.update_zdr',
    label: 'Update organization ZDR posture',
    description: 'Toggle the organization\'s Zero Data Retention posture. A compliance-relevant setting affecting every member.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgUpdateZdrInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.update_support_ai_mode',
    label: 'Update organization AI support mode',
    description: 'Set the organization\'s support-AI autonomy posture (off, assist, or review).',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgUpdateSupportAiModeInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.update_agent_learning_policy',
    label: 'Update organization memory and learning policy',
    description: 'Allow or disable memory for the whole organization, and opt in or out of learning review of conversations. Affects every member.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgUpdateAgentLearningPolicyInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'membership.invite_member',
    label: 'Invite organization member',
    description: 'Invite a new member to the organization with a stated role.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: membershipInviteMemberInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'membership.remove_member',
    label: 'Remove organization member',
    description: 'Remove a member from the organization.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: membershipRemoveMemberInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'membership.update_member_role',
    label: 'Update member role',
    description: 'Change a member\'s role between member and admin. Granting admin is a real privilege escalation.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: membershipUpdateMemberRoleInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'organization.switch_active',
    label: 'Switch active organization',
    description: 'Switch which organization is active for the caller\'s own session.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: organizationSwitchActiveInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.approve_plan',
    label: 'Approve run autonomy',
    description: 'Grant a planning run an autonomy rung, up to danger_full_access, for the thread it belongs to. When planId is given the structured plan is approved with the person\'s editedSteps in the same decision, and the grant records its expiry. The owner independently verifies the caller owns the run, and the grant is held by the server, not by the client. Never model-executable: a model does not grant itself authority.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatApprovePlanInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.execute_approved_plan',
    label: 'Run approved plan',
    description: 'Start a NEW executing run on a thread whose plan the person approved, under the rung the server holds for that thread. The plan run itself stays read-only and is never widened. The turn is refused typed, with nothing run, when the server holds no live grant (plan_grant_unavailable) or the capability is switched off on the deployment (plan_execute_disabled). One approval is one execute turn on the client, and the grant is never accepted from a client. It is the person\'s click after they approved the plan, and never model-executable.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatExecuteApprovedPlanInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.cancel_invocation',
    label: 'Cancel invocation',
    description: 'Cancel an in-flight chat invocation.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatCancelInvocationInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.queue_invocation_input',
    label: 'Queue invocation input',
    description: 'Deliver a message typed mid-stream to a running agent invocation. With a clientMessageId the owner stores the message under that id before delivering it: a repeat of the same id is one transcript row, and the answer names the message (client_message_id, message_id, persisted). A run that ended before the message landed answers resend_as_new_turn (persisted says whether it was already stored): send it as an ordinary turn under the same id.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatQueueInvocationInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.run.continue',
    label: 'Continue approved run',
    description: 'Resume a run that paused for a granted approval: the approved call runs once under the authority the pause recorded, then the same run finishes. Streams the continuation. The claim token is single use, and every consumed approval is refused with 409 continuation_already_consumed, the same token included. The closed refusals, and what each says about the spend, live in chat-run-continue-contract.ts. A connected or error frame carrying approval_spent true is final: follow the run and never retry with a new token. After an ambiguous failure (a 502 upstream_unavailable, a 504, a network error, an error frame from the closed set without approval_spent, or a not_resumable, approval_not_found or continue_disabled that a spent approval can also get) do not assume either way: follow the run by its id with the existing run read; the run read is the arbiter. A retry with the same token is answered either with the continuation or with a refusal, and only 409 continuation_already_consumed means the click was spent; not_resumable, approval_not_found and continue_disabled leave the spend unknown. Following the run does not promise that the run is live: after continue_unavailable a run may still be recovered inside the recovery window, about 15 minutes from the consume. The thread\'s approved plan and its grant are not consumed. A PARKED run (one the executor parked at a round boundary) is continued through this same action in its park form: omit approvalId and send only runId and claimToken. No approval is read and nothing is spent, so a failure frame carrying park_continue true never says approval_spent. It uses the closed refusals, including park_not_restorable, park_route_unavailable and park_not_due, and the client follows the run by its id with the existing run read. The run read is the arbiter of whether the run stays parked: not_resumable and park_not_restorable say it does not, continue_outcome_unknown, continue_in_progress and a bare agent_dispatch_unavailable on a park stream leave that unknown, and every other park refusal leaves it parked.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatRunContinueInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.answer_question',
    label: 'Answer a run question',
    description: 'Answer, or skip, a question a run put to the person (a run_questions row, not an approval). The owner records one answer per question: an identical retry is accepted again, a different second answer is refused 409 question_already_answered, a question past its time is refused 410 question_expired, another person\'s question is 404 question_not_found, and an answer that does not fit the question is 422 invalid_answer with the refused field and reason. An answer is information, never an authority grant, and it cannot approve a tool call or a plan. Refused typed under zero data retention and for a waived credential, since a zero-retention or waived run holds no question. Never model-executable: the answer is the person\'s own.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatAnswerQuestionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'approvals.decide',
    label: 'Decide an approval',
    description: 'Approve or reject one pending approval of a run, for this one call. Only the person the approval is requested of can decide it; the decider is the verified credential, never an input, and a client field naming a decider, a decision source or a grant is refused 400. A scope other than once is refused 422 scope_not_enabled until standing grants exist, so a caller never believes it granted more than it did. A decision is idempotent, and a different second decision is a 412 conflict. Refused typed under zero data retention and for a waived credential, since such a run holds no approval. Never model-executable: an approval is the person\'s own authority, and a model deciding its own approval would void the gate.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: approvalsDecideInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.clear_threads',
    label: 'Clear all chat threads',
    description: 'Delete every chat thread for the caller. Irreversible bulk deletion.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: chatClearThreadsInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.delete_thread',
    label: 'Delete chat thread',
    description: 'Delete a single chat thread.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatDeleteThreadInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.mark_thread_seen',
    label: 'Mark chat thread seen',
    description:
      "Mark one of the caller's threads read. Owner-only; refused for zero-retention and temporary chats, which keep read state on the client. Never model-executable.",
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatMarkThreadSeenInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.save_thread_snapshot',
    label: 'Save chat thread snapshot',
    description: 'Save a chat thread\'s title, pin state, or preview.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatSaveThreadSnapshotInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.submit_feedback',
    label: 'Submit chat feedback',
    description: 'Submit a positive or negative rating on a chat response.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatSubmitFeedbackInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.upload_document',
    label: 'Upload chat document',
    description: 'Ingest an attached text document as a durable Data Plane document so later turns can retrieve it. Refused under ZDR.',
    ownerPlane: 'data',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatUploadDocumentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.extract_document',
    label: 'Read chat attachment',
    description: 'Extract a bounded document in memory for this turn without storing it in organization knowledge.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: z.object({ filename: z.string().min(1).max(255), content_base64: z.string().min(1).max(1_400_000), content_type: z.string().max(128).optional() }),
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.set_connector_scope',
    label: 'Choose connected apps for a turn',
    description: 'Name the connected accounts one chat turn may read: the composer\'s connected-apps chips, sent as connector_scope on that turn only. At most 8 distinct connections, and an empty list clears the choice. The selection is the person\'s consent for that turn, so it is never remembered between turns, and a turn that is zero-retention, Space-scoped or filtered to one source never carries it. Never model-executable: a model does not choose which mailbox it reads.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatSetConnectorScopeInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.set_agent_mode',
    label: 'Choose the agent mode for a chat turn',
    description: 'Name the agent mode one chat turn runs under: lockdown (no agent runs), read_only (a plan run that refuses writes), ask_first (asks before a risky call) or auto (no gate beyond the platform floor). The server resolves it strictest-wins against the posture the platform already sets, so a choice can only tighten. It is sent as agent_mode on that turn only and is never remembered between turns. Never model-executable: a model does not choose the mode it runs under.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatSetAgentModeInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'audio.transcribe',
    label: 'Transcribe audio',
    description: 'Transcribe a base64-encoded audio recording to text.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: audioTranscribeInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'audio.dictate',
    label: 'Dictate audio',
    description: 'Transcribe a base64-encoded audio recording and clean it up into dictated text.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: audioDictateInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'orchestration.decide_approval',
    label: 'Decide orchestration approval',
    description: 'Approve or reject a pending orchestration-run approval. This is the human checkpoint the run is waiting on. On a reject, reason may carry redirect text (what the person wants the agent to do instead); it is capped at 1000 characters. A reject with reason aborts the run today unless the owner accepts the redirect.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: orchestrationDecideApprovalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'orchestration.resume_run',
    label: 'Resume orchestration run',
    description: 'Resume a paused orchestration run.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: orchestrationResumeRunInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'orchestration.cancel_run',
    label: 'Cancel orchestration run',
    description: 'Cancel an orchestration run entirely.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: orchestrationCancelRunInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_run.start',
    label: 'Start browser agent run',
    description: 'Start a durable, multi-step browser-agent run toward a stated goal. The wire-level requireApproval flag is confirmed inert (never read server-side), so this registry\'s own approval gate is the only real control here.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: browserRunStartInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_run.control',
    label: 'Control browser agent run',
    description: 'Pause, resume, or stop a running browser-agent run.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserRunControlInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_omnibox.search',
    label: 'Search from the browser omnibox',
    description: 'Run a Verevon web search for free text typed in the browser address bar and show the results in the browser frame. Human-only: the person operates the omnibox, a model does not.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserOmniboxSearchInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_omnibox.suggest',
    label: 'Suggest browser omnibox queries',
    description: 'Query suggestions for the browser omnibox dropdown. Empty for zero-data-retention callers. Human-only.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserOmniboxSuggestInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_evidence.compile_procedure',
    label: 'Compile browser procedure',
    description: 'Compile a replay candidate from the receipts of a browser session and show its promotion check. Typed text is never shown. Human-only, and unavailable to zero-data-retention sessions.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserEvidenceCompileProcedureInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_session.create',
    label: 'Create browser session',
    description: 'Open a new managed browser session at a target URL, optionally attaching a persistent or ephemeral profile.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserCreateSessionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_session.close',
    label: 'Close browser session',
    description: 'Close a running browser session and release its resources.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: browserCloseSessionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_session.keepalive',
    label: 'Keep browser session alive',
    description: 'Extend a person\'s open browser session grant by one broker interval while the session is still in use. Returns only the new expiry time. Starts no run and changes no page.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserKeepAliveSessionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_tab.create',
    label: 'Open browser tab',
    description: 'Open a new tab within an existing browser session, optionally navigating it to a URL.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserCreateTabInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_tab.select',
    label: 'Select browser tab',
    description: 'Switch a browser session\'s active tab.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserSelectTabInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_tab.close',
    label: 'Close browser tab',
    description: 'Close a single tab within a browser session.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: browserCloseTabInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_action.run',
    label: 'Run browser action',
    description: 'Execute one browser action (click, type, navigate, etc.) against a live session. The owner rejects agent-issued actions while a human holds control, and sanitizes the action itself (blocks private navigation, script evaluation, and unbounded coordinate takeover) — this stays approval-gated at the registry level because a sanitized action can still have arbitrary, irreversible effects on the target page.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: browserRunActionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_action.set_control_mode',
    label: 'Set browser control mode',
    description: 'Hand browser-session control between agent and human. Control authority lives in Quarry; this is a thin proxy to it.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserSetControlModeInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_action.suggest',
    label: 'Suggest browser action',
    description: 'Ask the model for a suggested next browser action given the session\'s last observation. Read-only — suggests, does not execute.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserSuggestActionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_profile.create',
    label: 'Create browser profile',
    description: 'Create a named, scoped browser profile (cookies/storage persisted across sessions per its scope). Ephemeral scope is rejected here — that is what "no profile at all" already means.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserCreateProfileInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_profile.rename',
    label: 'Rename or rescope browser profile',
    description: 'Rename and/or rescope an existing stored browser profile. At least one field must be present; rescoping to ephemeral is rejected (delete the profile instead).',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserRenameProfileInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_profile.delete',
    label: 'Delete browser profile',
    description: 'Permanently delete a stored browser profile and its persisted cookies/storage.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: true,
    reversible: false,
    inputSchema: browserDeleteProfileInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser_profile.probe_restore',
    label: 'Probe browser profile restore',
    description: 'Check, without committing a session, whether a stored profile can restore authenticated access to a target URL. Read-only.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserProbeProfileRestoreInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'browser.blocked.try_other_source',
    label: 'Leave a site that blocks automation',
    description: 'The person chose another source after a site blocked automated browsing (a strong bot-wall signal): stops the given browser-agent run when one is named (it must be one of the caller own runs; it is not checked against the session), treating a run that already ended as nothing left to stop, and closes the session. Where run controls are disabled (zero data retention) the run cannot be stopped, so the session is closed anyway and the result says runControlsDisabled. Any other failure to stop the run fails the action before the session is closed. No page content is read or kept, and nothing tries to get past the block.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: browserBlockedTryOtherSourceInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'router_policy.update',
    label: 'Update model router policy',
    description: 'Replace the organization\'s entire model-routing policy document (cost caps, complexity scoring, which model serves which tier). The owner enforces no admin gate of its own today, so this stays approval-gated at the registry level.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: routerPolicyUpdateInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'audit.verify_chain',
    label: 'Verify audit log integrity',
    description: 'Recompute the organization\'s audit hash chain and report verified, verified_with_legacy, broken (with the first broken position), partial or key_unavailable. Read-only; owner or admin, human sessions only. The run is itself recorded in the audit log.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: auditVerifyChainInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'audit.export_log',
    label: 'Export audit log',
    description: 'Page the organization\'s full chained audit log, each row carrying its hash and previous hash so it can be verified offline. Owner or admin, human sessions only; every read is recorded in the audit log.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: auditExportLogInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'audit.create_siem_destination',
    label: 'Add SIEM destination',
    description: 'Register an https endpoint (NDJSON webhook or Splunk HEC) that receives the organization\'s audit log. The secret is write-only and sealed by Control. Owner or admin, human sessions only; plan-gated.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: auditCreateSiemDestinationInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'audit.update_siem_destination',
    label: 'Update SIEM destination',
    description: 'Change, pause or resume one SIEM destination, guarded by the revision it was read at. Owner or admin, human sessions only.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: auditUpdateSiemDestinationInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'audit.delete_siem_destination',
    label: 'Remove SIEM destination',
    description: 'Remove one SIEM destination and its sealed secret. Rows already delivered stay with the receiver. Owner or admin, human sessions only.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: auditDeleteSiemDestinationInput,
    outputSchema: envelopeStatusOutput,
  },
] as const satisfies readonly ActionDescriptor[]

export type ActionId = (typeof actionRegistry)[number]['id']

export function getActionDescriptor(actionId: ActionId) {
  return actionRegistry.find((action) => action.id === actionId)
}
