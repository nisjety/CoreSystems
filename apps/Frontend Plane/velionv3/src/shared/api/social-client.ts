import { requestJson } from '@/shared/api/http'

export type SocialProviderKey = 'linkedin' | 'x' | 'instagram' | 'facebook' | 'tiktok' | 'snapchat'
export type SocialPostStatus = 'draft' | 'pending_approval' | 'scheduled' | 'publishing' | 'published' | 'failed' | 'blocked'
export type SocialApprovalState = 'not_requested' | 'requested' | 'approved' | 'rejected' | 'pending' | 'not_required'
export type SocialCampaignStatus = 'draft' | 'active' | 'completed' | 'archived'

export type SocialAccount = {
  id: string
  providerKey: SocialProviderKey
  label: string
  handle: string
  status: 'connected' | 'needs_oauth' | 'manual_review' | 'disabled'
  capabilities: string[]
  accent: string
}

export type SocialPost = {
  id: string
  title: string
  body: string
  status: SocialPostStatus
  scheduledAt: string
  platforms: SocialProviderKey[]
  source: {
    kind: 'manual' | 'inbox' | 'knowledge' | 'campaign'
    label: string
    href?: string | null
  }
  approval: {
    required: boolean
    state: SocialApprovalState
  }
  media: Array<{
    kind: 'image' | 'video' | 'link'
    label: string
    status: 'draft' | 'ready' | 'failed'
  }>
  previews?: SocialPlatformPreview[]
}

export type SocialPlatformAdapter = {
  providerKey: SocialProviderKey
  label: string
  mode: 'direct_api' | 'media_container' | 'graph_pages_api' | 'content_posting_api' | 'marketing_api'
  endpoint: string
  maxCharacters: number
  mediaRequired: boolean
  requiredCapabilities: string[]
  notes: string[]
}

export type SocialPlatformPreview = {
  providerKey: SocialProviderKey
  label: string
  text: string
  characterCount: number
  maxCharacters: number
  mediaRequired: boolean
  ready: boolean
  warnings: string[]
}

export type RecommendedSocialWindow = {
  id: string
  label: string
  startsAt: string
  reason: string
}

export type SocialCalendar = {
  accounts: SocialAccount[]
  posts: SocialPost[]
  recommendedWindows: RecommendedSocialWindow[]
}

export type SocialPublishAttempt = {
  providerKey: SocialProviderKey
  label: string
  status: 'queued' | 'blocked'
  mode: SocialPlatformAdapter['mode']
  endpoint: string
  message: string
  externalId?: string | null
  warnings: string[]
}

export type SocialPublishResult = {
  id: string
  status: 'queued' | 'partial' | 'blocked'
  idempotencyKey: string
  attempts: SocialPublishAttempt[]
}

export type SocialPublishMutation = {
  post: SocialPost
  result: SocialPublishResult
}

export type SocialApprovalItem = {
  id: string
  postId: string
  campaignId: string
  state: SocialApprovalState | 'blocked'
  requestedByUserId?: string
  requestedOfUserId?: string
  decidedByUserId?: string
  decisionReason?: string
  dueAt?: string | null
  decidedAt?: string | null
  post?: SocialPost
  createdAt?: string
  updatedAt?: string
}

export type SocialApprovalDecisionInput = {
  decision: 'approved' | 'rejected'
  reason?: string
}

export type SocialCampaign = {
  id: string
  name: string
  brief: string
  goal: string
  status: SocialCampaignStatus
  platforms: SocialProviderKey[]
  startsAt?: string | null
  endsAt?: string | null
  source: SocialPost['source']
  ownerUserId?: string
  createdAt?: string
  updatedAt?: string
}

export type CreateSocialCampaignInput = {
  name: string
  brief?: string
  goal?: string
  status?: SocialCampaignStatus
  platforms?: SocialProviderKey[]
  startsAt?: string | null
  endsAt?: string | null
}

