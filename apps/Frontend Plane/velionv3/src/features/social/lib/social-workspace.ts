import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import {
  getSocialCalendar,
  listSocialApprovals,
  listSocialAdapters,
  listSocialCampaigns,
  listSocialCompetitorWatch,
  listSocialEvergreenItems,
  listSocialTrends,
  type SocialApprovalItem,
  type SocialCalendar,
  type SocialCampaign,
  type SocialCompetitorWatchItem,
  type SocialEvergreenItem,
  type SocialPlatformAdapter,
  type SocialPost,
  type SocialProviderKey,
  type SocialTrendSignal,
} from '@/shared/api/social-client'

export const platformLabels: Record<SocialProviderKey, string> = {
  linkedin: 'LinkedIn',
  x: 'X',
  instagram: 'Instagram',
  facebook: 'Facebook',
  tiktok: 'TikTok',
  snapchat: 'Snapchat',
}

export const composerPlatforms: SocialProviderKey[] = ['linkedin', 'x', 'instagram', 'facebook', 'tiktok', 'snapchat']

export type SocialWorkspaceContext = {
  email: string
  name: string
  orgId: string
  orgLabel: string
}

export type SocialResourceSource = 'live' | 'fallback'

export type SocialWorkspaceResources = {
  adapters: SocialResourceSource
  approvals: SocialResourceSource
  calendar: SocialResourceSource
  campaigns: SocialResourceSource
  competitors: SocialResourceSource
  evergreen: SocialResourceSource
  trends: SocialResourceSource
}

export type SocialWorkspace = {
  adapters: SocialPlatformAdapter[]
  approvals: SocialApprovalItem[]
  calendar: SocialCalendar
  campaigns: SocialCampaign[]
  competitors: SocialCompetitorWatchItem[]
  context: SocialWorkspaceContext
  evergreen: SocialEvergreenItem[]
  resources: SocialWorkspaceResources
  source: SocialResourceSource
  trends: SocialTrendSignal[]
}

type ResourceResult<T> = {
  data: T
  source: SocialResourceSource
}

export async function loadSocialContext(): Promise<SocialWorkspaceContext> {
  const [session, ctx] = await Promise.all([getAuthSession(), getSessionContext().catch(() => null)])
  const activeOrg = ctx?.orgs[0] ?? null

  return {
    email: session?.user.email ?? ctx?.email ?? '',
    name: session?.user.name ?? ctx?.name ?? '',
    orgId: activeOrg?.id ?? ctx?.orgId ?? '',
    orgLabel: activeOrg?.name ?? ctx?.orgId ?? 'Velion',
  }
}

export async function loadSocialCalendar(orgId: string, orgLabel = 'Velion'): Promise<SocialCalendar> {
  return (await readSocialCalendar(orgId, orgLabel)).data
}

export async function loadSocialCalendarResource(orgId: string, orgLabel = 'Velion'): Promise<ResourceResult<SocialCalendar>> {
  return readSocialCalendar(orgId, orgLabel)
}

export async function loadSocialAdapters(orgId: string): Promise<SocialPlatformAdapter[]> {
  return (await readSocialAdapters(orgId)).data
}

export async function loadSocialWorkspace(): Promise<SocialWorkspace> {
  const context = await loadSocialContext()
  const [calendar, adapters] = await Promise.all([
    readSocialCalendar(context.orgId, context.orgLabel),
    readSocialAdapters(context.orgId),
  ])
  const fallbackOperations = fallbackSocialOperations(calendar.data, adapters.data)
  const [approvals, campaigns, competitors, trends, evergreen] = await Promise.all([
    readOptionalSocialResource(context.orgId, () => listSocialApprovals(context.orgId).then((result) => result.approvals), fallbackOperations.approvals),
    readOptionalSocialResource(context.orgId, () => listSocialCampaigns(context.orgId).then((result) => result.campaigns), fallbackOperations.campaigns),
    readOptionalSocialResource(context.orgId, () => listSocialCompetitorWatch(context.orgId).then((result) => result.competitors), fallbackOperations.competitors),
    readOptionalSocialResource(context.orgId, () => listSocialTrends(context.orgId).then((result) => result.trends), fallbackOperations.trends),
    readOptionalSocialResource(context.orgId, () => listSocialEvergreenItems(context.orgId).then((result) => result.items), fallbackOperations.evergreen),
  ])
  const resources = {
    adapters: adapters.source,
    approvals: approvals.source,
    calendar: calendar.source,
    campaigns: campaigns.source,
    competitors: competitors.source,
    evergreen: evergreen.source,
    trends: trends.source,
  }

  return {
    adapters: adapters.data,
    approvals: approvals.data,
    calendar: calendar.data,
    campaigns: campaigns.data,
    competitors: competitors.data,
    context,
    evergreen: evergreen.data,
    resources,
    source: calendar.source === 'live' && adapters.source === 'live' ? 'live' : 'fallback',
    trends: trends.data,
  }
}

