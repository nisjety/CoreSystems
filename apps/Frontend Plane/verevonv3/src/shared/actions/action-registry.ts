import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'
import { spaceActions } from '@/shared/actions/domains/spaces'
import {
  threadArchiveAction,
  threadExportInput,
  threadImportInput,
  threadGrantSnapshotInput,
  threadMarkUnreadAction,
  threadRevokeSnapshotInput,
  threadShareSnapshotInput,
  threadStarAction,
} from '@/shared/actions/chat-history'
import {
  chatLibraryListInput,
  chatLibraryOpenOriginInput,
  chatLibraryReadTextInput,
  chatLibraryShareInput,
} from '@/shared/actions/chat-library'
import { reviewRunInput } from '@/shared/actions/chat-review'
import { checkMailEditLimits } from '@/shared/approvals/approval-edit-limits'

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

// The caller's own profile text (EP-8 S4), bounded as session-core bounds it, and the
// revision it was edited against; whose profile it is never comes from here.
const memoryProfileSaveInput = z.object({
  content: z.string().max(1400),
  expectedRevision: z.number().int().min(0),
})

// `force` replaces an edited profile; the person's own button always sends it.
const memoryProfileRebuildInput = z.object({
  force: z.boolean(),
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

// /learn (EP-11 S7): one of the caller's own saved conversations and an optional note of at most
// 500 characters, bounded as the server bounds them. Whose conversation it is comes from the
// verified session, never from here.
const learningLearnFromThreadInput = z.object({
  threadId: z.string().trim().min(1).max(128),
  note: z.string().trim().max(500).optional(),
})

// Teach mode (F-t3-081): one run the person drove or took over in the browser, the skill's name, and an
// optional hint of at most 500 characters that the draft reads as data. The run id is bounded as the
// gateway bounds it; whose run it is comes from the verified session, never from here.
const learningTeachFromRunInput = z.object({
  runId: z.string().trim().min(1).max(64),
  name: z.string().trim().min(1).max(64),
  goalHint: z.string().trim().max(500).optional(),
})

// The answer to a teach: the proposal's id and the review state it starts in (`proposed`).
const learningTeachFromRunOutput = z.object({
  skill_id: z.string().min(1),
  review_state: z.string(),
})

// A skill the person saves from one of their own answers (EP-11 S1). The name, description and
// content are bounded as the server bounds them, and the owner and the organisation come from the
// verified session, never from here.
const skillsSaveFromMessageInput = z.object({
  threadId: z.string().min(1).max(128),
  messageId: z.string().min(1).max(128),
  name: z.string().trim().min(1).max(64),
  description: z.string().trim().max(300).optional(),
  content: z.string().trim().min(1).max(32_000),
  enabled: z.boolean(),
})

// One skill's history (EP-11 S2), paged as the server pages it. Who may read it is the server's
// rule for reading the skill; nothing here names an org or a user.
const skillsVersionsListInput = z.object({
  skillId: z.string().min(1).max(200),
  limit: z.number().int().min(1).max(100).optional(),
  beforeVersion: z.number().int().min(0).optional(),
})

const skillsVersionsListOutput = z.object({
  versions: z.array(
    z.object({
      version: z.number().int().min(1),
      digest: z.string().regex(/^[0-9a-f]{64}$/),
      name: z.string(),
      description: z.string(),
      content: z.string(),
      trigger_keywords: z.array(z.string()),
      trigger_file_patterns: z.array(z.string()),
      tool_restrictions: z.array(z.string()),
      bundle_sha256: z.string().optional(),
      actor_kind: z.string(),
      actor_id: z.string().optional(),
      created_at: z.string(),
    }),
  ),
  has_more: z.boolean(),
})

// A skill's draft (EP-11 S5): saved over the live text without changing it, then published as a
// new version, discarded, or filled from an old version. Owner-or-admin is the server's rule; nothing
// here names an org or a user.
const skillsDraftSaveInput = z.object({
  skillId: z.string().min(1).max(200),
  description: z.string().trim().max(300).optional(),
  content: z.string().trim().min(1).max(32_000).optional(),
})

const skillIdInput = z.object({
  skillId: z.string().min(1).max(200),
})

const skillsPublishOutput = z.object({
  id: z.string(),
  version: z.number().int().min(0),
  published: z.literal(true),
})

const skillsVersionRestoreInput = z.object({
  skillId: z.string().min(1).max(200),
  version: z.number().int().min(1),
})

const skillsVersionRestoreOutput = z.object({
  id: z.string(),
  draft: z.literal(true),
  restored_from: z.number().int().min(1),
  bundle_restored: z.boolean(),
})

// Import a skill from a file (EP-11 S8). The file itself travels to the server; the contract names
// what the person picked and bounds it by the server's bundle cap (8 MiB and zip headers).
const skillsImportInput = z
  .object({
    fileName: z.string().trim().min(1).max(255),
    sizeBytes: z.number().int().min(1).max(8 * 1024 * 1024 + 256 * 1024),
    scope: z.enum(['user', 'org']).optional(),
  })
  .strict()

const skillsImportOutput = z.object({
  id: z.string(),
  draft: z.literal(true),
  enabled: z.literal(false),
})

// Authored templates and presets (EP-11 S6): rows of the skills registry with a kind. A template
// is text the composer inserts before the person sends, with `{{input}}` as its only placeholder;
// a preset is a closed set of visible controls. The server lints both and binds the owner to the
// session; nothing here names an org or a user.
const authoredTemplateInput = z.object({
  name: z.string().trim().min(1).max(64),
  description: z.string().trim().max(300).optional(),
  content: z.string().trim().min(1).max(32_000),
})

const authoredPresetSettings = z
  .object({
    modeId: z.string().min(1).max(128).optional(),
    browseWeb: z.boolean().optional(),
    planMode: z.literal(true).optional(),
    skillIds: z.array(z.string().min(1).max(200)).max(4).optional(),
    templateId: z.string().min(1).max(200).optional(),
  })
  .strict()

const authoredPresetInput = z.object({
  name: z.string().trim().min(1).max(64),
  description: z.string().trim().max(300).optional(),
  content: z.string().trim().min(1).max(32_000),
  settings: authoredPresetSettings,
})

const authoredUpdateInput = z.object({
  id: z.string().min(1).max(200),
  description: z.string().trim().max(300).optional(),
  content: z.string().trim().min(1).max(32_000).optional(),
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
  // Contract v2: 'auto' means follow the locale (stored, but not an explicit choice).
  dateFormat: z.enum(['MM/DD/YYYY', 'DD/MM/YYYY', 'YYYY-MM-DD', 'auto']).optional(),
  timeFormat: z.enum(['12h', '24h', 'auto']).optional(),
  compactMode: z.boolean().optional(),
  notifications: z.record(z.string(), z.boolean()).optional(),
  crawlIngestMode: z.enum(['auto', 'never', 'prompt']).optional(),
})

const settingsUpdateSettingInput = z.object({
  key: z.string().trim().min(1),
  value: z.unknown(),
})

// The caller's own reply tone (EP-8-S3), the closed set user-core stores. `default` asks for nothing extra.
const settingsSetTonePresetInput = z.object({
  tonePreset: z.enum(['default', 'professional', 'friendly', 'direct', 'warm', 'neutral']),
})

// The answers a person gives once, on first use (EP-8-S8). Every field is optional and bounded; user-core
// checks the time zone and the language again before it stores them. Whose settings these are never comes
// from the input: it is the verified caller.
const settingsCompletePersonalSetupInput = z.object({
  displayName: z.string().trim().max(80).optional(),
  timezone: z.string().trim().max(64).optional(),
  language: z.enum(['nb-NO', 'en-US']).optional(),
  tonePreset: z.enum(['default', 'professional', 'friendly', 'direct', 'warm', 'neutral']).optional(),
})

// Skipping takes no input: it records only that the person chose not to answer now.
const settingsSkipPersonalSetupInput = z.object({})

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

// The probe of an MCP server before it is saved (`POST /api/v1/mcp/servers/probe`, W4-CC-2). It takes the address
// alone, so a credential has no field to travel in, and it answers the server's auth requirement and, when the server
// needs no credential, its tools with the server's own hints. It registers nothing and is not a connect: connecting is
// still the one entry point above. Served to the settings page only; the model is never offered it.
const mcpProbeServerInput = z
  .object({
    url: z.string().trim().min(1).max(2048),
  })
  .strict()

const mcpProbeServerOutput = z.object({
  auth: z.enum(['none', 'oauth', 'bearer_required']),
  tools: z.array(
    z.object({
      name: z.string(),
      description: z.string(),
      read_only_hint: z.boolean().nullable(),
      destructive_hint: z.boolean().nullable(),
    }),
  ),
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

// The gateway accepts only the fixed cadences; Quarry derives the cron from them.
const monitoringCreateMonitorInput = z.object({
  url: z.string().url(),
  preset: z.enum(['hourly', 'daily', 'weekly']).optional(),
})

const monitoringDeleteMonitorInput = z.object({
  monitorId: z.string().min(1),
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
    // Extract only: a JSON Schema object (or the same document as text).
    schema: z.union([z.record(z.string(), z.unknown()), z.string()]).optional(),
    // Extract only: instructions for the model (1 to 10,000 characters after trimming).
    prompt: z.string().trim().max(10_000).optional(),
    // Batch only: re-fetch each URL and persist its page to Knowledge. Absent, the pages are not persisted.
    ingest: z.boolean().optional(),
  })
  .refine((value) => (value.kind === 'batch' ? (value.urls?.length ?? 0) > 0 : !!value.url), {
    message: "kind 'batch' requires a non-empty 'urls' array; any other kind requires 'url'",
  })
  .refine((value) => value.ingest !== true || value.kind === 'batch', {
    message: "'ingest' is only accepted for kind 'batch'",
  })

const ingestionsJobControlInput = z.object({
  jobId: z.string().min(1),
  action: z.enum(['pause', 'resume', 'cancel']),
})

const ingestionsStartAgentRunInput = z.object({
  url: z.string().url(),
  goal: z.string().trim().min(1).max(2000),
  maxSteps: z.number().int().min(1).max(100).optional(),
  allowedDomains: z.array(z.string().min(1)).max(50).optional(),
})

// WS15.L5: a schedule is created paused-by-default upstream and activated by a
// separate step, so this input has no 'activate' member and the client never
// sends one. 'batch' schedules carry 'urls' (the owner rejects a batch with no
// valid URL); every other kind carries 'targetUrl'.
const ingestionsCreateScheduleInput = z
  .object({
    name: z.string().trim().min(1),
    kind: z.string().min(1),
    targetUrl: z.string().url().optional(),
    urls: z.array(z.string().url()).min(1).optional(),
    maxPages: z.number().int().positive().optional(),
    overlapPolicy: z.enum(['skip', 'cancel', 'allow']).optional(),
    cron: z.string().trim().optional(),
    scheduleAt: z.string().datetime().optional(),
  })
  .strict()
  .refine((value) => !!value.cron || !!value.scheduleAt, {
    message: "Provide either 'cron' or 'scheduleAt'",
  })
  .refine((value) => (value.kind === 'batch' ? (value.urls?.length ?? 0) > 0 : !!value.targetUrl), {
    message: "kind 'batch' requires a non-empty 'urls' array; any other kind requires 'targetUrl'",
  })

// Pausing a schedule is reversible and loses nothing, so it needs no approval.
const ingestionsPauseScheduleInput = z.object({
  scheduleId: z.string().min(1),
})

// Every other schedule state change goes through one approval-gated action.
// The 'action' values are the wire verbs POST /api/ingestions/actions already
// takes, so the gateway split is routing only.
const ingestionsChangeScheduleStateInput = z
  .object({
    action: z.enum(['unpause_schedule', 'trigger_schedule', 'backfill_schedule', 'delete_schedule']),
    scheduleId: z.string().min(1),
    startAt: z.string().datetime().optional(),
    endAt: z.string().datetime().optional(),
  })
  .refine((value) => value.action !== 'backfill_schedule' || (!!value.startAt && !!value.endAt), {
    message: "backfill_schedule requires 'startAt' and 'endAt'",
  })

// Run control (WS15.L5). 'kind' picks the Quarry job family; crawl is the only one the
// gateway proxies today, so it is also the default when 'kind' is omitted.
const ingestionsRunControlKind = z.enum(['crawl'])

const ingestionsRunRefInput = z.object({
  runId: z.string().trim().min(1),
  kind: ingestionsRunControlKind.optional(),
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

// How the agent may use the person's connected apps (EP-10 S7). The levels only tighten: a read is used
// without asking, asked about first, or off; a write is always asked about, or off. The client maps a
// level onto the person's own tool policy on cap.tool.provider.read or cap.tool.provider.execute and
// never names an org, a user or an allow.
export const CONNECTOR_READ_LEVELS = ['use_without_asking', 'ask_first', 'off'] as const
export const CONNECTOR_WRITE_LEVELS = ['always_ask', 'off'] as const

export const integrationsSetConnectorPolicyInput = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('read'), level: z.enum(CONNECTOR_READ_LEVELS) }).strict(),
  z.object({ kind: z.literal('write'), level: z.enum(CONNECTOR_WRITE_LEVELS) }).strict(),
])

export const integrationsConnectorPoliciesOutput = z.object({
  // False while the deployment does not enforce tool policies: the levels are stored and not yet honoured.
  enforced: z.boolean(),
  read: z.enum(CONNECTOR_READ_LEVELS),
  write: z.enum(CONNECTOR_WRITE_LEVELS),
  // The org's own level, which wins whenever it is the stricter one.
  orgRead: z.enum(CONNECTOR_READ_LEVELS),
  orgWrite: z.enum(CONNECTOR_WRITE_LEVELS),
})

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

// The context chips one chat turn carries (EP-3 S5), sent as `context_refs`. The bounds are the gateway's own
// (mp-contracts `context_refs`): at most 20 references, ids of 1-128 characters from [A-Za-z0-9_.:-], labels
// of at most 120 characters, no repeated kind and id. A selection is EP-3 S6's and is not a chip, and a
// conversation joins the list with the thread tools (EP-3 S7), which the gateway refuses until then.
const chatContextRefInput = z
  .object({
    kind: z.enum(['document', 'space', 'skill']),
    id: z.string().regex(/^[A-Za-z0-9_.:-]{1,128}$/),
    label: z.string().max(120),
  })
  .strict()

const chatSetContextRefsInput = z
  .object({
    contextRefs: z
      .array(chatContextRefInput)
      .max(20)
      .refine((refs) => new Set(refs.map((ref) => `${ref.kind}:${ref.id}`)).size === refs.length, {
        message: 'context references must be distinct',
      }),
  })
  .strict()

// Opening the context picker reads what the person can already open: one tab, and an optional filter.
const chatPickContextRefInput = z
  .object({
    tab: z.enum(['documents', 'skills', 'space']),
    query: z.string().max(200).optional(),
  })
  .strict()

// The agent mode one chat turn runs under (WS25-S6): the four ids of `api/contracts/agent-modes.json`,
// read by the gateway as `agent_mode`. A closed list, so an id outside it is refused.
const chatSetAgentModeInput = z
  .object({
    mode: z.enum(['lockdown', 'read_only', 'ask_first', 'auto']),
  })
  .strict()

// The transcript level is the person's own display choice (EP-14 S2): the four levels user-core stores, and
// nothing else a caller could widen the view with.
const chatSetTranscriptDensityInput = z
  .object({
    transcriptDensity: z.enum(['detailed', 'standard', 'compact', 'minimal']),
  })
  .strict()

// One model call of the person's own run (EP-14 S7). The run id has the shape model-gateway accepts and the call
// number starts at 1; no user or org field exists, because the server reads the owner from the session.
const chatInspectRunRequestInput = z
  .object({
    runId: z.string().regex(/^[A-Za-z0-9_-]{1,64}$/),
    callSeq: z.number().int().min(1),
  })
  .strict()

// One cited source's passage (EP-2-S4): the citation it opens and, for a knowledge source, the chunk the gateway
// located. Ids only; no org or user field, because Data Plane reads the viewer's grants from the session.
const chatOpenCitedPassageInput = z
  .object({
    citationId: z.string().min(1).max(512),
    documentId: z.string().min(1).max(512).optional(),
    chunkIndex: z.number().int().min(0).optional(),
  })
  .strict()

// One page of an artifact's stored revisions (EP-3 S3): the thread and artifact it belongs to and an
// optional page position. No user or org field: Session Core reads the owner from the session.
const chatListArtifactRevisionsInput = z
  .object({
    threadId: z.string().min(1).max(128),
    artifactId: z.string().min(1).max(128),
    beforeVersion: z.number().int().min(1).optional(),
    limit: z.number().int().min(1).max(20).optional(),
  })
  .strict()

// A wide-research start (EP-2 S8) names a count preset or the person's own items. The server clamps
// the count to the org's item cap, so nothing here can widen a run.
const chatWideResearchStartInput = z
  .object({
    itemCap: z.number().int().min(1).max(16).optional(),
    items: z.array(z.string().min(1).max(500)).max(64).optional(),
  })
  .strict()

// The Start of a deep-research plan card (EP-2 S5): the edited sub-queries and the answers to the
// plan's questions, sent back as the run turn. The bounds are the card's; model-gateway re-clamps
// every entry to its own cap and length, so nothing here can widen a run.
const chatResearchPlanStartInput = z
  .object({
    subQueries: z.array(z.string().trim().min(1).max(200)).max(10),
    answers: z
      .array(
        z
          .object({
            question: z.string().max(300),
            answer: z.string().max(1000),
          })
          .strict(),
      )
      .max(3)
      .optional(),
  })
  .strict()

// Dismissing a plan card sends nothing: no research state waits on the server between the turns.
const chatResearchPlanCancelInput = z.object({}).strict()

// Accepting a connected-app suggestion card (EP-10 S8): `connect` starts the provider's connect session,
// `use` selects one existing connection for the question and sends it again. The providers are mp-contracts
// `connector_suggest`'s closed list, and a connection id has the bounds of `chat.set_connector_scope`.
const connectorSuggestionProvider = z.enum(['google', 'microsoft'])
const connectorSuggestionId = z.string().min(1).max(64).regex(/^[A-Za-z0-9_-]+$/)
const chatAcceptConnectorSuggestionInput = z.discriminatedUnion('path', [
  z
    .object({ path: z.literal('connect'), suggestionId: connectorSuggestionId, provider: connectorSuggestionProvider })
    .strict(),
  z
    .object({
      path: z.literal('use'),
      suggestionId: connectorSuggestionId,
      provider: connectorSuggestionProvider,
      connectionId: z.string().regex(/^[A-Za-z0-9_-]{1,128}$/),
    })
    .strict(),
])

// The same bounds model-gateway applies before it forwards a retry (EP-2 S9): printable ASCII ids,
// a child run id of at most 128 characters and a batch id of at most 256.
const chatWideResearchRetryItemInput = z
  .object({
    childRunId: z.string().min(1).max(128).regex(/^[\x21-\x7e]+$/),
    batchId: z.string().min(1).max(256).regex(/^[\x21-\x7e]+$/),
  })
  .strict()

// The template picker's read (EP-5 S7): the org comes from the session, so it takes no input at all.
const chatListDeckTemplatesInput = z.object({}).strict()

// A template pick (EP-5 S7) names one listed template by its skill id, in the shape model-gateway
// accepts; the server checks it against the org's templates again, so nothing here can name another
// org's bundle. `threadId` is the deck's thread, absent before the first turn.
const chatApplyDeckTemplateInput = z
  .object({
    skillId: z.string().min(1).max(128).regex(/^[A-Za-z0-9_.:-]+$/),
    threadId: z.string().min(1).max(128).optional(),
  })
  .strict()

// The slide a person opens in a deck's strip (EP-5 S4): the artifact the panel holds and the slide's id in its
// manifest. Strict, so nothing else reaches the strip; the slide itself is read from the manifest the panel has.
const chatOpenDeckSlideInput = z
  .object({
    artifactId: z.string().min(1).max(128),
    slideId: z.string().min(1).max(64).regex(/^[A-Za-z0-9_-]+$/),
  })
  .strict()

// The deck a person presents full screen (EP-5 S5): the artifact the panel holds. Strict, so nothing else reaches the
// presentation; its slides, its pages and the present gate are read from the manifest the panel has.
const chatPresentDeckInput = z
  .object({
    artifactId: z.string().min(1).max(128),
  })
  .strict()

// The person's own edit or restore of a thread artifact (EP-3 S4). `expectedVersion` is the head the panel
// showed, the server's compare-and-set; no org or user field, because the server reads the owner from the
// session. The text bound is the gateway's text-artifact limit.
const chatSaveArtifactRevisionInput = z
  .object({
    threadId: z.string().min(1).max(128),
    artifactId: z.string().min(1).max(256),
    content: z.string().min(1).max(200_000),
    title: z.string().max(256).optional(),
    expectedVersion: z.number().int().min(1),
  })
  .strict()

const chatRestoreArtifactRevisionInput = z
  .object({
    threadId: z.string().min(1).max(128),
    artifactId: z.string().min(1).max(256),
    version: z.number().int().min(1),
    expectedVersion: z.number().int().min(1),
  })
  .strict()

// A selection edit names a range of one revision by character offsets and the hash of its text, never the text itself
// (EP-3 S6). The gateway checks the version is still the head and the range still holds that text.
const chatEditArtifactSelectionInput = z
  .object({
    artifactId: z.string().regex(/^[A-Za-z0-9_.:-]{1,128}$/),
    version: z.number().int().min(1),
    start: z.number().int().min(0),
    end: z.number().int().min(1),
    quoteSha256: z.string().regex(/^[0-9a-f]{64}$/),
    instruction: z.string().min(1).max(20_000),
  })
  .strict()

// Save one artifact revision to knowledge (EP-3 S8): the text on screen becomes a Data Plane document owned
// by the person. The ids name the source revision for the document's trace only; the BFF bounds them to 256
// characters and copies nothing from the thread. No org or user field: documents-api reads the owner from
// the person's own scoped token.
const chatSaveArtifactToKnowledgeInput = z
  .object({
    threadId: z.string().min(1).max(256),
    artifactId: z.string().min(1).max(256),
    version: z.number().int().min(1),
    title: z.string().trim().min(1).max(256),
    content: z.string().trim().min(1).max(200_000),
  })
  .strict()

// A thread goal (EP-7 S1): the person's standing objective on one of their own threads, recited to the
// model as data on every Do turn. The bounds are session-core's (D11): an objective of 1 to 2,000
// characters, at most 8 criteria of 500, a turn cap of 1 to 50. No org or user field: the server reads
// the owner from the session.
const threadGoalCriteria = z.array(z.string().trim().min(1).max(500)).max(8)

const chatSetGoalInput = z
  .object({
    threadId: z.string().min(1).max(128),
    objective: z.string().trim().min(1).max(2000),
    successCriteria: threadGoalCriteria.optional(),
    maxTurns: z.number().int().min(1).max(50).optional(),
    replace: z.boolean().optional(),
  })
  .strict()

// `expectedRevision` is the revision the goal bar showed, the server's compare-and-set. The status a
// person may write is `active` or `paused` here; `met` is marked only by `chat.goal_mark_done` (EP-7 S3), and the
// stop states come only from the server.
const chatUpdateGoalInput = z
  .object({
    threadId: z.string().min(1).max(128),
    goalId: z.string().min(1).max(128),
    expectedRevision: z.number().int().min(1),
    objective: z.string().trim().min(1).max(2000).optional(),
    successCriteria: threadGoalCriteria.optional(),
    maxTurns: z.number().int().min(1).max(50).optional(),
    status: z.enum(['active', 'paused']).optional(),
  })
  .strict()

const chatClearGoalInput = z
  .object({
    threadId: z.string().min(1).max(128),
    goalId: z.string().min(1).max(128),
    expectedRevision: z.number().int().min(1),
  })
  .strict()

const chatContinueGoalInput = z
  .object({
    threadId: z.string().min(1).max(128),
    goalId: z.string().min(1).max(128),
  })
  .strict()

// "Keep going" on the goal bar (EP-7 S4): the person's own switch for the goal pump on one thread. It
// is held in the open tab only and grants nothing: each turn the pump sends is claimed by the server,
// which refuses while a decision waits on the person.
const chatGoalAutoToggleInput = z
  .object({
    threadId: z.string().min(1).max(128),
    goalId: z.string().min(1).max(128),
    on: z.boolean(),
  })
  .strict()

// The person's own override of the judge (EP-7 S3): mark the open goal done, or reopen a met or blocked one. Only
// the goal, its thread and the revision the goal bar showed; the server decides which status change is allowed.
const chatGoalMarkDoneInput = z
  .object({
    threadId: z.string().min(1).max(128),
    goalId: z.string().min(1).max(128),
    expectedRevision: z.number().int().min(1),
  })
  .strict()

const chatGoalReopenInput = z
  .object({
    threadId: z.string().min(1).max(128),
    goalId: z.string().min(1).max(128),
    expectedRevision: z.number().int().min(1),
  })
  .strict()

// "Keep working while away" (EP-7 S6): the person's own start of a goal-bound schedule. The schedule is the stored
// consent, so the form names the read tools it may use, the most runs, how often it fires (the four intervals the
// goal bar offers) and for how many days. No org or user field: the server reads the owner from the session.
const chatGoalAwayStartInput = z
  .object({
    threadId: z.string().min(1).max(128),
    goalId: z.string().min(1).max(128),
    tools: z.array(z.string().trim().min(1).max(128)).min(1).max(32),
    maxRuns: z.number().int().min(1).max(20),
    everyMinutes: z.union([z.literal(30), z.literal(60), z.literal(240), z.literal(1440)]),
    days: z.number().int().min(1).max(90),
  })
  .strict()

// A follow-up an inline widget proposed (EP-4 S3), staged as a chip the person clicks. Only the text: the widget,
// the answer and the thread are the host's, and the bridge's own limit is 500 characters.
const chatStageWidgetFollowupInput = z
  .object({
    text: z.string().min(1).max(500),
  })
  .strict()

// A passage an inline widget selected (EP-4 W4-IO-2), quoted as a chip the person clicks into the composer. Only the
// text: the widget, the answer and the thread are the host's, and the bridge's own limit is 2,000 characters.
const chatStageWidgetSelectionInput = z
  .object({
    text: z.string().min(1).max(2000),
  })
  .strict()

// A link an inline widget asked to open (EP-4 S3). Web and mail only, as `isSafeLinkHref` in the chat feature
// decides, and never longer than the bridge's 8 KiB message; the person opens it from the host's confirmation.
const chatOpenWidgetLinkInput = z
  .object({
    url: z
      .string()
      .max(8 * 1024)
      .regex(/^(?:https?:|mailto:)[^\u0000-\u001f\u007f]*$/i),
  })
  .strict()

// The app an MCP tool call in a chat answer declares (EP-4 S9). Named by the call's server and tool only; the
// gateway re-checks that the viewer may use the server and re-discovers which `ui://` document the tool names.
const chatOpenMcpAppInput = z
  .object({
    serverId: z.string().trim().min(1).max(256),
    tool: z.string().trim().min(1).max(256),
  })
  .strict()

// A message an MCP App staged (EP-4 S9): the same bounds as a widget's follow-up, the person sends it.
const chatStageMcpAppMessageInput = z
  .object({
    text: z.string().min(1).max(500),
  })
  .strict()

// A tool an MCP App calls on its own server (EP-4 S10). The run and server are the frame's and the nonce
// is the frame's own; the org and the viewer are the session's, so none of them is an input here.
const chatMcpAppToolCallInput = z
  .object({
    runId: z.string().trim().min(1).max(256),
    serverId: z.string().trim().min(1).max(256).refine((value) => !value.includes('__')),
    tool: z.string().trim().min(1).max(256),
    arguments: z.record(z.string(), z.unknown()),
    frameNonce: z.string().regex(/^[A-Za-z0-9_-]{16,128}$/),
  })
  .strict()

// A tool call the person opens in the computer panel (P1). Only the thread and the call's id: the viewer is picked
// from the call's own intent, never named by the caller, and the call is read from the transcript the panel holds.
const chatOpenComputerViewerInput = z
  .object({
    threadId: z.string().trim().min(1).max(128),
    callId: z.string().trim().min(1).max(256),
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

// Retention window and legal holds (EP-9 C1). org-core owns every rule; these mirror its bounds so a
// draft is refused before it is sent. A null window means keep, the default.
const orgSetRetentionInput = z.object({
  orgId: z.string().min(1),
  expected_revision: z.number().int().nonnegative(),
  thread_days: z.number().int().min(30).max(3650).nullable(),
}).strict()

const orgPlaceLegalHoldInput = z.discriminatedUnion('scope', [
  z.object({
    orgId: z.string().min(1),
    scope: z.literal('org'),
    hold_ref: z.string().trim().min(1).max(200),
  }).strict(),
  z.object({
    orgId: z.string().min(1),
    scope: z.literal('thread'),
    thread_id: z.string().min(1).max(128).regex(/^[A-Za-z0-9][A-Za-z0-9._:-]*$/),
    hold_ref: z.string().trim().min(1).max(200),
  }).strict(),
])

const orgReleaseLegalHoldInput = z.object({
  orgId: z.string().min(1),
  holdId: z.string().regex(/^lh_[0-9a-f]{32}$/),
}).strict()

// Domain verification (WS05-S6). The domain is a hostname; org-core normalizes it, refuses IP
// literals, public suffixes and free-mail providers, and owns every other rule.
const orgDomainInput = z.object({
  orgId: z.string().min(1),
  domain: z.string().trim().min(1).max(253),
})

// SSO policy and providers (WS05-S7). org-core owns the policy, the verified-domain precondition
// and the enforce checks; the provider's client secret is write-only and never echoed back.
const orgUpdateSsoPolicyInput = z.object({
  orgId: z.string().min(1),
  mode: z.enum(['off', 'report', 'enforce']),
  breakGlassUserIds: z.array(z.string().trim().min(1).max(128)).max(2),
  expectedRevision: z.number().int().min(0),
})

const orgAddSsoProviderInput = z.object({
  orgId: z.string().min(1),
  providerId: z.string().trim().min(3).max(64),
  domain: z.string().trim().min(1).max(253),
  issuer: z.string().trim().url().max(2048),
  clientId: z.string().trim().min(1).max(512),
  clientSecret: z.string().trim().min(1).max(512),
})

const orgRemoveSsoProviderInput = z.object({
  orgId: z.string().min(1),
  providerId: z.string().trim().min(3).max(64),
})

// SCIM tokens (WS05-S8). org-core mints the bearer token and shows it once; revoking is immediate.
const orgCreateScimTokenInput = z.object({
  orgId: z.string().min(1),
  name: z.string().trim().min(1).max(80),
  expiresInDays: z.number().int().min(1).max(365),
})

const orgRevokeScimTokenInput = z.object({
  orgId: z.string().min(1),
  tokenId: z.string().trim().min(1).max(64),
})

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
        // F-t3-083: the organization's agents-paused switch, held by capability-core under `agents.paused`.
        'agents.paused': z.boolean().optional(),
        'ext.mcp_servers': orgAgentPolicyAllowListInput.optional(),
        mcp_tool_approval: z.enum(['approved_only', 'trust_annotations']).optional(),
      })
      .strict(),
  })
  .strict()

