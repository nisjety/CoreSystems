import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

const sourceInput = z.object({
  sourceId: z.string().min(1),
})

const ticketInput = z.object({
  ticketId: z.string().min(1),
  responseTone: z.enum(['concise', 'warm', 'formal']),
})

const ticketCreateInput = z.object({
  conversationId: z.string().min(1),
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
  status: z.enum(['suggested', 'open', 'waiting_customer', 'waiting_team', 'escalated', 'resolved']).optional(),
  priority: z.enum(['low', 'normal', 'high', 'urgent']).optional(),
  severity: z.enum(['low', 'medium', 'high', 'critical']).optional(),
  category: z.string().trim().max(80).optional(),
  intent: z.string().trim().max(120).optional(),
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
  resourceKind: z.enum(['social_post', 'campaign', 'order', 'document', 'external']),
  resourceId: z.string().trim().optional(),
  resourceUrl: z.string().trim().optional(),
  label: z.string().trim().optional(),
})

const ticketResolveInput = z.object({
  ticketId: z.string().min(1),
  resolution: z.string().trim().max(500).optional(),
})

const agentInput = z.object({
  agentId: z.string().min(1),
  channel: z.enum(['chatbot', 'email', 'social']),
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

const scrapeUrlInput = z.object({
  url: z.string().min(1),
})

const crawlSiteInput = z.object({
  url: z.string().min(1),
  maxPages: z.number().int().positive().optional(),
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

const httpUrl = z.string().trim().url().refine(
  (value) => {
    try {
      const url = new URL(value)
      return url.protocol === 'http:' || url.protocol === 'https:'
    } catch {
      return false
    }
  },
  { message: 'URL must use http or https' },
)

const externalSecurityUrlInput = z.object({
  url: httpUrl,
  dataClass: z.enum(['public', 'organization', 'customer']).default('public'),
  reason: z.string().trim().min(1).max(280).optional(),
})

const urlReputationInput = externalSecurityUrlInput.extend({
  provider: z.literal('google_web_risk').default('google_web_risk'),
  purpose: z.enum(['ingestion_guard', 'manual_review', 'model_tool']).default('manual_review'),
  allowExternalLookup: z.literal(true),
})

const urlInvestigationInput = externalSecurityUrlInput.extend({
  provider: z.literal('urlscan_io').default('urlscan_io'),
  visibility: z.enum(['private', 'unlisted', 'public']).default('private'),
  tags: z.array(z.string().trim().min(1).max(48)).max(10).optional(),
  allowExternalSubmission: z.literal(true),
}).refine(
  (input) => input.dataClass === 'public' || input.visibility === 'private',
  {
    message: 'Non-public URLs require private scan visibility',
    path: ['visibility'],
  },
)

const runOutput = z.object({
  runId: z.string(),
  status: z.enum(['queued', 'planning', 'waiting_approval', 'executing', 'completed', 'failed']),
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

const urlReputationOutput = z.object({
  id: z.string(),
  checkedAt: z.string(),
  verdict: z.enum(['safe', 'suspicious', 'malicious', 'unknown']),
  provider: z.enum(['google_web_risk', 'local_feeds', 'policy_cache']).optional(),
  matches: z.array(z.object({
    provider: z.string(),
    threatType: z.enum(['malware', 'social_engineering', 'unwanted_software', 'potentially_harmful_application', 'unknown']),
    expiresAt: z.string().optional(),
  })).default([]),
  policy: z.object({
    externalLookupUsed: z.boolean(),
    nextAction: z.enum(['allow', 'warn', 'block', 'require_approval']),
  }).optional(),
})

const urlInvestigationOutput = z.object({
  id: z.string(),
  status: z.enum(['queued', 'submitted', 'running', 'completed', 'failed', 'blocked_by_policy']),
  provider: z.literal('urlscan_io'),
  visibility: z.enum(['private', 'unlisted', 'public']),
  submittedAt: z.string().optional(),
  resultUrl: z.string().url().optional(),
  verdict: z.enum(['safe', 'suspicious', 'malicious', 'unknown']).optional(),
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
    id: 'operating_map.generate',
    label: 'Generate Operating Map',
    description: 'Ask Model Plane to synthesize an evidence-grounded AI Operating Map proposal from Velion Knowledge.',
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
    id: 'security.check_url_reputation',
    label: 'Check URL reputation',
    description: 'Ask the gateway to check a URL against the approved Web Risk connector before crawl, import, or model use.',
    ownerPlane: 'ingestion',
    risk: 'medium',
    requiresApproval: true,
    reversible: false,
    inputSchema: urlReputationInput,
    outputSchema: urlReputationOutput,
  },
  {
    id: 'security.investigate_url',
    label: 'Investigate URL',
    description: 'Submit a URL to the approved urlscan.io connector with explicit visibility and audit controls.',
    ownerPlane: 'ingestion',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: urlInvestigationInput,
    outputSchema: urlInvestigationOutput,
  },
  {
    id: 'inbox.draft_reply',
    label: 'Draft reply',
    description: 'Ask Model Plane to draft a grounded customer support response.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: true,
    reversible: true,
    inputSchema: ticketInput,
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
    description: 'Update ticket status, priority, severity, category, or intent.',
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
    id: 'agents.deploy_channel',
    label: 'Deploy channel',
    description: 'Publish an approved agent role to a selected customer channel.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: agentInput,
    outputSchema: runOutput,
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
] as const satisfies readonly ActionDescriptor[]

export type ActionId = (typeof actionRegistry)[number]['id']

export function getActionDescriptor(actionId: ActionId) {
  return actionRegistry.find((action) => action.id === actionId)
}