async function readSocialCalendar(orgId: string, orgLabel: string): Promise<ResourceResult<SocialCalendar>> {
  if (!orgId.trim()) {
    return { data: fallbackSocialCalendar(orgLabel), source: 'fallback' }
  }

  try {
    return { data: await getSocialCalendar(orgId), source: 'live' }
  } catch {
    return { data: fallbackSocialCalendar(orgLabel), source: 'fallback' }
  }
}

async function readSocialAdapters(orgId: string): Promise<ResourceResult<SocialPlatformAdapter[]>> {
  if (!orgId.trim()) {
    return { data: fallbackSocialAdapters(), source: 'fallback' }
  }

  try {
    const result = await listSocialAdapters(orgId)
    return { data: result.adapters, source: 'live' }
  } catch {
    return { data: fallbackSocialAdapters(), source: 'fallback' }
  }
}

async function readOptionalSocialResource<T>(
  orgId: string,
  load: () => Promise<T>,
  fallback: T,
): Promise<ResourceResult<T>> {
  if (!orgId.trim()) {
    return { data: fallback, source: 'fallback' }
  }

  try {
    return { data: await load(), source: 'live' }
  } catch {
    return { data: fallback, source: 'fallback' }
  }
}

export function fallbackSocialWorkspace(orgLabel = 'Velion'): SocialWorkspace {
  const calendar = fallbackSocialCalendar(orgLabel)
  const adapters = fallbackSocialAdapters()
  const operations = fallbackSocialOperations(calendar, adapters)
  const resources: SocialWorkspaceResources = {
    adapters: 'fallback',
    approvals: 'fallback',
    calendar: 'fallback',
    campaigns: 'fallback',
    competitors: 'fallback',
    evergreen: 'fallback',
    trends: 'fallback',
  }

  return {
    ...operations,
    adapters,
    calendar,
    context: {
      email: '',
      name: '',
      orgId: '',
      orgLabel,
    },
    resources,
    source: 'fallback',
  }
}

export function fallbackSocialOperations(
  calendar: SocialCalendar,
  adapters: readonly SocialPlatformAdapter[],
): Pick<SocialWorkspace, 'approvals' | 'campaigns' | 'competitors' | 'evergreen' | 'trends'> {
  void calendar
  void adapters

  return {
    approvals: [],
    campaigns: [],
    competitors: [],
    evergreen: [],
    trends: [],
  }
}

export function fallbackSocialApprovals(calendar: SocialCalendar): SocialApprovalItem[] {
  return calendar.posts
    .filter((post) => approvalNeedsReview(post) || post.status === 'blocked' || post.status === 'failed')
    .map((post) => {
      const blocked = post.status === 'blocked' || post.status === 'failed'
      return {
        id: `approval_${post.id}`,
        postId: post.id,
        campaignId: '',
        state: blocked ? 'blocked' : post.approval.state === 'not_requested' ? 'requested' : post.approval.state,
        requestedByUserId: '',
        requestedOfUserId: '',
        decidedByUserId: '',
        decisionReason: '',
        dueAt: post.scheduledAt,
        decidedAt: null,
        post: {
          ...post,
          approval: { ...post.approval },
          media: post.media.map((asset) => ({ ...asset })),
          platforms: [...post.platforms],
          previews: (post.previews ?? []).map((preview) => ({ ...preview, warnings: [...preview.warnings] })),
          source: { ...post.source },
        },
        createdAt: post.scheduledAt,
        updatedAt: post.scheduledAt,
      }
    })
}

export function fallbackSocialCampaigns(calendar: SocialCalendar): SocialCampaign[] {
  const grouped = calendar.posts.reduce<Record<string, SocialPost[]>>((groups, post) => {
    if (post.source.kind !== 'campaign') return groups
    const label = post.source.label || 'Campaign'
    return { ...groups, [label]: [...(groups[label] ?? []), post] }
  }, {})

  return Object.entries(grouped).map(([name, posts]) => ({
    id: `campaign_${slugId(name)}`,
    name,
    brief: posts[0]?.body ?? '',
    goal: `${posts.length} social ${posts.length === 1 ? 'post' : 'posts'} connected to this campaign.`,
    status: campaignStatus(posts),
    platforms: uniquePlatforms(posts.flatMap((post) => post.platforms)),
    startsAt: firstIso(posts.map((post) => post.scheduledAt)),
    endsAt: lastIso(posts.map((post) => post.scheduledAt)),
    source: {
      kind: 'campaign',
      label: posts[0]?.source.label ?? 'Social calendar',
      href: posts.find((post) => post.source.href)?.source.href ?? null,
    },
    ownerUserId: '',
    createdAt: firstIso(posts.map((post) => post.scheduledAt)) ?? undefined,
    updatedAt: lastIso(posts.map((post) => post.scheduledAt)) ?? undefined,
  }))
}