// The organization's web switches (Wave 3 EP-9 W2): public web access for its agents, and whether a paid search
// provider may serve them. Both are written together. `cached_only` is reserved on the server and so is not offered.
const orgSetWebPolicyInput = z
  .object({
    revision: z.number().int().min(0),
    webMode: z.enum(['off', 'live']),
    paidProviders: z.enum(['off', 'on']),
  })
  .strict()

const orgPreviewEffectivePolicyInput = z.object({}).strict()

// Settings > Browser (E3.2-g1): the organization browser policy, org-core's web policy (WS7.H).
// The gateway only proxies it. A save is the whole document at the revision it was read at
// (ifMatch, which org-core checks as If-Match); a partial set would loosen the policy by omission.
const browserPolicyDocumentInput = z
  .object({
    caps: z.record(z.string().min(1).max(64), z.string().min(1).max(32)).optional(),
    robots_max: z.string().min(1).max(32).optional(),
    proxy_max: z.string().min(1).max(32).optional(),
    domains: z
      .object({
        allow: z.array(z.string().min(1).max(253)).max(64).optional(),
        deny: z.array(z.string().min(1).max(253)).max(64).optional(),
      })
      .strict()
      .optional(),
  })
  .strict()
const browserGetOrgPolicyInput = z.object({ orgId: z.string().min(1) }).strict()
const browserSetOrgPolicyInput = z
  .object({
    orgId: z.string().min(1),
    ifMatch: z.number().int().min(0),
    document: browserPolicyDocumentInput,
  })
  .strict()