export type SocialCompetitorWatchItem = {
  id: string
  label: string
  providerKey: SocialProviderKey
  handle: string
  signal: string
  velocity: string
  capturedAt?: string | null
  status: 'watching' | 'captured' | 'endpoint_pending'
  sourceHref?: string | null
}

export type SocialTrendSignal = {
  id: string
  label: string
  providerKey: SocialProviderKey
  format: string
  opportunity: string
  velocity: string
  status: 'ready' | 'needs_media' | 'blocked' | 'endpoint_pending'
  sourceHref?: string | null
}

export type SocialEvergreenItem = {
  id: string
  title: string
  cadence: string
  lastPublishedAt?: string | null
  nextEligibleAt?: string | null
  guardrail: string
  status: 'ready' | 'needs_refresh' | 'needs_approval' | 'blocked'
  platforms: SocialProviderKey[]
  sourcePostId?: string | null
}

export type CreateSocialPostInput = {
  title: string
  body: string
  platforms: SocialProviderKey[]
  scheduledAt: string
}

export type CreateSocialDraftFromInboxInput = {
  ticketId: string
  ticketTitle: string
  customerName?: string
  channel?: string
  excerpt?: string
}

export function listSocialAccounts(orgId: string) {
  return requestJson<{ accounts: SocialAccount[] }>('/api/v1/social/accounts', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function listSocialAdapters(orgId: string) {
  return requestJson<{ adapters: SocialPlatformAdapter[] }>('/api/v1/social/adapters', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function listSocialPosts(orgId: string) {
  return requestJson<{ posts: SocialPost[] }>('/api/v1/social/posts', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function getSocialCalendar(orgId: string) {
  return requestJson<SocialCalendar>('/api/v1/social/calendar', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function createSocialPost(orgId: string, input: CreateSocialPostInput) {
  return requestJson<{ post: SocialPost }>('/api/v1/social/posts', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function createSocialDraftFromInbox(orgId: string, input: CreateSocialDraftFromInboxInput) {
  return requestJson<{ post: SocialPost }>('/api/v1/social/drafts/from-inbox', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function scheduleSocialPost(orgId: string, postId: string, scheduledAt: string) {
  return requestJson<{ post: SocialPost }>(`/api/v1/social/posts/${encodeURIComponent(postId)}/schedule`, {
    method: 'POST',
    body: JSON.stringify({ scheduledAt }),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function publishSocialPost(orgId: string, postId: string) {
  return requestJson<SocialPublishMutation>(`/api/v1/social/posts/${encodeURIComponent(postId)}/publish`, {
    method: 'POST',
    headers: { 'x-velion-org-id': orgId },
  })
}

export function listSocialApprovals(orgId: string) {
  return requestJson<{ approvals: SocialApprovalItem[] }>('/api/v1/social/approvals', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function decideSocialApproval(orgId: string, approvalId: string, input: SocialApprovalDecisionInput) {
  return requestJson<SocialApprovalItem>(`/api/v1/social/approvals/${encodeURIComponent(approvalId)}/decide`, {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function listSocialCampaigns(orgId: string) {
  return requestJson<{ campaigns: SocialCampaign[] }>('/api/v1/social/campaigns', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function createSocialCampaign(orgId: string, input: CreateSocialCampaignInput) {
  return requestJson<SocialCampaign>('/api/v1/social/campaigns', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function listSocialCompetitorWatch(orgId: string) {
  return requestJson<{ competitors: SocialCompetitorWatchItem[] }>('/api/v1/social/competitor-watch', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function listSocialTrends(orgId: string) {
  return requestJson<{ trends: SocialTrendSignal[] }>('/api/v1/social/trends', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function listSocialEvergreenItems(orgId: string) {
  return requestJson<{ items: SocialEvergreenItem[] }>('/api/v1/social/evergreen', {
    headers: { 'x-velion-org-id': orgId },
  })
}