export function fallbackSocialCompetitorWatch(accounts: readonly SocialCalendar['accounts'][number][]): SocialCompetitorWatchItem[] {
  if (!accounts.length) {
    return [{
      id: 'competitor_watch_pending',
      label: 'Watchlist endpoint pending',
      providerKey: 'linkedin',
      handle: 'No organization accounts resolved',
      signal: 'Connect social accounts first; competitor watch will inherit the same org boundary.',
      velocity: 'Unavailable',
      capturedAt: null,
      status: 'endpoint_pending',
      sourceHref: '/settings/integrations',
    }]
  }

  return accounts.slice(0, 3).map((account) => ({
    id: `competitor_watch_${account.providerKey}`,
    label: `${platformLabels[account.providerKey]} competitor lane`,
    providerKey: account.providerKey,
    handle: account.handle,
    signal: account.status === 'connected'
      ? 'Ready to attach watched accounts once the competitor-watch endpoint lands.'
      : 'Account connection needs attention before this channel can track competitors.',
    velocity: account.status === 'connected' ? 'Ready' : 'Blocked',
    capturedAt: null,
    status: 'endpoint_pending',
    sourceHref: account.status === 'connected' ? '/social/trends' : '/settings/integrations',
  }))
}

export function fallbackSocialTrends(
  posts: readonly SocialPost[],
  adapters: readonly SocialPlatformAdapter[],
): SocialTrendSignal[] {
  const warningSignals = posts
    .flatMap((post) => (post.previews ?? []).flatMap((preview) =>
      preview.warnings.map((warning, index) => ({
        id: `trend_${post.id}_${preview.providerKey}_${index}`,
        label: `${preview.label} rewrite signal`,
        providerKey: preview.providerKey,
        format: `${preview.characterCount}/${preview.maxCharacters} chars`,
        opportunity: warning,
        velocity: preview.ready ? 'Ready' : 'Needs rewrite',
        status: preview.ready ? 'ready' as const : 'blocked' as const,
        sourceHref: post.source.href ?? '/social/calendar',
      })),
    ))
    .slice(0, 4)

  if (warningSignals.length) return warningSignals

  return adapters.slice(0, 4).map((adapter) => ({
    id: `trend_adapter_${adapter.providerKey}`,
    label: `${adapter.label} format rules`,
    providerKey: adapter.providerKey,
    format: `${adapter.maxCharacters} chars · ${adapter.mode}`,
    opportunity: `${adapter.notes[0] ?? 'Adapter rules are ready for format-aware rewriting.'}${adapter.mediaRequired ? ' Requires ready media.' : ''}`,
    velocity: adapter.mediaRequired ? 'Media-first' : 'Copy-ready',
    status: adapter.mediaRequired ? 'needs_media' : 'ready',
    sourceHref: '/social/drafts',
  }))
}

export function fallbackSocialEvergreen(
  posts: readonly SocialPost[],
  adapters: readonly SocialPlatformAdapter[],
): SocialEvergreenItem[] {
  const reusable = posts.filter((post) => post.source.kind === 'knowledge' || post.status === 'scheduled' || post.status === 'published')

  if (!reusable.length) {
    return [{
      id: 'evergreen_seed',
      title: 'Knowledge-backed evergreen backlog',
      cadence: 'Monthly',
      lastPublishedAt: null,
      nextEligibleAt: null,
      guardrail: 'Queue activates after posts prove useful or originate from durable knowledge.',
      status: 'needs_refresh',
      platforms: ['linkedin'],
      sourcePostId: null,
    }]
  }

  return reusable.slice(0, 4).map((post) => ({
    id: `evergreen_${post.id}`,
    title: post.title,
    cadence: post.status === 'published' ? 'Quarterly' : 'Monthly',
    lastPublishedAt: post.status === 'published' ? post.scheduledAt : null,
    nextEligibleAt: post.scheduledAt,
    guardrail: postNeedsMedia(post, adapters)
      ? 'Refresh media assets and run approval before reuse.'
      : 'Run freshness, duplication, and approval checks before republish.',
    status: postNeedsMedia(post, adapters) ? 'needs_refresh' : post.approval.state === 'approved' ? 'ready' : 'needs_approval',
    platforms: [...post.platforms],
    sourcePostId: post.id,
  }))
}