const browserGetOrgPolicyOutput = z.object({
  policy: z.record(z.string(), z.unknown()).nullable(),
  effective: z.record(z.string(), z.unknown()),
  revision: z.number().int().min(0),
  rev: z.string(),
  entitlements: z.record(z.string(), z.boolean()),
  can_manage: z.boolean(),
  updated_by: z.string().nullable(),
  updated_at: z.string().nullable(),
})
const browserSetOrgPolicyOutput = z.object({
  revision: z.number().int().min(0),
  rev: z.string(),
})
// The effective policy of one thread (Wave 3 EP-9 T2): each value the gateway enforces for it, with the layer that set
// it and a lock where a higher layer fixes it. The organization is the session's, so the input names the thread only.
const policyReadEffectiveInput = z.object({ threadId: z.string().trim().min(1).max(200) }).strict()
const policyEffectiveItem = z.object({
  key: z.string(),
  value: z.unknown(),
  origin: z.enum(['platform', 'org', 'space', 'user', 'request']).nullable(),
  revision: z.number().int().nonnegative().optional(),
  locked: z.boolean(),
  unavailable: z.boolean().optional(),
})
// The items as the client reads them: requestJson unwraps the route's `{ data }` envelope.
const policyReadEffectiveOutput = z.object({ items: z.array(policyEffectiveItem) })

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

// The organization's permission rules (W5-074-c, F-t3-074): a deny or ask rule on one tool, for one target. A rule only
// tightens a call, so there is no allow. Inputs name no organization: the session supplies it. The target kinds are the
// closed set capability-core stores: a web domain, a mail recipient's domain (never the address), a command prefix, or
// any target of the tool. Inputs are camelCase like the rest of the registry; the REST bodies and the outputs are
// capability-core's wire form.
const orgPermissionRuleTargetKinds = ['domain', 'recipient', 'command_prefix', 'any'] as const

const orgCreatePermissionRuleInput = z
  .object({
    toolPattern: z
      .string()
      .min(1)
      .max(200)
      .regex(/^(\*|[A-Za-z0-9][A-Za-z0-9_.:-]{0,199}\*?)$/),
    targetKind: z.enum(orgPermissionRuleTargetKinds),
    targetPattern: z.string().min(1).max(200),
    decision: z.enum(['deny', 'ask']),
  })
  .strict()
  .refine((input) => input.targetKind !== 'any' || input.targetPattern === '*', {
    message: 'an any-target rule takes the pattern *',
    path: ['targetPattern'],
  })

const orgDeletePermissionRuleInput = z.object({ ruleId: z.string().min(1).max(200) }).strict()

const orgPermissionRuleRecord = z
  .object({
    id: z.string(),
    tool_pattern: z.string(),
    target_kind: z.enum(orgPermissionRuleTargetKinds),
    target_pattern: z.string(),
    decision: z.enum(['deny', 'ask']),
    created_by: z.string(),
    created_at: z.string(),
  })
  .passthrough()

const orgPermissionRuleListOutput = z
  .object({
    org_id: z.string(),
    revision: z.number().int().min(0),
    rules: z.array(orgPermissionRuleRecord),
  })
  .passthrough()

const orgPermissionRuleWriteOutput = orgPermissionRuleRecord
  .extend({ revision: z.number().int().min(0) })
  .passthrough()

const orgPermissionRuleDeleteOutput = z
  .object({
    id: z.string(),
    deleted: z.literal(true),
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

// The person's changes to a drafted mail send (EP-10-S6, decision D2): recipients, subject and the
// plain-text body, nothing else. At least one field; the limits are the server's (EP-10-S3), checked
// here only so a form never sends what the server will refuse.
const approvalEditPatch = z
  .object({
    to: z.array(z.string().trim().min(1)).optional(),
    subject: z.string().optional(),
    bodyText: z.string().optional(),
  })
  .strict()
  .superRefine((patch, ctx) => {
    if (patch.to === undefined && patch.subject === undefined && patch.bodyText === undefined) {
      ctx.addIssue({ code: 'custom', message: 'no_change' })
      return
    }
    const code = checkMailEditLimits(patch)
    if (code) ctx.addIssue({ code: 'custom', message: code })
  })

// "Send with my edits" (EP-10-S6, decision D1): the patch and the fingerprint of the draft the
// person saw. A draft that changed since is a 409 approval_changed, never a silent overwrite.
export const approvalEditInput = z
  .object({
    patch: approvalEditPatch,
    baseFingerprint: z.string().regex(/^[0-9a-fA-F]{64}$/),
  })
  .strict()

// The decision on a pending approval (build plan WS13 S2). The decider is the verified credential's,
// never an input, so a field naming one, a decision source or a grant is refused rather than ignored.
// `scope` is how far the decision reaches; only `once` exists until standing grants do. An `edit`
// rides an approval only (EP-10-S6): a rejection has nothing to send.
const approvalsDecideInput = z
  .object({
    approvalId: z.string().min(1),
    decision: z.enum(['approve', 'reject']),
    reason: z.string().max(500).optional(),
    scope: z.literal('once').optional(),
    edit: approvalEditInput.optional(),
  })
  .strict()
  .refine((input) => input.edit === undefined || input.decision === 'approve', {
    message: 'edit_requires_grant',
  })

// The full draft behind a pending write approval (EP-10-S1). Only the approval id is an input: the
// org and the owner are the verified credential's.
const approvalsPreviewInput = z
  .object({
    approvalId: z.string().min(1),
  })
  .strict()

// What the card shows. `mail_send` carries every recipient, the full subject and the full body as
// plain text; `other` names only the provider, operation and connection; `none` is an approval
// with no retained call (a browser takeover).
const approvalsPreviewOutput = z.object({
  kind: z.enum(['mail_send', 'other', 'none']),
  provider: z.string(),
  operation: z.string(),
  connectionId: z.string(),
  to: z.array(z.string()),
  subject: z.string(),
  bodyText: z.string(),
  hasHtml: z.boolean(),
  isReply: z.boolean(),
  rawOnly: z.boolean(),
  editableFields: z.array(z.string()),
  actionFingerprint: z.string(),
  revision: z.number().int().nonnegative(),
  // The gateway takes "Send with my edits" for this draft (EP-10-S6); server-controlled.
  canEdit: z.boolean().optional(),
})

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

// The export's answer is the file itself, which the browser saves: the contract names its media
// type and the name it carries, not a JSON envelope (EP-12 S11).
const threadExportOutput = z.object({
  contentType: z.string().min(1),
  filename: z.string().min(1),
})

// The new thread an import made (EP-12 S15): its id, how many turns it kept and how many things the file held that it left out.
const threadImportOutput = z.object({
  threadId: z.string().min(1),
  importedMessages: z.number().int().nonnegative(),
  dropped: z.number().int().nonnegative(),
})

// A snapshot as the owner sees it (W4-SH-6): the id a grant names, the thread it copies and when it was taken or revoked.
// The copy itself is read through the shared-snapshot route, never returned by these writes.
const threadSnapshotOutput = z.object({
  id: z.string().min(1),
  threadId: z.string().min(1),
  title: z.string(),
  createdAt: z.string(),
  revokedAt: z.string().nullable(),
})

// One member grant on a snapshot (W4-SH-6): the member and when Control recorded the grant.
const threadSnapshotGrantOutput = z.object({
  granteeUserId: z.string().min(1),
  grantedAt: z.string().nullable(),
})

// The library's answers (F-t3-080): one page of document metadata, never text, and one document's text when it is read.
const chatLibraryListOutput = z.object({
  documents: z.array(z.record(z.string(), z.unknown())),
  total: z.number().int().nonnegative(),
})

const chatLibraryReadTextOutput = z.object({
  content: z.string(),
})

// The review's answer (F-t3-086): one card under `data.run`. The card's fields are read where the client reads them
// (chat-client.ts), so the registry only checks that the card is there.
const reviewRunOutput = z.object({
  data: z.object({ run: z.record(z.string(), z.unknown()) }),
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

export const orchestrationDecideApprovalInput = z
  .object({
    approvalId: z.string().min(1),
    decision: z.enum(['approve', 'reject']),
    reason: z.string().trim().max(ORCHESTRATION_APPROVAL_REASON_MAX).optional(),
    // "Send with my edits" (EP-10-S6): approved for this one call only, never with a reject.
    edit: approvalEditInput.optional(),
  })
  .refine((input) => input.edit === undefined || input.decision === 'approve', {
    message: 'edit_requires_grant',
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
  // Refuse the run unless it is bound to this owned browser session.
  requireAttached: z.boolean().optional(),
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

// A person's steer into a live browser-agent run (E3.3-g): one message typed in the dock while the
// run works, which the run takes at its next step. `clientMessageId` names the message, so a repeated
// send of the same id is one message. The owner holds the text in memory until a step takes it, and
// never writes it to the run's event log or to the transcript.
const browserRunSteerInput = z.object({
  runId: z.string().min(1).max(128),
  content: z.string().trim().min(1).max(2000),
  clientMessageId: z
    .string()
    .regex(/^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/)
    .optional(),
}).strict()

// `queued`: the run has the message and takes it at its next step. `run_ended`: the run ended first, so
// nothing was taken and nothing was started, and the person decides what happens to the text.
export const browserRunSteerOutput = z.object({
  outcome: z.enum(['queued', 'run_ended']),
  clientMessageId: z.string().min(1).max(128).optional(),
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

// The downloads drawer (E3.4-7) sends one file the browser downloaded in a session. Both ids are
// opaque handles the session reported: the owner checks that the artifact belongs to this session
// and to the caller's org, and refuses an artifact it does not hold.
export const browserDownloadSendToKnowledgeInput = z.object({
  sessionId: z.string().trim().min(1).max(64),
  artifactId: z.string().trim().min(1).max(64),
}).strict()

// The Data Plane document the owner created and how far its indexing has got. Indexing runs after
// the write, so a fresh document reports `pending`.
export const browserDownloadSendToKnowledgeOutput = z.object({
  documentId: z.string().min(1),
  indexStatus: z.enum(['pending', 'indexed', 'failed', 'skipped']),
})

const routerPolicyModeTableInput = z.object({
  simple: z.string().min(1),
  moderate: z.string().min(1),
  complex: z.string().min(1),
})

// The side-task purposes the role table names (EP-13 S4); inference-core refuses any other key.
const routerPolicyRolePurposes = ['title', 'follow_ups', 'verification_judge', 'research_plan'] as const

const routerPolicyRoleInput = z.object({
  candidates: z.array(z.string().min(1)).min(1).max(4),
  effort: z.string().optional(),
})

// The platform catalogue overlay (F-t3-065 phase 1): the models the listing hides, the order it puts first and the alias
// ids that stand for a listed model. inference-core caps the lists at 200, 200 and 50 entries, and an alias id is one to
// 64 lower-case letters, digits, `.`, `_` or `-`; it checks the aliases against the catalogue on every write.
const routerPolicyCatalogueOverlayInput = z.object({
  hidden: z.array(z.string().min(1)).max(200),
  order: z.array(z.string().min(1)).max(200),
  aliases: z
    .record(z.string().regex(/^[a-z0-9._-]{1,64}$/), z.string().min(1))
    .refine((aliases) => Object.keys(aliases).length <= 50, 'at most 50 aliases'),
})

// The owner enforces no admin gate on this write today — only org-scoping.
// See dispatch_router_policy_update's own comment: the high risk/
// requires-approval posture here compensates for that at the registry level,
// since this is a full-document PUT of the org's entire model-routing table.
// Every field of the document is listed, the ones the page does not show included:
// the owner defaults a field it does not receive, so an unlisted field is reset by a save.
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
    tool_use_floors_complex: z.boolean(),
  }),
  table: z.object({
    budget: routerPolicyModeTableInput,
    balance: routerPolicyModeTableInput,
    genius: routerPolicyModeTableInput,
  }),
  tool_fallback_ladder: z.array(z.string().min(1)).max(8),
  roles: z.partialRecord(z.enum(routerPolicyRolePurposes), routerPolicyRoleInput).optional(),
  catalogue_overlay: routerPolicyCatalogueOverlayInput.optional(),
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

// Audit list v2 (WS05-S4): the paged admin view, no export (export is audit.export_log).
const auditListEventsInput = z.object({
  event: z.string().trim().max(128).optional(),
  userId: z.string().trim().max(128).optional(),
  plane: z.string().trim().max(128).optional(),
  outcome: z.string().trim().max(128).optional(),
  subject: z.string().trim().max(256).optional(),
  resourceId: z.string().trim().max(256).optional(),
  since: z.string().datetime().optional(),
  until: z.string().datetime().optional(),
  limit: z.number().int().min(1).max(200).optional(),
  cursor: z.string().trim().max(512).optional(),
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

// W4-BYO-6: an organisation's own OpenAI-compatible model gateway. The key is sent once, sealed by
// Integration Core and never read back. A customer gateway is never zero data retention.
const modelsCreateCustomerGatewayInput = z
  .object({
    name: z.string().trim().min(1).max(120),
    base_url: z.string().trim().min(1).max(2048),
    api_key: z.string().min(1).max(4096),
    model_ids: z.array(z.string().trim().min(1).max(128)).min(1).max(20),
  })
  .strict()

const modelsDeleteCustomerGatewayInput = z
  .object({
    connection_id: z.string().trim().min(1).max(128),
  })
  .strict()

// W4-CC-8: an organisation's stored connector keys and its custom REST connector. The key is sent once and never read
// back; Integration Core seals it. Strict, so a field the form never sends (an organisation, a host override) is refused.
const connectorsCreateCredentialBindingInput = z
  .object({
    kind: z.enum(['mcp_bearer', 'rest_api_key', 'rest_bearer']),
    host: z.string().trim().min(1).max(253),
    header_name: z.enum(['Authorization', 'X-Api-Key', 'Api-Key', 'X-Auth-Token']),
    allowed_methods: z.array(z.enum(['GET', 'HEAD', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS'])).min(1).max(7),
    allowed_path_prefixes: z.array(z.string().min(1).max(256)).min(1).max(32),
    secret: z.string().min(1).max(4096),
  })
  .strict()

const connectorsRevokeCredentialBindingInput = z
  .object({
    binding_id: z.string().trim().min(1).max(128),
  })
  .strict()

const connectorsCreateCustomRestInput = z
  .object({
    name: z.string().trim().min(1).max(120),
    credential_binding_id: z.string().trim().min(1).max(128),
  })
  .strict()

const envelopeStatusOutput = z.object({
  status: z.string(),
})

// The automations hub's schedule calls (EP-6-S1). Strict, so a field the hub never sends (a goal,
// an approval, a creator) is refused here rather than dropped. Creator-or-admin is enforced by the
// BFF (authorize_own_schedule), not by these shapes.
const scheduleListInput = z.object({}).strict()

const scheduleIdInput = z
  .object({
    scheduleId: z.string().trim().min(1).max(200),
  })
  .strict()

const scheduleUpdateTimeInput = z
  .object({
    scheduleId: z.string().trim().min(1).max(200),
    scheduleExpr: z.string().trim().min(1).max(120),
  })
  .strict()

const scheduleFiresListInput = z
  .object({
    scheduleId: z.string().trim().min(1).max(200),
    limit: z.number().int().min(1).max(100).optional(),
  })
  .strict()

// Run now (EP-6-S2). The request id is generated once per press and reused on retry, so a retry
// finds the first fire; capability-core accepts 16 to 64 letters, digits or hyphens.
const scheduleRunNowInput = z
  .object({
    scheduleId: z.string().trim().min(1).max(200),
    requestId: z.string().regex(/^[A-Za-z0-9-]{16,64}$/),
  })
  .strict()

// Event triggers (Wave 4 EP-7, F-t3-047, W4-T8): a webhook event on one connection may wake a run of the person's own
// schedule. The automations hub calls these through event-trigger-client.ts. Strict, camelCase and person-only: a model
// never creates, changes or dry runs a trigger (MODEL_ACCESS 'none'). Session Core checks the member, the connection,
// the Space, the schedule's approval and the closed wake filter, so these shapes only bound what the hub sends.
const eventTriggerWakeFilterInput = z
  .string()
  .max(4096)
  .refine((value) => {
    if (value.trim() === '') return true
    try {
      const parsed: unknown = JSON.parse(value)
      return parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed)
    } catch {
      return false
    }
  }, 'wakeFilterJson is empty or a JSON object')

const eventTriggerListInput = z
  .object({
    spaceRef: z.string().trim().min(1).max(128).optional(),
  })
  .strict()

const eventTriggerCreateInput = z
  .object({
    spaceRef: z.string().trim().min(1).max(128),
    connectionId: z.string().trim().min(1).max(128),
    eventType: z.literal('webhook_received'),
    name: z.string().trim().min(1).max(120),
    clockEnabled: z.boolean().optional(),
    scheduleId: z.string().trim().min(1).max(128).optional(),
    wakeFilterJson: eventTriggerWakeFilterInput.optional(),
  })
  .strict()

const eventTriggerUpdateInput = z
  .object({
    triggerId: z.string().trim().min(1).max(128),
    expectedRevision: z.number().int().min(1),
    updatePaths: z.array(z.enum(['name', 'clock_enabled', 'status', 'schedule_id', 'wake_filter_json'])).min(1).max(5),
    name: z.string().trim().min(1).max(120).optional(),
    clockEnabled: z.boolean().optional(),
    status: z.enum(['active', 'paused', 'disabled']).optional(),
    scheduleId: z.string().trim().max(128).optional(),
    wakeFilterJson: eventTriggerWakeFilterInput.optional(),
  })
  .strict()

const eventTriggerDryRunInput = z
  .object({
    triggerId: z.string().trim().min(1).max(128),
    webhookEventId: z.string().trim().min(1).max(128),
  })
  .strict()

const eventTriggerRecordOutput = z.object({
  id: z.string(),
  spaceRef: z.string(),
  connectionId: z.string(),
  eventType: z.string(),
  name: z.string(),
  clockEnabled: z.boolean(),
  status: z.string(),
  revision: z.number(),
  scheduleId: z.string().nullable(),
  wakeFilterJson: z.string().nullable(),
  createdAt: z.string(),
  updatedAt: z.string(),
})

const eventTriggerListOutput = z.object({
  triggers: z.array(eventTriggerRecordOutput),
})

const eventTriggerEnvelopeOutput = z.object({
  trigger: eventTriggerRecordOutput,
})

const eventTriggerDryRunOutput = z.object({
  wouldWake: z.boolean(),
  reason: z.string(),
  matched: z.array(z.string()),
})

// The agents landing's read of the org's definitions and the rooms each is installed in (EP-6-S5).
// Strict and empty: the organisation comes from the session, never from the caller.
const agentInstallationsListInput = z.object({}).strict()

const agentInstallationsListOutput = z.object({
  definitions: z.array(
    z.object({
      agent_ref: z.string(),
      name: z.string().optional(),
      description: z.string().optional(),
      definition_status: z.enum(['active', 'inactive', 'draft']).optional(),
      installations: z.array(
        z.object({
          space_ref: z.string(),
          space_name: z.string(),
          space_kind: z.enum(['personal', 'room', 'project', 'case']),
          status: z.enum(['pending', 'active', 'paused', 'revoked', 'failed']).optional(),
        }),
      ),
    }),
  ),
})

// The agent builder's calls (EP-6-S8, AGENT_DEFINITION_VERSIONS). Strict, so a field the builder
// never sends (a version, a digest, a publisher) is refused here rather than dropped. Who may edit
// (org admin, the creator, or a manager of the room a room-born agent came from) is enforced by the
// BFF (authorize_agent_builder), not by these shapes.
const agentRefInput = z
  .object({
    agentRef: z.string().trim().min(1).max(128),
  })
  .strict()

const agentAutonomyCap = z.enum(['read_only', 'ask_first', 'auto'])

const agentDraftBody = z
  .object({
    name: z.string().trim().min(2).max(60),
    system_prompt: z.string().max(4000),
    model: z.string().trim().min(1).max(200),
    // Capability ids from the WS05 registry, never free tool names.
    tool_capability_ids: z
      .array(z.string().regex(/^[a-z][a-z0-9_.-]{0,127}$/))
      .max(64),
    autonomy_cap: agentAutonomyCap,
    knowledge_bindings: z
      .object({
        scope: z.enum(['all', 'selected']),
        document_ids: z.array(z.string()).optional(),
        source_ids: z.array(z.string()).optional(),
        include_qna: z.boolean().optional(),
      })
      .strict()
      .optional(),
  })
  .strict()

const agentDraftSaveInput = z
  .object({
    agentRef: z.string().trim().min(1).max(128),
    draft: agentDraftBody,
  })
  .strict()

const agentDraftSaveOutput = z.object({
  draft: agentDraftBody,
})

const agentPublishOutput = z.object({
  version: z.number().int().min(1),
  digest: z.string().regex(/^sha256:[0-9a-f]{64}$/),
})

const agentVersionsListOutput = z.object({
  versions: z.array(
    z.object({
      version: z.number().int().min(1),
      digest: z.string(),
      name: z.string(),
      system_prompt: z.string(),
      model: z.string(),
      tool_capability_ids: z.array(z.string()),
      autonomy_cap: agentAutonomyCap,
      published_by: z.string(),
      published_at: z.number(),
    }),
  ),
})

// The builder's preview (EP-6-S8): one question answered by an agent's saved draft in a temporary chat.
// Strict and bounded: it names the agent and the question, and nothing a thread, a Space or a mention
// would bind it to. The BFF enforces the same refusals and the builder's access check, and stores nothing.
const agentPreviewInput = z
  .object({
    agentRef: z.string().trim().min(1).max(128),
    content: z.string().trim().min(1).max(32_000),
  })
  .strict()

// The owner actions an org's run view can offer (EP-6 S11), read by the agent presets. Strict and
// empty: the organisation comes from the session. The answer is a mode and action ids, never a schema.
const agentAdmittedActionsListInput = z.object({}).strict()

const agentAdmittedActionsListOutput = z.object({
  mode: z.string(),
  action_ids: z.array(z.string()),
})

// EP-6 P3-c: the notes one bound agent kept in one Space (`GET /api/v1/spaces/{space_ref}/memory?agent_subject_id=`).
// The Space comes from the path, and Control decides who may read it (the room's owner or an admin). The notes
// are quoted context and grant the agent nothing. A Zero Data Retention caller is refused as `named_agent`.
const spaceAgentMemoryListInput = z.object({
  spaceRef: z.string().min(1),
  agentSubjectId: z.string().min(1).max(64),
})

const spaceAgentMemoryListOutput = z.object({
  memories: z.array(
    z.object({
      memory_id: z.string(),
      content: z.string(),
      updated_at: z.string(),
      provenance: z.string(),
      source_label: z.string(),
      thread_id: z.string(),
      withheld: z.boolean(),
    }),
  ),
  withheld: z.number().int().min(0),
})

// EP-6 P3-c: one note of one bound agent, removed for every member of the Space through the Space's own delete
// route. It names the memory id only, and Control decides who may delete (the room's owner or an admin).
const spaceAgentMemoryDeleteInput = z.object({
  spaceRef: z.string().min(1),
  memoryId: z.string().min(1).max(64),
})

const spaceAgentMemoryDeleteOutput = z.object({
  memory_id: z.string(),
  deleted: z.boolean(),
})

// W4-PL-4: the org's plugin catalogue, and the admin's install, uninstall, catalogue and kill-list writes (F-t3-059).
// Human only: every row is model access none, so the agent is never offered these. The routes are the gateway's
// pass-through to capability-core (agent_actions.rs), and capability-core decides every admin, zero-data-retention,
// consent and kill check. A digest has the shape capability-core writes: sha256 and 64 lower-case hex digits.
const pluginDigestInput = z.string().regex(/^sha256:[0-9a-f]{64}$/)
const pluginIdInput = z.string().trim().min(1).max(200)

const pluginsListCatalogInput = z.object({}).strict()

const pluginCatalogOutput = z.object({
  plugins: z.array(
    z.object({
      id: z.string(),
      name: z.string(),
      version: z.string(),
      description: z.string(),
      risk_level: z.string(),
      enabled: z.boolean(),
      rollout_state: z.string(),
      source_kind: z.string(),
      source_ref: z.string(),
      source_pin: z.string(),
      manifest_digest: z.string(),
      installed: z.boolean(),
      components: z.array(z.object({ kind: z.string(), name: z.string(), status: z.string() })),
    }),
  ),
})

const pluginIdOnlyInput = z.object({ pluginId: pluginIdInput }).strict()

const pluginInstallInput = z
  .object({
    pluginId: pluginIdInput,
    consentedDigests: z.array(pluginDigestInput).min(1).max(256),
  })
  .strict()

const pluginInstallOutput = z.object({
  plugin_id: z.string(),
  installed: z.boolean(),
  components: z.array(
    z.object({ kind: z.string(), name: z.string(), digest: z.string(), status: z.string(), report: z.string().optional() }),
  ),
  children: z.array(z.object({ kind: z.string(), name: z.string(), id: z.string(), digest: z.string().optional() })),
})

const pluginCatalogApproveInput = z
  .object({
    pluginId: pluginIdInput,
    approved: z.boolean(),
  })
  .strict()

const pluginCatalogApproveOutput = z.object({
  id: z.string(),
  catalog_approved: z.boolean(),
  updated_at: z.string(),
})

const pluginKillDigestInput = z
  .object({
    digest: pluginDigestInput,
    reason: z.string().trim().min(1).max(500),
  })
  .strict()

const pluginKillDigestOutput = z.object({
  digest: z.string(),
  scope: z.string(),
  org_id: z.string(),
  created: z.boolean(),
  uninstalled_plugins: z.array(z.string()),
  children_removed: z.number().int().min(0),
})

// The admin's lift of an org kill-list entry (W4-PL-4): the digest in, the removal the route answers with out.
const pluginLiftKillInput = z.object({ digest: pluginDigestInput }).strict()

const pluginKillRemovalOutput = z.object({
  digest: z.string(),
  scope: z.string(),
  org_id: z.string(),
  removed: z.boolean(),
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
    // E3.7.9a: approval depends on the page count, so the flag stays false; use
    // actionRequiresApproval(id, input) to decide. The description states the rule.
    description: 'Crawl a website with Quarry v2 and ingest discovered pages into Data Plane v2. Up to 50 pages run under the run budget; a larger crawl needs its page count approved before it starts.',
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
  ...spaceActions,
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
    ownerPlane: 'data',
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
    ownerPlane: 'data',
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
    id: 'memory.profile.save',
    label: 'Save profile',
    description: 'Replace the caller\'s standing profile with their own text. Kept as written until they choose to rebuild it. Refused while memory is off and under Zero Data Retention.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: memoryProfileSaveInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'memory.profile.rebuild',
    label: 'Rebuild profile',
    description: 'Rebuild the caller\'s standing profile from the memories they have accepted, replacing their edit. Refused while memory is paused or off and under Zero Data Retention.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: memoryProfileRebuildInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.agent_memory.list',
    label: 'List a Space agent\'s notes',
    description: 'List the notes one bound agent kept in this room, for the room\'s owner or manager. Read only. The notes are quoted context and grant the agent nothing. Refused under Zero Data Retention.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceAgentMemoryListInput,
    outputSchema: spaceAgentMemoryListOutput,
  },
  {
    id: 'spaces.agent_memory.delete',
    label: 'Delete a Space agent\'s note',
    description: 'Delete one note an agent kept in this room. The room stops remembering it for every member. Destructive and non-recoverable; allowed under every retention setting.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceAgentMemoryDeleteInput,
    outputSchema: spaceAgentMemoryDeleteOutput,
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
    id: 'learning.learn_from_thread',
    label: 'Learn from this conversation',
    description: 'Ask Verevon to distil one of the caller\'s own saved conversations into a skill proposal, with an optional note it reads as data. The proposal waits in the caller\'s learning inbox and is used only once they accept it. Hidden and refused in a temporary chat (zero data retention), and refused while memory is paused or off or the organization has not turned on learning. Never model-executable: a model does not decide what Verevon learns.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: learningLearnFromThreadInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'learning.teach_skill_from_run',
    label: 'Save these steps as a skill',
    description: 'Save the steps the caller took in one browser run, after they gave the browser back, as a disabled skill proposal in the caller\'s learning inbox. The name and the optional hint are the caller\'s text, bounded here as the server bounds them. The owner is the verified session. Nothing is used until they accept it. Hidden and refused in a temporary chat, and refused for an organization that keeps no content (zero data retention). Never model-executable: a model does not decide what Verevon learns.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: learningTeachFromRunInput,
    outputSchema: learningTeachFromRunOutput,
  },
  {
    id: 'skills.save_from_message',
    label: 'Save an answer as a skill',
    description: 'Save one of the person\'s own answers as a private skill of theirs, used in their own matching chats. The name, description and content are the person\'s text, bounded here as the server bounds them. The owner is the verified session, and no organisation is named. Org-wide skills stay admin-only; a member needs the member authoring switch on. The button is hidden in a temporary chat (zero data retention) and in a conversation not yet saved. Never model-executable: a model does not write its own skills.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: skillsSaveFromMessageInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'skills.versions.list',
    label: 'List skill versions',
    description: 'Read the immutable versions of one skill, newest first, with each version\'s text, digest, who cut it and when, for the Skills page\'s history. Read only: a version is never edited, and a restore would be a new version. Whoever may read the skill may read its history; another person\'s private skill reads as not found. Answers not found while the server\'s version history is off. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: skillsVersionsListInput,
    outputSchema: skillsVersionsListOutput,
  },
  {
    id: 'skills.draft.save',
    label: 'Save a skill draft',
    description: 'Save an edit of a published skill\'s description or text as its draft. Turns keep using the published text until the draft is published, so an edit never changes a running conversation silently. The owner or an org admin may save it; a proposal is still edited directly, because accepting it is its publish. Only while the server\'s drafts are on; off, an edit is live at once as before. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: skillsDraftSaveInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'skills.publish',
    label: 'Publish a skill draft',
    description: 'Make a skill\'s draft its live text. The server cuts a new immutable version, which every new turn uses; a run already under way keeps the version it started with. The draft is linted as the text it makes live. The owner or an org admin may publish. Never model-executable.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: skillIdInput,
    outputSchema: skillsPublishOutput,
  },
  {
    id: 'skills.draft.discard',
    label: 'Discard a skill draft',
    description: 'Throw away a skill\'s unpublished draft. The published text and every version stay as they are. The owner or an org admin may discard it. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: skillIdInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'skills.version.restore',
    label: 'Restore a skill version as a draft',
    description: 'Fill a skill\'s draft with the text of one of its earlier versions. Nothing is live until the draft is published, which cuts a new version: a restore never points the skill back at an old version. The old bundle comes back only while the server still keeps it. The owner or an org admin may restore. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: skillsVersionRestoreInput,
    outputSchema: skillsVersionRestoreOutput,
  },
  {
    id: 'skills.import',
    label: 'Import a skill from a file',
    description: 'Bring in a SKILL.md, or a zip with SKILL.md and the files it uses. The server checks the file (size caps, safe paths, no links, scripts only under scripts/), lints its text and creates the skill switched off, with its text and files as a draft: nothing steers a turn until the draft is published and the skill switched on. A person imports a private skill; an org-wide skill, or a zip with files beside SKILL.md, is an admin\'s. Refused under zero data retention. Never fetches anything and never model-executable.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: skillsImportInput,
    outputSchema: skillsImportOutput,
  },
  {
    id: 'templates.create',
    label: 'Create a slash template',
    description: 'Save a private `/` template of the person\'s own: text the composer puts in the draft when the template is picked, with `{{input}}` filled from the rest of the line. Nothing is sent until the person sends. A template is never injected into a turn as a skill. Answers kind_not_enabled while the server\'s authored kinds are off. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: authoredTemplateInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'templates.update',
    label: 'Update a slash template',
    description: 'Change the description or text of one of the person\'s templates. The owner or an org admin may change it; the server lints the text again. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: authoredUpdateInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'presets.create',
    label: 'Create a run preset',
    description: 'Save a private preset of the person\'s own: a goal text and the visible run console controls it sets (mode, web search, up to four skills, a template). Plan mode stays on and a preset carries no persona, tool list or autonomy. Applying it only fills the console\'s controls; the run still starts with the person\'s click. Answers kind_not_enabled while the server\'s authored kinds are off. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: authoredPresetInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'presets.update',
    label: 'Update a run preset',
    description: 'Change the description, goal text or settings of one of the person\'s presets, under the same closed settings as on create. The owner or an org admin may change it. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: authoredUpdateInput.extend({ settings: authoredPresetSettings.optional() }),
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
    description: 'Update the caller\'s own preferences (theme, language, timezone, date format, time format, compact mode, notification and crawl-ingest settings).',
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
    id: 'settings.set_tone_preset',
    label: 'Set reply tone',
    description: 'Set the caller\'s own reply tone (default, professional, friendly, direct, warm or neutral). It applies to the caller\'s new chats and runs, beneath the organization\'s and Space\'s instructions.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: settingsSetTonePresetInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'settings.complete_personal_setup',
    label: 'Complete personal setup',
    description: 'Save the caller\'s own display name, time zone, language and reply tone, then mark the first-run personal setup done. Only the fields given are saved, and the setup is marked done last, once they are saved.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: settingsCompletePersonalSetupInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'settings.skip_personal_setup',
    label: 'Skip personal setup',
    description: 'Record that the caller skipped the first-run personal setup. The card does not show again.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: settingsSkipPersonalSetupInput,
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
    id: 'mcp.probe_server',
    label: 'Probe MCP server',
    description: 'Ask what an MCP server needs before it is saved: its auth requirement and, when it needs no credential, up to 64 of its tools with the server\'s own hints. Read only. Saves nothing, registers nothing and sends no credential. Refused under zero data retention.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: mcpProbeServerInput,
    outputSchema: mcpProbeServerOutput,
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
    id: 'org.set_web_policy',
    label: 'Set organization web access',
    description: 'Turn public web access for the organization\'s agents off or on, and allow or forbid paid search providers for them. Takes the revision it was read at and is refused if the policy has changed since, and keeps every other agent-policy setting as it is stored. With web access off, agents are offered no web search, page fetch or crawl, and a chat turn says why. Takes effect only where the deployment has switched policy enforcement on. An org admin only. Refused typed under zero data retention. Never model-executable: a model does not set the rules it runs under.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgSetWebPolicyInput,
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
    id: 'browser.get_org_policy',
    label: 'Read organization browser policy',
    description:
      'Read the organization browser policy: what agents may do in the browser, the domains they may visit, the plan entitlements and the effective view, at its current revision. Any active member may read it; the stored document is shown to an owner or admin only. Never model-executable.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserGetOrgPolicyInput,
    outputSchema: browserGetOrgPolicyOutput,
  },
  {
    id: 'browser.set_org_policy',
    label: 'Set organization browser policy',
    description:
      'Replace the organization browser policy with the document given, at the revision it was read at. A stale revision is refused and nothing is merged. Owner or admin only; a switch the plan does not include is refused. Never model-executable: a model does not set the rules it runs under.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: browserSetOrgPolicyInput,
    outputSchema: browserSetOrgPolicyOutput,
  },
  {
    id: 'policy.read_effective',
    label: 'Read effective policy',
    description: 'Read the effective policy of one thread: each value the gateway enforces for it, the layer that set it, and a lock where a higher layer fixes it. Any member of the organization may read it, and the organization comes from the session. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: policyReadEffectiveInput,
    outputSchema: policyReadEffectiveOutput,
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
    id: 'org.list_permission_rules',
    label: 'List organization permission rules',
    description: 'Read the deny and ask rules that tighten the tool calls of the organization agents, and the policy revision they are at. Any member of the organization may read them. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: z.object({}).strict(),
    outputSchema: orgPermissionRuleListOutput,
  },
  {
    id: 'org.create_permission_rule',
    label: 'Add organization permission rule',
    description: 'Add a rule that denies, or asks first about, one tool call of the organization agents for one target: a web domain, a mail recipient domain (never an address), a command prefix, or any target of the tool. A rule only tightens a call and never allows one. Takes effect only where the deployment has switched permission rules on. An org admin only. Refused typed under zero data retention. Never model-executable: a model does not set the rules it runs under.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgCreatePermissionRuleInput,
    outputSchema: orgPermissionRuleWriteOutput,
  },
  {
    id: 'org.delete_permission_rule',
    label: 'Remove organization permission rule',
    description: 'Remove an organization permission rule, so it no longer tightens the calls it covered. The removal is audited. An org admin only. Refused typed under zero data retention. Never model-executable.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgDeletePermissionRuleInput,
    outputSchema: orgPermissionRuleDeleteOutput,
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
    id: 'monitoring.create_monitor',
    label: 'Create change monitor',
    description: 'Re-check a URL hourly, daily or weekly and record a baseline whenever its content changes.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: monitoringCreateMonitorInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'monitoring.delete_monitor',
    label: 'Delete change monitor',
    description: 'Stop and remove a recurring change monitor. Baselines already captured are kept.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: monitoringDeleteMonitorInput,
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
    id: 'ingestions.job_control',
    label: 'Pause, resume or cancel job',
    description: 'Pause, resume or cancel a running crawl or batch job. Scrape, search, extract and agent runs finish in one request and cannot be controlled.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: ingestionsJobControlInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.start_agent_run',
    label: 'Start browser agent run',
    description: 'Open a browser session on a URL and start a durable browser-agent run that works toward a goal within a step budget.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: ingestionsStartAgentRunInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.create_schedule',
    label: 'Create ingestion schedule',
    description: 'Create a recurring or one-time-scheduled ingestion job (crawl, scrape, extract, search, or a batch of URLs). The schedule is created inactive and is never activated by this action.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ingestionsCreateScheduleInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.get_run_status',
    label: 'Get ingestion run status',
    description: 'Read the status, phase, desired state and progress of one crawl job. Read only.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: ingestionsRunRefInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.pause_run',
    label: 'Pause ingestion run',
    description: 'Ask a running crawl to pause. Work already fetched is kept; resume continues it.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ingestionsRunRefInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.resume_run',
    label: 'Resume ingestion run',
    description: 'Ask a paused crawl to continue from where it stopped.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ingestionsRunRefInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.cancel_run',
    label: 'Cancel ingestion run',
    description: 'Stop a crawl for good. A cancelled run cannot be resumed; pages already stored are kept.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: true,
    reversible: false,
    inputSchema: ingestionsRunRefInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.pause_schedule',
    label: 'Pause ingestion schedule',
    description: 'Pause an ingestion schedule so it stops firing. Unpausing is a separate state change.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: ingestionsPauseScheduleInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'ingestions.change_schedule_state',
    label: 'Change schedule state',
    description: 'Unpause, trigger, backfill, or delete an existing ingestion schedule. Deleting a schedule cannot be undone.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: true,
    reversible: false,
    inputSchema: ingestionsChangeScheduleStateInput,
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
    id: 'integrations.list_connector_policies',
    label: 'Show connected app levels',
    description: 'Read how the agent may use the person\'s connected apps: reads used without asking, asked about first or off, and writes always asked about or off, for the person and for their org, with whether the deployment enforces the levels yet. Read-only. Never model-executable: a model does not read the rules it runs under to plan around them.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: z.object({}).strict(),
    outputSchema: integrationsConnectorPoliciesOutput,
  },
  {
    id: 'integrations.set_connector_policy',
    label: 'Set connected app level',
    description: 'Set the person\'s own level for every connected app at once: reads used without asking, asked about first or off; writes always asked about or off. Only tightens: a write is never released from asking, and nothing is relaxed past what the org set, because the strictest level in force wins. Stored as the person\'s tool policy on the provider capabilities and honoured once the deployment enforces tool policies. Refused typed under zero data retention. Never model-executable: a model does not set the rules it runs under.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: integrationsSetConnectorPolicyInput,
    outputSchema: envelopeStatusOutput,
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
    id: 'org.set_retention',
    label: 'Set organization retention window',
    description: 'Set how many days of inactivity a personal thread is kept before the retention sweep may delete it (30 to 3650), or keep threads indefinitely. Held threads are never deleted. Owner or admin only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgSetRetentionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.place_legal_hold',
    label: 'Place legal hold',
    description: 'Hold the whole organization or one thread so the retention sweep deletes nothing it covers until the hold is released. A thread kept under zero data retention has nothing to hold and is refused. Owner or compliance only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgPlaceLegalHoldInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.release_legal_hold',
    label: 'Release legal hold',
    description: 'Release a legal hold, after which the held content follows the retention window again. Owner or compliance only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: orgReleaseLegalHoldInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.add_domain',
    label: 'Add organization domain',
    description: 'Claim a company domain for the organization. Returns a DNS TXT challenge to publish; nothing is trusted until it is verified. Owner or admin only.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgDomainInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.verify_domain',
    label: 'Verify organization domain',
    description: 'Check the published DNS TXT challenge for a claimed domain. DNS is the only proof; limited to six attempts per domain per hour. Owner or admin only.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: orgDomainInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.revoke_domain',
    label: 'Remove organization domain',
    description: 'Withdraw a claimed or verified company domain. Refused while the domain backs an enforced single sign-on policy. Owner or admin only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgDomainInput,
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
    id: 'org.update_sso_policy',
    label: 'Update organization single sign-on policy',
    description: 'Set the single sign-on policy to off, report or enforce, and name the break-glass owners. Enforce needs a verified domain, a working provider, your own recent SSO sign-in and two-factor break-glass owners; it ends the password sessions of covered users. Owner or admin only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgUpdateSsoPolicyInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.add_sso_provider',
    label: 'Add single sign-on provider',
    description: 'Register an OpenID Connect identity provider for a verified organization domain. The client secret is write-only. Owner or admin only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgAddSsoProviderInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.remove_sso_provider',
    label: 'Remove single sign-on provider',
    description: 'Remove an identity provider. Refused for the last provider of an enforced policy. Owner or admin only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: orgRemoveSsoProviderInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.create_scim_token',
    label: 'Create SCIM token',
    description: 'Create a bearer token your identity provider uses to provision and deprovision members through SCIM. The token is shown once. Needs a verified domain. Owner or admin only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgCreateScimTokenInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'org.revoke_scim_token',
    label: 'Revoke SCIM token',
    description: 'Revoke a SCIM token so the identity provider can no longer provision or deprovision members. Owner or admin only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: orgRevokeScimTokenInput,
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
    description: 'Approve or reject one pending approval of a run, for this one call. Only the person the approval is requested of can decide it; the decider is the verified credential, never an input, and a client field naming a decider, a decision source or a grant is refused 400. A scope other than once is refused 422 scope_not_enabled until standing grants exist, so a caller never believes it granted more than it did. A decision is idempotent, and a different second decision is a 412 conflict. Refused typed under zero data retention and for a waived credential, since such a run holds no approval. An approval may carry edit {patch, baseFingerprint}: the person\'s changes to a drafted mail send (to, subject, bodyText only), approved for this one call; an edit with a reject is refused edit_requires_grant, a draft that changed since the person saw it is a 409 approval_changed, and while edits are switched off the server refuses it 403 approval_edit_disabled. Never model-executable: an approval is the person\'s own authority, and a model deciding its own approval would void the gate.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: approvalsDecideInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'approvals.preview',
    label: 'Preview an approval draft',
    description: 'Read the full draft a pending write approval would send, before deciding it: every recipient, the full subject and the full plain-text body of a mail send. Read-only; it changes nothing and grants nothing. Only the run\'s owner sees it, and only while the approval is pending: another person, another org, a decided or expired approval and an unknown id all answer the same 404 approval_not_found. A call other than a mail send shows only its provider and operation. Refused typed under zero data retention and for a waived credential, since such a run keeps no draft. Off until the preview is switched on, when it answers 404 preview_unavailable. The body is text and is never rendered as markup. Never model-executable: the draft is shown to the person deciding it.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: approvalsPreviewInput,
    outputSchema: approvalsPreviewOutput,
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
    id: 'chat.thread.archive',
    label: 'Archive chat thread',
    description: 'Hide one of the caller\'s own chat threads from the ordinary list. It stays in the archive and can be restored. Owner only; never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: threadArchiveAction.omit({ type: true }),
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.thread.star',
    label: 'Star chat thread',
    description: 'Star or unstar one of the caller\'s own chat threads. A star is the person\'s own mark, so it is never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: threadStarAction.omit({ type: true }),
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.thread.mark_unread',
    label: 'Mark chat thread unread',
    description: 'Mark one of the caller\'s own chat threads unread, so it returns to the attention list. Owner only; never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: threadMarkUnreadAction.omit({ type: true }),
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'thread.export',
    label: 'Export chat thread',
    description: 'Download one of the caller\'s own chat conversations as a file, JSON (verevon.thread-export.v1) or Markdown, in full or redacted; redacted leaves out tool payloads, connector data and attachment names. Owner only, and read only: nothing is changed or stored. Refused under zero data retention, for a thread with a run kept outside retention, and for a Space conversation; the route answers 404 while the export is off. Never model-executable: a model does not take a copy of a person\'s conversation off the platform.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: threadExportInput,
    outputSchema: threadExportOutput,
  },
  {
    id: 'thread.share_snapshot',
    label: 'Take a conversation snapshot',
    description: 'Take an immutable, redacted copy of one of the caller\'s own chat conversations so it can be shared with named members of the organisation. There is no public link: the snapshot is read only by its owner and by members the owner then grants a view of. Nothing is shared by taking it. Owner only. Refused under zero data retention, for a waived caller, for a Space conversation and for a thread with a run kept outside retention; at most 20 live snapshots per thread; the route answers 404 while snapshots are off. Never model-executable: a model does not copy a person\'s conversation for others.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: threadShareSnapshotInput,
    outputSchema: threadSnapshotOutput,
  },
  {
    id: 'thread.revoke_snapshot',
    label: 'Revoke a conversation snapshot',
    description: 'Revoke one of the caller\'s own conversation snapshots: its copy is deleted and so is every member grant on it, so nobody can read it again. Owner only. Allowed under zero data retention, since revoking makes no copy. Irreversible: a revoked snapshot is gone, and a new one has to be taken. Never model-executable.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: threadRevokeSnapshotInput,
    outputSchema: threadSnapshotOutput,
  },
  {
    id: 'thread.grant_snapshot',
    label: 'Share a snapshot with a member',
    description: 'Give one member of the caller\'s own organisation a view of a live conversation snapshot, or take that view away (change add or remove). The grant is Control\'s: the member reads the redacted copy as static text, only while they are a member and the snapshot is live, and the role is always view. Owner only. Refused under zero data retention for add, for a member outside the organisation, and for a revoked snapshot; the route answers 404 while snapshots are off. Reversible: remove undoes add. Never model-executable: a model never grants access to a person\'s conversation.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: threadGrantSnapshotInput,
    outputSchema: threadSnapshotGrantOutput,
  },
  {
    id: 'thread.import',
    label: 'Import chat thread',
    description: 'Make a new chat thread the caller owns from one of their own conversation export files (verevon.thread-export.v1, JSON, at most 2 MiB). The file is untrusted input: it is parsed strictly, only the person\'s and the assistant\'s text is kept as quoted text, and what the file holds that a thread does not keep is left out and counted. Owner only. Refused under zero data retention and for a waived caller; the route answers 404 while the import is off. Never model-executable: a model does not write a person\'s conversation into their history.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: threadImportInput,
    outputSchema: threadImportOutput,
  },
  {
    id: 'chat.save_artifact_revision',
    label: 'Save an edit of an artifact',
    description: 'Save the person\'s own text of a document, code or HTML artifact in the Output panel as its newest version, under a compare-and-set on the version the panel showed: a moved head is refused with revision_mismatch and the head now stored, and nothing is overwritten unless the person chooses to. Owner only. Refused under zero data retention, in a thread that holds a waived run, in a Space thread, while a run in the thread is still working, and for any other kind (a deck is never edited as text). Off until artifact editing is switched on. Reversible: every earlier version stays and can be restored. Never model-executable: the model revises artifacts through its own tools.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatSaveArtifactRevisionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.restore_artifact_revision',
    label: 'Restore an earlier version of an artifact',
    description: 'Copy an earlier version of a document, code or HTML artifact into a new newest version, under the same compare-and-set and the same refusals as saving an edit. Nothing is deleted: the versions after the restored one stay in the history. Owner only, off until artifact editing is switched on. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatRestoreArtifactRevisionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.edit_artifact_selection',
    label: 'Revise a selected part of an artifact',
    description: 'Ask for a change to one selected range of a document, code or HTML artifact in the Output panel, as a normal chat turn that names the range: the artifact, the version the person saw, the character offsets and the SHA-256 of the selected text (the text itself is not sent). The gateway refuses a version that is no longer the head with artifact_selection_stale, refuses a quote that does not match the range with artifact_selection_mismatch, and refuses any rewrite that changes text outside the range. Off until selection editing is switched on. Refused under zero data retention, in a temporary or Space conversation, and for any other kind. Never model-executable: a model does not choose which part of the person\'s document it rewrites.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatEditArtifactSelectionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.save_artifact_to_knowledge',
    label: 'Save an artifact version to knowledge',
    description: 'Copy the document artifact version shown in the Output panel into the person\'s knowledge base as a new Data Plane document (origin chat-artifact), with the thread, artifact and version kept in its source trace. The copy is independent: later edits of the artifact do not sync, and it stays when the chat is deleted, under Data Plane retention and erase. Refused under zero data retention and in a temporary chat. Off until the gateway\'s artifact promote switch is on. Not reversible from the chat: the copy is removed from the knowledge page. Never model-executable: a model does not decide what enters the knowledge base.',
    ownerPlane: 'data',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatSaveArtifactToKnowledgeInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'library.list',
    label: 'List chat library',
    description: "List the person's saved chat files, generated artifact copies, uploaded text, or both, newest first, one page at a time. Read only: nothing is changed. The viewer's own predicate applies (their own files, files shared with them, and org-visible files), Space room files are left out, and a page carries metadata, never text. Off until the gateway's artifact promote switch is on. Never model-executable: a model does not browse the person's saved files.",
    ownerPlane: 'data',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatLibraryListInput,
    outputSchema: chatLibraryListOutput,
  },
  {
    id: 'library.read_text',
    label: 'Read a chat library file',
    description: "Read the text of one saved chat file by its document id, through the knowledge document route. Read only: nothing is changed or stored, and only a file the viewer may read is returned. Never model-executable: a model does not take the text of the person's saved files.",
    ownerPlane: 'data',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatLibraryReadTextInput,
    outputSchema: chatLibraryReadTextOutput,
  },
  {
    id: 'library.open_origin',
    label: 'Open the chat a library file came from',
    description: "Open the chat thread a saved copy was made in. Navigation only, in the browser: it selects that chat thread and goes to the chat page. Nothing is read from the server or changed, and the thread opens under the person's own access. Never model-executable.",
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatLibraryOpenOriginInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'library.share',
    label: 'Share a chat library file',
    description: "Writes resource grants (an effect): gives org members or groups viewer or editor access to one of the person's own saved chat files. Grant only: there is no public link, and a key the contract does not name is refused. Owner only, and refused under zero data retention. Never model-executable: a model does not decide who may read the person's files.",
    ownerPlane: 'data',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatLibraryShareInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'review.run',
    label: 'Review a text as a run',
    description: "Run one read-only review of a text the person chose: an artifact revision, one turn's diff or an attached file. The model's located P0-P3 findings come back with the gateway's own source checks, kept apart, and each finding is checked against the text before it is kept. Read only: nothing is changed or stored, and the answer is the review card alone. Refused for an unknown target or a text over its limit, and answers 404 review_run_disabled while the review switch is off. Never model-executable: a model does not choose what is reviewed.",
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: reviewRunInput,
    outputSchema: reviewRunOutput,
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
    id: 'chat.set_context_refs',
    label: 'Attach context to a chat turn',
    description: 'Name the context one chat turn carries: the composer\'s context chips (knowledge documents, skills, the conversation\'s own Space), sent as context_refs on that turn only. At most 20 distinct references, each a kind, an id and a display label; an empty list clears the choice. The gateway resolves every reference again under the person\'s own access, a document only narrows the knowledge search, and a Space can only name the conversation\'s own. A zero-retention or temporary turn never carries references. Never model-executable: a model does not choose what the person attaches.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatSetContextRefsInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.pick_context_ref',
    label: 'Open the context picker',
    description: 'Open the composer\'s context picker (the "+ Kontekst" button or /kontekst) on one tab: knowledge documents, skills, or the conversation\'s own Space. Read only: it lists what the person can already open through the existing document and skill reads, and nothing is attached until they pick. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatPickContextRefInput,
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
    id: 'chat.set_transcript_density',
    label: 'Choose how much chat work the transcript shows',
    description: 'Name the transcript level the person reads chat work at: detailed, standard, compact (settled turns fold to one row) or minimal (answers, decisions and files, with the tool and reasoning trail hidden). The level is stored in the person\'s appearance settings and applies to every chat they open. Never model-executable: a model does not choose how the person reads its work.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatSetTranscriptDensityInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.inspect_run_request',
    label: 'Inspect what a model call was sent',
    description: 'Read what one model call of the person\'s own run was sent and offered: the parameters, the routed model, the prompt sections in order (platform sections as label, version digest and length only; organisation, user and memory sections with their text), the tool catalogue and the message manifest without message bodies. Read only and owner only: anyone else, an organisation admin included, gets not found, and a zero-retention session is refused. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatInspectRunRequestInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.open_cited_passage',
    label: 'Open the passage a source contributed',
    description: 'Open the passage one cited source contributed to an answer: the quote the gateway sent with the citation, for a knowledge source the chunk it names read again through Data Plane\'s chunk read (Data Plane resolves the viewer\'s own grants first, so a document the person may not read shows a refusal and no text), and for a web source the live page opened at the quote. Read only; nothing is stored. Never model-executable.',
    ownerPlane: 'data',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatOpenCitedPassageInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.list_artifact_revisions',
    label: 'List an artifact\'s revisions',
    description: 'Read the stored revisions of one artifact in the person\'s own chat thread, newest first, with who made each (the model, the person, or a restore) and when, so the Output panel\'s version stepper survives a reload and another device. Read only and owner only: another person\'s thread reads as not found, a zero-retention session is refused, and a temporary or Space room thread is never asked. Answers not found while the gateway\'s revision history is off. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatListArtifactRevisionsInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.wide_research_start',
    label: 'Start wide research',
    description: 'Start a wide-research run from the composer: one Do turn that researches up to the organisation\'s item cap of separate questions in parallel and merges the answers. The person gives a pasted list (one question per line) or a count preset, and sees an estimate of the items, time and cost first. The server clamps the count to the organisation\'s research.max_fan_out_items policy and names any items past it in the answer; a zero-retention session is refused. Never model-executable: a model does not start a fan-out that spends on the person\'s behalf.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatWideResearchStartInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.research_plan_start',
    label: 'Start deep research from the plan',
    description: 'Run a deep research exactly as the person edited its plan: the plan card\'s sub-queries (edited, removed or added up to the cap) and the answers to its clarifying questions are sent as the run turn. Model-gateway re-clamps the plan (length, count, duplicates) before anything is searched; a zero-retention session never gets a plan card. Never model-executable: a model does not start research that searches and spends on the person\'s behalf.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatResearchPlanStartInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.research_plan_cancel',
    label: 'Dismiss a deep research plan',
    description: 'Close a deep research plan card without running it. Nothing is sent: no research state waits on the server between the plan turn and the run turn, so a dismissed plan leaves nothing behind.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatResearchPlanCancelInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.accept_connector_suggestion',
    label: 'Accept a connected-app suggestion',
    description: 'Answer the card the model shows when a question needs the person\'s own mail, calendar or files and no connected account was selected: connect the suggested provider through the existing connect session and its sign-in window (integrations.start_connect_session), or use an account already connected for this question only, which selects it (chat.set_connector_scope) and sends the question again. The card names one provider from a closed list (google, microsoft) and is never shown in a Space, a temporary chat or under zero data retention. Nothing is remembered beyond the one turn. Never model-executable: a model does not connect or select the person\'s accounts.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatAcceptConnectorSuggestionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.wide_research_retry_item',
    label: 'Retry a wide-research item',
    description: 'Run one item of a wide-research fan-out again as its own one-item wide-research turn in the same conversation. The item is not sent from the browser: execution-core reads it back from the child run that failed, under the person\'s own credential, and only from a fan-out item of this conversation; the new attempt joins the same batch, so the grid shows it beside its siblings. Counts against the organisation\'s item cap like any wide-research run; a zero-retention session is refused. Never model-executable: a model does not spend on the person\'s behalf.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatWideResearchRetryItemInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.list_deck_templates',
    label: 'List deck templates',
    description: 'Read the organisation\'s brand deck templates for the template picker: each one\'s name, description, colours and font names, never a file. The templates are skill bundles in capability-core, read under the person\'s own credential, so only the person\'s own organisation and the skills they may use are listed. Read only. Hidden while the gateway\'s deck templates are switched off; a zero-retention session is refused. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatListDeckTemplatesInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.apply_deck_template',
    label: 'Apply a deck template',
    description: 'Use one of the organisation\'s brand deck templates for a deck: the picker sends a chat turn that carries the template\'s skill id, and model-gateway checks it against the organisation\'s templates before the deck is built with it. The model is only told which template was applied and never picks one itself. Reversible: picking another template, or Standard, builds the deck again. Refused under zero data retention; off until deck templates are switched on. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatApplyDeckTemplateInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.open_deck_slide',
    label: 'Open a slide of a deck',
    description: 'Open one slide of a deck in the chat\'s artifact panel: the strip shows the slide\'s lines, its state, its findings and its speaker notes. Read only, and nothing is sent to the server: the panel reads the manifest it already holds. Never model-executable: a model does not choose what the person looks at.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatOpenDeckSlideInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.present_deck',
    label: 'Present a deck',
    description: 'Present a deck in the chat\'s artifact panel full screen, one slide at a time: from the rendered pages when the render has one page per slide, and from the slides otherwise. Read only, and nothing is sent to the server: the panel shows the manifest and the render it already holds. Refused, with the reason shown, while the deck has no slide, a slide with an error or a slide that was never built; a warning never holds it back. Never model-executable: a model does not choose what the person presents.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatPresentDeckInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.set_goal',
    label: 'Set a goal on the conversation',
    description: 'Set a standing goal on one of the person\'s own conversations: an objective and up to 8 success criteria. Every later Do turn in the conversation is told the goal as data, never as instructions. One open goal per conversation; replacing it closes the old one as cleared. Owner only. Refused under zero data retention, in a conversation that holds a waived run and in a Space conversation. Off until thread goals are switched on. Never model-executable: the model reads its goal and never sets it.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatSetGoalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.update_goal',
    label: 'Edit or pause the conversation goal',
    description: 'Change the objective, the success criteria or the turn cap of the conversation\'s open goal, or pause and resume it, under a compare-and-set on the revision the goal bar showed: a goal changed elsewhere is refused with thread_goal_revision_conflict. A goal that is met or stopped cannot be changed. Owner only, with the same refusals as setting a goal. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatUpdateGoalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.clear_goal',
    label: 'Clear the conversation goal',
    description: 'Close the conversation\'s open goal as cleared, so no later turn is told it. The record stays until the conversation is deleted. Owner only, under the same compare-and-set as editing it. Never model-executable.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatClearGoalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.continue_goal',
    label: 'Continue toward the goal',
    description: 'Send one turn toward the conversation\'s active goal, through the ordinary send path and in the conversation\'s own mode: a plan turn, or an execute turn only while the conversation holds a live plan grant. The server refuses a goal that is not the conversation\'s active goal with thread_goal_mismatch before any run exists, and every approval the run needs still waits for the person. Never model-executable: a goal never continues itself.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatContinueGoalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.goal_auto_toggle',
    label: 'Keep going toward the goal',
    description: 'Turn the goal bar\'s "Keep going" on or off for the open conversation. While it is on, after a goal turn completes and the judge did not find the goal met, the bar counts down five seconds, with Stop, and sends the next Continue; a hidden tab never sends. It stops on any pending approval, question, proposed plan or parked run, on a failed turn, on a cap, and on a met or blocked goal, and it never approves anything. The server refuses a pump-sent turn with goal_auto_disabled while the feature is off and refuses every goal turn with thread_goal_pending_decision while a decision waits. Never model-executable: a goal never continues itself.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatGoalAutoToggleInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.goal_mark_done',
    label: 'Mark the conversation goal done',
    description: 'Mark the conversation\'s open goal met, as the person\'s own call over the judge\'s verdict, under a compare-and-set on the revision the goal bar showed. The goal closes, so no later turn is told it and the goal pump stops. Owner only, with the same refusals as an edit. Never model-executable: the person closes a goal, and the model never does.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatGoalMarkDoneInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.goal_reopen',
    label: 'Reopen the conversation goal',
    description: 'Reopen the conversation\'s met or blocked goal as active, under a compare-and-set on the revision the goal bar showed, so later turns are told it again and it can continue. Owner only, with the same refusals as an edit. Never model-executable: a goal never reopens itself.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatGoalReopenInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.goal_away_start',
    label: 'Keep the conversation goal working while away',
    description: 'Start a schedule that keeps one of the person\'s own goals working while they are away: at most 20 runs, one every 30 minutes, hourly, every four hours or daily, for at most 90 days. Each run is read only and runs as the service under a stored-consent contract that names the read tools; each run answers in its own scheduled thread, and a change it would make is held as a draft. Owner only, on an active goal in a durable personal conversation, and the server reads the switch, the judge and the zero-retention state again. Refused under zero data retention, in a Space conversation and while the feature is off. Never model-executable: the person starts it, and a goal never schedules itself.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatGoalAwayStartInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.stage_widget_followup',
    label: 'Stage a follow-up an inline widget proposed',
    description: 'Show a follow-up question an inline widget in a chat answer proposed as a chip under that widget, labelled as coming from the widget. Nothing is sent: only the person\'s click on the chip puts it through the answer\'s ordinary follow-up handler, the same one the answer\'s suggested follow-ups use. At most one chip per widget at a time, five per answer per minute, and 500 characters; anything over is dropped. Never model-executable: a widget is model output, and the person decides every turn it proposes.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatStageWidgetFollowupInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.stage_widget_selection',
    label: 'Quote text an inline widget selected',
    description: 'Show a passage an inline widget in a chat answer selected as a quote under that widget, labelled as selected in the widget. Nothing is sent: only the person\'s click on the quote puts it, quoted, into the composer through the answer\'s ordinary follow-up handler, and the person sends it as an ordinary turn. One quote per widget at a time, five per answer per minute shared with its follow-up chips, and 2,000 characters; line breaks become spaces and other control characters are removed. Never model-executable: a widget is model output, and the person decides what they send.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatStageWidgetSelectionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.open_widget_link',
    label: 'Open a link an inline widget asked for',
    description: 'Show a link an inline widget in a chat answer asked to open, with its full address, under that widget; the person opens it in a new tab with no referrer, or cancels. Web and mail addresses only: any other scheme is refused with a note and nothing is opened. Nothing is sent to the server. Never model-executable: a widget is model output, and the person decides where they go.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatOpenWidgetLinkInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.open_mcp_app',
    label: 'Show the app of an MCP tool call',
    description: 'Draw the app an MCP tool call in a chat answer declares, under that answer, labelled with its server, in the same isolated frame as an inline widget: no network, no storage and no host data. The gateway reads the app with the viewer\'s own access to the server, and nothing is stored. The app is told the call\'s input and result once; it may size itself, stage a message and ask to open a link, and the person decides each. At most two apps per answer open on their own; the others wait for a click. Hidden in a temporary chat (zero data retention). Never model-executable: what a tool draws is not the model\'s to open.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatOpenMcpAppInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.stage_mcp_app_message',
    label: 'Stage a message an MCP App proposed',
    description: 'Show a message an MCP App in a chat answer asked to send as a chip under that app, labelled as coming from the app. Nothing is sent: only the person\'s click on the chip puts it through the answer\'s ordinary follow-up handler. At most one chip per app at a time, five per answer per minute, and 500 characters; anything over is refused to the app. Never model-executable: an app is a server\'s content, and the person decides every turn it proposes.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatStageMcpAppMessageInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.mcp_app_tool_call',
    label: 'Run a read tool an MCP App asked for',
    description: 'Run a tool an MCP App in a chat answer asked its host to call, on the app\'s own server, as the person viewing it, and hand the app the result. The gateway runs it only for the owner of the run that drew the app, only for a tool the server\'s allowlist names and the registry classes as a read with no approval floor, and through the same execution step and policy check a Do run uses; anything else is refused with app_call_requires_chat and the app may stage a message instead. At most ten calls per app per minute; the output is scrubbed and capped at 64 KiB, and the arguments are never logged. Refused in a temporary chat (zero data retention). Never model-executable: an app is a server\'s content, not the model\'s.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: chatMcpAppToolCallInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'chat.open_computer_viewer',
    label: 'Open a tool call in the computer panel',
    description: 'Show one tool call of the person\'s conversation in the computer panel, in the viewer its kind calls for: a terminal for a command or a process, and a file view for a workspace file. A browser step keeps its own view. Read only, and nothing is sent to the server: the panel reads the transcript it already holds. Off until the computer panel is switched on. Never model-executable: a model does not choose what the person looks at.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: chatOpenComputerViewerInput,
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
    description: 'Approve or reject a pending orchestration-run approval. This is the human checkpoint the run is waiting on. On a reject, reason may carry redirect text (what the person wants the agent to do instead); it is capped at 1000 characters. A reject with reason aborts the run today unless the owner accepts the redirect. An approve may carry edit {patch, baseFingerprint} (to, subject, bodyText of a drafted mail send), for this one call only: never with a reject, a standing scope or Always; a changed draft is a 409 approval_changed.',
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
    id: 'browser_run.steer',
    label: 'Steer browser agent run',
    description: 'Send one message from the person into a running browser-agent run. The run takes it at its next step. If the run ends first it answers run_ended and nothing starts. Human-only, from the browser dock: a model cannot send it. Ephemeral: the owner holds the text in memory until a step takes it and never writes it to the run event log or the transcript. Refused under zero data retention. Not reversible once taken.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: browserRunSteerInput,
    outputSchema: browserRunSteerOutput,
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
    description: 'Hand browser-session control between agent and human, compare-and-set on the control epoch. Also the Take over and Give back of a chat run\'s browser in the live panel (EP-1), on the Quarry run the turn is bound to. Control authority lives in Quarry; this is a thin proxy to it.',
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
    id: 'browser_download.send_to_knowledge',
    label: 'Send download to knowledge',
    description: 'Send one file the browser downloaded in this session to the knowledge base. Human-only, from the downloads drawer. Only PDF, plain text, Markdown and CSV are admitted: their parsed text goes to Data Plane through Ingestion Plane under the caller\'s org, and the file itself is not sent. Refused under zero data retention, for other file types and for an artifact the session does not hold. The owner dedupes on the request key, so a retry cannot add the document twice.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: browserDownloadSendToKnowledgeInput,
    outputSchema: browserDownloadSendToKnowledgeOutput,
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
    id: 'models.create_customer_gateway',
    label: 'Register customer gateway',
    description: 'Register the organisation\'s own OpenAI-compatible model gateway. Owners and admins only. Integration Core seals the key and never returns it. A customer gateway is never zero data retention and is the lowest privacy tier.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: modelsCreateCustomerGatewayInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'models.delete_customer_gateway',
    label: 'Remove customer gateway',
    description: 'Remove one of the organisation\'s customer gateways. Owners and admins only. Its key is discarded and its models stop being offered.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: modelsDeleteCustomerGatewayInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'connectors.create_credential_binding',
    label: 'Store connector key',
    description: 'Store one bearer token or API key for one host, for a custom REST connector or an MCP server. Owners and admins only. The key is sent once; Integration Core seals it and never returns it. Refused for an organization with zero data retention.',
    ownerPlane: 'ingestion',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: connectorsCreateCredentialBindingInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'connectors.revoke_credential_binding',
    label: 'Revoke connector key',
    description: 'Revoke one of the organization\'s stored connector keys. Owners and admins only. Its stored secret is emptied, and a connector that calls with it stops working. Revoking is never refused.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: connectorsRevokeCredentialBindingInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'connectors.create_custom_rest',
    label: 'Create custom REST connector',
    description: 'Create a custom REST connector that calls one host through a stored connector key. Owners and admins only. Its host, methods and path prefixes are the key\'s, not the request\'s.',
    ownerPlane: 'ingestion',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: connectorsCreateCustomRestInput,
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
    id: 'audit.list_events',
    label: 'List audit log events',
    description: 'Page the organization\'s audit trail newest first with a cursor and filters on event, user, plane, outcome, subject and resource. Read-only metadata, no export. Owner or admin see the organization; other roles only their own rows. Human sessions only.',
    ownerPlane: 'control',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: auditListEventsInput,
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
  {
    id: 'schedule.list',
    label: 'List schedules',
    description: 'List the schedules the signed-in person created in this organization, with cadence, enabled state and next run. Read only.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: scheduleListInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'schedule.pause',
    label: 'Pause schedule',
    description: 'Stop the future fires of one schedule. A run already in progress finishes. The creator or an organization admin may pause it.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: scheduleIdInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'schedule.resume',
    label: 'Resume schedule',
    description: 'Start the fires of a paused schedule again. A schedule whose approval has expired cannot be resumed; renewing the approval creates a new schedule.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: scheduleIdInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'schedule.update_time',
    label: 'Change schedule time',
    description: 'Change the cron expression of one schedule. Only the time changes: the goal, the approval and the creator stay as they are.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: scheduleUpdateTimeInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'schedule.delete',
    label: 'Delete schedule',
    description: 'Delete one schedule. It stops firing at once and cannot be restored from the product. The creator or an organization admin may delete it.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: scheduleIdInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'schedule.fires.list',
    label: 'List schedule fires',
    description: 'List the most recent fires of one of your schedules, with their status, outcome and the thread that holds the answer. Schedule creator or org admin only.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: scheduleFiresListInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'schedule.run_now',
    label: 'Run schedule now',
    description: 'Run one of your schedules once, now, under the approval it already holds. It goes through the same fresh authorization as a scheduled run, counts against the schedule\'s manual-run limits (one at a time, six an hour) and leaves the next scheduled run where it is. Schedule creator only.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: scheduleRunNowInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'automations.list_event_triggers',
    label: 'List event triggers',
    description: 'List the event triggers the signed-in person set up, newest first, or only those in one Space: the connection, the Space, the status and the bound schedule. Read only. Refused under zero data retention.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: eventTriggerListInput,
    outputSchema: eventTriggerListOutput,
  },
  {
    id: 'automations.create_event_trigger',
    label: 'Create event trigger',
    description: 'Set up a trigger that starts a run when a webhook event on one connection matches its wake filter. It is bound to one of the person\'s own schedules in the same Space, which must hold an approval, or to none. The binding grants nothing: every run is authorised again under that schedule\'s approval. The person creates it, never a model. Refused under zero data retention.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: eventTriggerCreateInput,
    outputSchema: eventTriggerEnvelopeOutput,
  },
  {
    id: 'automations.update_event_trigger',
    label: 'Change event trigger',
    description: 'Pause, resume or disable one of the person\'s event triggers, or change its name, schedule or wake filter, under the revision it was read at. Disabling is final: a disabled trigger takes no change. The person changes it, never a model. Refused under zero data retention.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: eventTriggerUpdateInput,
    outputSchema: eventTriggerEnvelopeOutput,
  },
  {
    id: 'automations.dry_run_event_trigger',
    label: 'Dry run event trigger',
    description: 'Ask whether one trigger would wake on one stored webhook event, and which of its filter rules the event meets. Read only: nothing is recorded or started, and no payload or field value is returned. Ten dry runs a minute per trigger. Refused under zero data retention.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: eventTriggerDryRunInput,
    outputSchema: eventTriggerDryRunOutput,
  },
  {
    id: 'agents.installations.list',
    label: 'List agent installations',
    description: 'List the agent definitions in this organization, with the rooms each one is added to and each installation\'s status. Read only: it shows where an agent is installed, not whether the caller may act in that room.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: agentInstallationsListInput,
    outputSchema: agentInstallationsListOutput,
  },
  {
    id: 'agents.draft.save',
    label: 'Save agent draft',
    description: 'Save the editable draft of one agent: name, instructions, model, tools by capability id and autonomy cap. Nothing a running turn reads changes until the draft is published. Org admin, the agent\'s creator, or a manager of the room it was created in.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: agentDraftSaveInput,
    outputSchema: agentDraftSaveOutput,
  },
  {
    id: 'agents.publish',
    label: 'Publish agent',
    description: 'Publish the saved draft as the agent\'s next numbered version. Earlier versions are kept unchanged; going back means publishing an older version again as a new number.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: agentRefInput,
    outputSchema: agentPublishOutput,
  },
  {
    id: 'agents.versions.list',
    label: 'List agent versions',
    description: 'List the published versions of one agent, newest first, with each version\'s instructions, tools, autonomy cap and who published it. Read only; the same people who may edit the agent may read it.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: agentRefInput,
    outputSchema: agentVersionsListOutput,
  },
  {
    id: 'agents.preview',
    label: 'Preview agent draft',
    description: 'Ask one question of an agent\'s saved draft in a temporary chat, to see how it answers before it is published. The preview runs in read-only mode with zero retention, and stores nothing (no thread, no transcript, no memory). It answers from the saved draft, not from unsaved edits on screen. Org admin, the agent\'s creator, or a manager of the room it was created in.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: agentPreviewInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'agents.admitted_actions.list',
    label: 'List admitted owner actions',
    description: 'List the owner actions this organization admits for agent runs, so a preset that needs one is offered only where it is admitted. Read only: it shows which actions are admitted, not what an agent may do with them.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: agentAdmittedActionsListInput,
    outputSchema: agentAdmittedActionsListOutput,
  },
  {
    id: 'plugins.list_catalog',
    label: 'List the plugin catalogue',
    description: 'Read the organization\'s plugin catalogue: the packages an admin listed that are still pinned to a source and not on the kill list, with their risk level and component summary. Any member may read it, zero data retention included. Read only; the agent is never offered it.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: pluginsListCatalogInput,
    outputSchema: pluginCatalogOutput,
  },
  {
    id: 'plugins.install',
    label: 'Install a plugin\'s components',
    description: 'An org admin consents to the exact component digests a plugin\'s install report lists, and the loader creates the skills, remote MCP servers and commands its manifest declares. Hooks, binaries, scripts and stdio servers are reported as not installed and never run. Refused under zero data retention, and the consent must match the manifest exactly. Admin only, acting as a person; the agent is never offered it.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: pluginInstallInput,
    outputSchema: pluginInstallOutput,
  },
  {
    id: 'plugins.uninstall',
    label: 'Uninstall a plugin',
    description: 'Remove every skill and remote MCP server one plugin installed, mark its components removed, and take the plugin out of service (disabled and not installed). Admin only, acting as a person; the agent is never offered it.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: pluginIdOnlyInput,
    outputSchema: pluginInstallOutput,
  },
  {
    id: 'plugins.approve_catalog',
    label: 'List or unlist a plugin in the catalogue',
    description: 'List one of the organization\'s pinned plugin packages in the org catalogue, or take it out again. Listing refuses a package that is not pinned to a source, has no digest, or whose digest is on the kill list. Admin only, acting as a person; the agent is never offered it.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: pluginCatalogApproveInput,
    outputSchema: pluginCatalogApproveOutput,
  },
  {
    id: 'plugins.kill_digest',
    label: 'Kill a plugin digest',
    description: 'Put one manifest digest on the organization\'s kill list. Every installed copy of it is uninstalled and its catalogue listing is cleared, in one transaction. Org scope only: a global kill needs a platform service principal and is not offered here. Not reversible from here, because what the kill removed stays removed. Admin only, acting as a person; the agent is never offered it.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: pluginKillDigestInput,
    outputSchema: pluginKillDigestOutput,
  },
  {
    id: 'plugins.lift_kill',
    label: 'Lift a plugin kill',
    description: 'Remove the organization\'s kill-list entry for one manifest digest, so the package can be installed and listed again. It restores nothing: what the kill uninstalled stays removed, and the package must be installed and listed again. Org scope only; a platform-wide kill is shown but not lifted here. Admin only, acting as a person; the removal is audited server-side. The agent is never offered it.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: pluginLiftKillInput,
    outputSchema: pluginKillRemovalOutput,
  },
] as const satisfies readonly ActionDescriptor[]

export type ActionId = (typeof actionRegistry)[number]['id']

export function getActionDescriptor(actionId: ActionId) {
  return actionRegistry.find((action) => action.id === actionId)
}

/**
 * E3.7.9a: a whole-site crawl above this many pages needs approval before it starts (owner
 * decision 2, 2026-10-23). The gateway's CRAWL_APPROVAL_PAGES may only lower it, so the
 * threshold the server applied (the 409 `approval_required` details carry it) is the one to
 * show a person.
 */
export const CRAWL_SITE_APPROVAL_PAGES = 50

/** The page ceiling the gateway applies to a crawl. A crawl with no bound is estimated at it. */
export const CRAWL_SITE_MAX_PAGES = 5000

/**
 * The page count a crawl is estimated to fetch: its bound, held to the ceiling. This is the
 * `pages` unit the E3.7.8 job price estimate takes for a crawl.
 */
export function crawlSiteEstimatedPages(maxPages: number | undefined): number {
  if (maxPages === undefined) return CRAWL_SITE_MAX_PAGES
  return Math.min(maxPages, CRAWL_SITE_MAX_PAGES)
}

/** Whether a knowledge.crawl_site request needs approval before it starts. */
export function crawlSiteRequiresApproval(input: unknown): boolean {
  const parsed = crawlSiteInput.safeParse(input)
  const maxPages = parsed.success ? parsed.data.maxPages : undefined
  return crawlSiteEstimatedPages(maxPages) > CRAWL_SITE_APPROVAL_PAGES
}

/**
 * Actions whose approval depends on their input rather than on the static flag. The flag is a
 * boolean and cannot carry the page threshold, so callers that decide approval ask
 * actionRequiresApproval, which consults this table.
 */
const conditionalApproval: Partial<Record<ActionId, (input: unknown) => boolean>> = {
  'knowledge.crawl_site': crawlSiteRequiresApproval,
}

export function actionRequiresApproval(actionId: ActionId, input: unknown): boolean {
  const descriptor = getActionDescriptor(actionId)
  if (!descriptor) throw new Error(`Unknown action: ${actionId}`)
  if (descriptor.requiresApproval) return true
  return conditionalApproval[actionId]?.(input) ?? false
}