function adapterFor(adapters: readonly SocialPlatformAdapter[], providerKey: SocialProviderKey) {
  return adapters.find((adapter) => adapter.providerKey === providerKey)
}

function postNeedsMedia(post: SocialPost, adapters: readonly SocialPlatformAdapter[]) {
  if (post.media.some((asset) => asset.status === 'ready')) return false
  return post.platforms.some((platform) => adapterFor(adapters, platform)?.mediaRequired)
}

function approvalNeedsReview(post: SocialPost) {
  return post.approval.required && !['approved', 'rejected', 'not_required'].includes(post.approval.state)
}

function campaignStatus(posts: readonly SocialPost[]): SocialCampaign['status'] {
  if (posts.length > 0 && posts.every((post) => post.status === 'published')) return 'completed'
  if (posts.some((post) => ['scheduled', 'publishing', 'published'].includes(post.status))) return 'active'
  return 'draft'
}

function uniquePlatforms(platforms: readonly SocialProviderKey[]) {
  return Array.from(new Set(platforms))
}

function firstIso(values: readonly string[]) {
  return sortedIso(values)[0] ?? null
}

function lastIso(values: readonly string[]) {
  const sorted = sortedIso(values)
  return sorted.at(-1) ?? null
}

function sortedIso(values: readonly string[]) {
  return [...values].filter(Boolean).sort((left, right) => left.localeCompare(right))
}

function slugId(value: string) {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '') || 'social_campaign'
}

export function fallbackSocialCalendar(orgLabel = 'Velion'): SocialCalendar {
  void orgLabel

  return {
    accounts: [],
    posts: [],
    recommendedWindows: [],
  }
}

export function fallbackSocialAdapters(): SocialPlatformAdapter[] {
  return [
    {
      providerKey: 'linkedin',
      label: 'LinkedIn',
      mode: 'direct_api',
      endpoint: 'POST /rest/posts',
      maxCharacters: 3000,
      mediaRequired: false,
      requiredCapabilities: ['social.profile.read', 'social.post.write'],
      notes: [
        "Create organization or member posts through LinkedIn's Posts API.",
        'Image and video posts require a media upload URN before post creation.',
      ],
    },
    {
      providerKey: 'x',
      label: 'X',
      mode: 'direct_api',
      endpoint: 'POST /2/tweets',
      maxCharacters: 280,
      mediaRequired: false,
      requiredCapabilities: ['social.profile.read', 'social.post.write'],
      notes: [
        'Text posts use X API v2 manage Posts endpoints.',
        'Longer drafts should become threads or be shortened before publish.',
      ],
    },
    {
      providerKey: 'instagram',
      label: 'Instagram',
      mode: 'media_container',
      endpoint: 'POST /{ig-user-id}/media + POST /{ig-user-id}/media_publish',
      maxCharacters: 2200,
      mediaRequired: true,
      requiredCapabilities: ['social.profile.read', 'social.post.write', 'social.media.upload'],
      notes: [
        'Content publishing creates a media container, then publishes that container.',
        'Feed, Reels, Stories, and carousel posts require approved Meta app access.',
      ],
    },
    {
      providerKey: 'facebook',
      label: 'Facebook',
      mode: 'graph_pages_api',
      endpoint: 'POST /{page-id}/feed or /{page-id}/photos',
      maxCharacters: 63206,
      mediaRequired: false,
      requiredCapabilities: ['social.profile.read', 'social.post.write', 'social.media.upload'],
      notes: [
        'Page publishing uses Meta Graph API and requires a connected Facebook Page.',
        'Photo posts are sent through the Page photos endpoint when ready media is present.',
      ],
    },
    {
      providerKey: 'tiktok',
      label: 'TikTok',
      mode: 'content_posting_api',
      endpoint: 'POST /v2/post/publish/content/init/',
      maxCharacters: 2200,
      mediaRequired: true,
      requiredCapabilities: ['social.profile.read', 'social.post.write', 'social.media.upload'],
      notes: [
        'Direct Post requires creator info first so the UI can render TikTok posting options.',
        'Video or photo media must be transferred to TikTok before publish completion.',
      ],
    },
    {
      providerKey: 'snapchat',
      label: 'Snapchat',
      mode: 'marketing_api',
      endpoint: 'Ads/creative workflows, not organic post publishing',
      maxCharacters: 250,
      mediaRequired: false,
      requiredCapabilities: ['social.profile.read', 'social.ads.manage'],
      notes: [
        'Snapchat is available for marketing, creative, campaign, and reporting workflows.',
        'Organic Story/Spotlight publishing is intentionally blocked until an approved API path exists.',
      ],
    },
  ]
}
