import { A } from '@solidjs/router'
import {
  AlertCircle,
  CheckCircle2,
  CheckCheck,
  Clock3,
  Megaphone,
  PenLine,
  Plug,
  Repeat2,
  Sparkles,
  Telescope,
  TrendingUp,
  type LucideProps,
} from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show, type Component } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import {
  fallbackSocialWorkspace,
  loadSocialWorkspace,
  platformLabels,
  type SocialWorkspace,
} from '@/features/social/lib/social-workspace'
import {
  createSocialCampaign,
  type SocialAccount,
  type SocialApprovalItem,
  type SocialCampaign,
  type SocialCompetitorWatchItem,
  type SocialEvergreenItem,
  type SocialPlatformAdapter,
  type SocialPost,
  type SocialProviderKey,
  type SocialTrendSignal,
} from '@/shared/api/social-client'
import { cn } from '@/shared/lib/cn'

export type SocialOperationsSection =
  | 'accounts'
  | 'drafts'
  | 'approvals'
  | 'campaigns'
  | 'competitors'
  | 'trends'
  | 'evergreen'

type SocialSectionConfig = {
  actionHref: string
  actionLabel: string
  description: string
  icon: Component<LucideProps>
  title: string
}

type SocialOpsMetric = {
  label: string
  value: string
}

type SocialOpsCard = {
  actionHref?: string
  actionLabel?: string
  detail: string
  meta: string
  status?: string
  title: string
}

const socialSections: Record<SocialOperationsSection, SocialSectionConfig> = {
  accounts: {
    title: 'Accounts',
    description: 'Channel readiness, OAuth state, capability coverage, and publishing constraints.',
    icon: Plug,
    actionHref: '/settings/integrations',
    actionLabel: 'Manage integrations',
  },
  drafts: {
    title: 'Drafts',
    description: 'A queue for Studio exports, inbox follow-ups, campaign variants, and platform rewrites.',
    icon: PenLine,
    actionHref: '/studio/canvas',
    actionLabel: 'Open Studio',
  },
  approvals: {
    title: 'Approvals',
    description: 'Human review queue for scheduled posts, generated media, and auto-action exceptions.',
    icon: CheckCheck,
    actionHref: '/social/calendar',
    actionLabel: 'Open calendar',
  },
  campaigns: {
    title: 'Campaigns',
    description: 'Social campaign plans with dates, channels, Studio boards, and active work ownership.',
    icon: Megaphone,
    actionHref: '/studio/campaigns',
    actionLabel: 'Plan in Studio',
  },
  competitors: {
    title: 'Competitor watch',
    description: 'Track accounts, breakout posts, creative patterns, and messages worth remixing.',
    icon: Telescope,
    actionHref: '/social/trends',
    actionLabel: 'Open trends',
  },
  trends: {
    title: 'Trends',
    description: 'Virals, saved hooks, reusable formats, channel timing, and Studio remix entry points.',
    icon: TrendingUp,
    actionHref: '/studio/canvas',
    actionLabel: 'Remix in Studio',
  },
  evergreen: {
    title: 'Evergreen queue',
    description: 'Reusable posts, recurring campaigns, active workflow links, and republish guardrails.',
    icon: Repeat2,
    actionHref: '/agents',
    actionLabel: 'Open workflows',
  },
}

export default function SocialOperationsPage(props: { section: SocialOperationsSection }) {
  const [workspace] = createResource(loadSocialWorkspace)
  const fallbackWorkspace = createMemo(() => fallbackSocialWorkspace())
  const currentWorkspace = createMemo(() => workspace() ?? fallbackWorkspace())
  const [createdCampaigns, setCreatedCampaigns] = createSignal<SocialCampaign[]>([])
  const [campaignName, setCampaignName] = createSignal('')
  const [campaignGoal, setCampaignGoal] = createSignal('')
  const [campaignBrief, setCampaignBrief] = createSignal('')
  const [campaignFeedback, setCampaignFeedback] = createSignal<string | null>(null)
  const [campaignBusy, setCampaignBusy] = createSignal(false)
  const effectiveWorkspace = createMemo<SocialWorkspace>(() => {
    const base = currentWorkspace()
    const localCampaigns = createdCampaigns()
    if (!localCampaigns.length) return base

    return {
      ...base,
      campaigns: mergeCampaigns(localCampaigns, base.campaigns),
      resources: { ...base.resources, campaigns: 'live' },
    }
  })
  const config = createMemo(() => socialSections[props.section])
  const resourceSource = createMemo(() => sourceForSection(props.section, effectiveWorkspace()))
  const sectionUsesDerivedData = createMemo(
    () => resourceSource() === 'fallback' && workspace()?.source !== 'fallback',
  )
  const metrics = createMemo(() => buildSectionMetrics(props.section, effectiveWorkspace()))
  const cards = createMemo(() => buildSectionCards(props.section, effectiveWorkspace()))

  const createCampaign = async (event: SubmitEvent) => {
    event.preventDefault()
    if (campaignBusy()) return

    const orgId = currentWorkspace().context.orgId
    const name = campaignName().trim()
    if (!orgId) {
      setCampaignFeedback('Campaign creation needs an organization-scoped social session.')
      return
    }
    if (!name) {
      setCampaignFeedback('Campaign name is required.')
      return
    }

    setCampaignBusy(true)
    setCampaignFeedback(null)
    try {
      const campaign = await createSocialCampaign(orgId, {
        name,
        brief: campaignBrief().trim(),
        goal: campaignGoal().trim(),
        status: 'draft',
        platforms: ['linkedin', 'x'],
      })
      setCreatedCampaigns((current) => [campaign, ...current.filter((item) => item.id !== campaign.id)])
      setCampaignName('')
      setCampaignGoal('')
      setCampaignBrief('')
      setCampaignFeedback('Campaign created.')
    } catch (reason) {
      setCampaignFeedback(reason instanceof Error ? reason.message : 'Campaign could not be created.')
    } finally {
      setCampaignBusy(false)
    }
  }

  return (
    <div class="velion-social-ops-page">
      <section class="velion-social-ops-hero">
        <div>
          <span class="velion-social-kicker">
            <Dynamic component={config().icon} size={14} />
            Social operations
          </span>
          <h1>{config().title}</h1>
          <p>{config().description}</p>
        </div>
        <A href={config().actionHref}>{config().actionLabel}</A>
      </section>

      <Show when={!workspace()}>
        <p class="velion-social-ops-state">
          <Clock3 size={16} />
          Loading organization-scoped social workspace...
        </p>
      </Show>

      <Show when={workspace()?.source === 'fallback'}>
        <p class="velion-social-ops-state velion-social-ops-state--warning">
          <AlertCircle size={16} />
          Showing fallback social data because the org-scoped social gateway is unavailable or no organization scope was resolved.
        </p>
      </Show>

      <Show when={sectionUsesDerivedData()}>
        <p class="velion-social-ops-state velion-social-ops-state--warning">
          <AlertCircle size={16} />
          Using calendar-derived {config().title.toLowerCase()} data until the dedicated social endpoint is available.
        </p>
      </Show>

      <section class="velion-social-ops-metrics" aria-label={`${config().title} metrics`}>
        <For each={metrics()}>
          {(metric) => (
            <article>
              <strong>{metric.value}</strong>
              <span>{metric.label}</span>
            </article>
          )}
        </For>
      </section>

      <Show when={props.section === 'campaigns'}>
        <form class="velion-social-ops-create" onSubmit={(event) => void createCampaign(event)}>
          <div>
            <h2>Create campaign</h2>
            <p>Draft campaign</p>
          </div>
          <label>
            <span>Name</span>
            <input value={campaignName()} onInput={(event) => setCampaignName(event.currentTarget.value)} />
          </label>
          <label>
            <span>Goal</span>
            <input value={campaignGoal()} onInput={(event) => setCampaignGoal(event.currentTarget.value)} />
          </label>
          <label>
            <span>Brief</span>
            <textarea rows={3} value={campaignBrief()} onInput={(event) => setCampaignBrief(event.currentTarget.value)} />
          </label>
          <button type="submit" disabled={campaignBusy()}>
            {campaignBusy() ? 'Creating...' : 'Create'}
          </button>
          <Show when={campaignFeedback()}>
            {(message) => <p class="velion-social-ops-create__feedback">{message()}</p>}
          </Show>
        </form>
      </Show>

      <section class="velion-social-ops-grid" aria-label={`${config().title} workspace`}>
        <For each={cards()}>
          {(card) => (
            <article class="velion-social-ops-card">
              <div>
                <Clock3 size={16} />
                <span>{card.meta}</span>
              </div>
              <h2>{card.title}</h2>
              <p>{card.detail}</p>
              <footer class="velion-social-ops-card__footer">
                <Show when={card.status}>
                  {(status) => <StatusPill status={status()} />}
                </Show>
                <Show when={card.actionHref}>
                  {(href) => <A href={href()}>{card.actionLabel ?? 'Open'}</A>}
                </Show>
              </footer>
            </article>
          )}
        </For>
      </section>

      <section class="velion-social-ops-next">
        <Sparkles size={18} />
        <div>
          <h2>System link</h2>
          <p>
            Accounts and posts are loaded from the org-scoped social API. Studio creates, Social schedules,
            Inbox converts conversations, Agents will automate evergreen workflows, and Insights measures outcomes.
          </p>
        </div>
      </section>
    </div>
  )
}

function buildSectionMetrics(
  section: SocialOperationsSection,
  workspace: SocialWorkspace,
): SocialOpsMetric[] {
  const accounts = workspace.calendar.accounts
  const posts = workspace.calendar.posts
  const adapters = workspace.adapters
  const connectedAccounts = accounts.filter((account) => account.status === 'connected').length
  const publishCapableAccounts = accounts.filter((account) => account.capabilities.includes('social.post.write')).length
  const draftPosts = posts.filter((post) => post.status === 'draft')
  const waitingApprovals = workspace.approvals.filter((approval) => approvalIsOpen(approval.state)).length
  const postsNeedingMedia = posts.filter((post) => postNeedsMedia(post, adapters)).length
  const supportedOrganicAdapters = adapters.filter((adapter) => adapter.requiredCapabilities.includes('social.post.write')).length

  switch (section) {
    case 'accounts':
      return [
        { label: 'Tracked providers', value: String(accounts.length) },
        { label: 'Connected', value: String(connectedAccounts) },
        { label: 'Publish capable', value: String(publishCapableAccounts) },
      ]
    case 'drafts':
      return [
        { label: 'Drafts', value: String(draftPosts.length) },
        { label: 'From inbox', value: String(posts.filter((post) => post.source.kind === 'inbox' && post.status === 'draft').length) },
        { label: 'Needs media', value: String(draftPosts.filter((post) => postNeedsMedia(post, adapters)).length) },
      ]
    case 'approvals':
      return [
        { label: 'Waiting', value: String(waitingApprovals) },
        { label: 'Approved', value: String(workspace.approvals.filter((approval) => approval.state === 'approved').length) },
        { label: 'Blocked', value: String(workspace.approvals.filter((approval) => approval.state === 'blocked').length) },
      ]
    case 'campaigns':
      return [
        { label: 'Campaigns', value: String(workspace.campaigns.length) },
        { label: 'Active', value: String(workspace.campaigns.filter((campaign) => campaign.status === 'active').length) },
        { label: 'Draft', value: String(workspace.campaigns.filter((campaign) => campaign.status === 'draft').length) },
      ]
    case 'competitors':
      return [
        { label: 'Watchlists', value: String(workspace.competitors.length) },
        { label: 'Endpoint', value: workspace.resources.competitors === 'live' ? 'Live' : 'Pending' },
        { label: 'Ready channels', value: String(connectedAccounts) },
      ]
    case 'trends':
      return [
        { label: 'Signals', value: String(workspace.trends.length) },
        { label: 'Ready', value: String(workspace.trends.filter((trend) => trend.status === 'ready').length) },
        { label: 'Needs media', value: String(postsNeedingMedia) },
      ]
    case 'evergreen':
      return [
        { label: 'Candidates', value: String(workspace.evergreen.length) },
        { label: 'Ready', value: String(workspace.evergreen.filter((item) => item.status === 'ready').length) },
        { label: 'Publish adapters', value: String(supportedOrganicAdapters) },
      ]
  }
}

function buildSectionCards(
  section: SocialOperationsSection,
  workspace: SocialWorkspace,
): SocialOpsCard[] {
  const calendar = workspace.calendar
  const adapters = workspace.adapters

  switch (section) {
    case 'accounts':
      return accountCards(calendar.accounts, adapters)
    case 'drafts':
      return postCards(
        calendar.posts.filter((post) => post.status === 'draft'),
        adapters,
        'No drafts yet',
        'Create in Studio or convert an inbox conversation to seed this queue.',
      )
    case 'approvals':
      return approvalCards(workspace.approvals)
    case 'campaigns':
      return campaignCards(workspace.campaigns)
    case 'competitors':
      return competitorCards(workspace.competitors)
    case 'trends':
      return trendCards(workspace.trends)
    case 'evergreen':
      return evergreenCards(workspace.evergreen)
  }
}

function accountCards(
  accounts: readonly SocialAccount[],
  adapters: readonly SocialPlatformAdapter[],
): SocialOpsCard[] {
  if (!accounts.length) {
    return [{
      title: 'No social accounts connected',
      meta: 'Integration required',
      detail: 'Connect LinkedIn, X, Instagram, Facebook, TikTok, or Snapchat under organization settings before publishing.',
      status: 'needs_oauth',
      actionHref: '/settings/integrations',
      actionLabel: 'Connect accounts',
    }]
  }

  return accounts.map((account) => {
    const adapter = adapterFor(adapters, account.providerKey)
    const missing = adapter?.requiredCapabilities.filter((capability) => !account.capabilities.includes(capability)) ?? []
    const readiness = missing.length ? `Missing ${missing.join(', ')}` : 'Required capabilities present'

    return {
      title: account.label,
      meta: `${platformLabels[account.providerKey]} · ${account.handle}`,
      detail: `${readiness}. ${adapter ? `${adapter.mode} via ${adapter.endpoint}.` : 'No adapter registered.'}`,
      status: account.status,
      actionHref: account.status === 'connected' ? '/social/calendar' : '/settings/integrations',
      actionLabel: account.status === 'connected' ? 'Use in calendar' : 'Connect',
    }
  })
}

function postCards(
  posts: readonly SocialPost[],
  adapters: readonly SocialPlatformAdapter[],
  emptyTitle: string,
  emptyDetail: string,
): SocialOpsCard[] {
  if (!posts.length) {
    return [{
      title: emptyTitle,
      meta: 'Queue empty',
      detail: emptyDetail,
      status: 'scheduled',
      actionHref: '/social/calendar',
      actionLabel: 'Open calendar',
    }]
  }

  return posts.map((post) => ({
    title: post.title,
    meta: `${post.source.label} · ${platformNames(post.platforms)}`,
    detail: postNeedsMedia(post, adapters)
      ? `${post.body} Media-first platforms need a ready image or video before publish.`
      : post.body,
    status: post.status,
    actionHref: post.source.href ?? '/social/calendar',
    actionLabel: post.source.href ? 'Open source' : 'Open calendar',
  }))
}

function approvalCards(approvals: readonly SocialApprovalItem[]): SocialOpsCard[] {
  if (!approvals.length) {
    return [{
      title: 'No approval blockers',
      meta: 'Queue empty',
      detail: 'Posts that require human review, media fixes, or policy decisions will appear here.',
      status: 'approved',
      actionHref: '/social/calendar',
      actionLabel: 'Open calendar',
    }]
  }

  return approvals.map((approval) => ({
    title: approval.post?.title ?? `Approval ${approval.id}`,
    meta: `${approval.campaignId ? 'Campaign review' : 'Post review'} · ${formatOptionalDate(approval.dueAt)}`,
    detail: `${approval.decisionReason || approvalDetail(approval)} Source: ${approval.post?.source.label ?? 'Social approval queue'}. Channels: ${platformNames(approval.post?.platforms ?? [])}.`,
    status: approval.state,
    actionHref: approval.post?.source.href ?? '/social/calendar',
    actionLabel: approval.post?.source.href ? 'Open source' : 'Review post',
  }))
}

function campaignCards(campaigns: readonly SocialCampaign[]): SocialOpsCard[] {
  if (!campaigns.length) {
    return [{
      title: 'No campaign posts connected',
      meta: 'Calendar-derived queue',
      detail: 'Plan campaign packs in Studio, then send posts into Social drafts and approvals. The page consumes /api/v1/social/campaigns when available.',
      status: 'draft',
      actionHref: '/social/drafts',
      actionLabel: 'Open drafts',
    }]
  }

  return campaigns.map((campaign) => ({
    title: campaign.name,
    meta: `${platformNames(campaign.platforms)} · ${campaign.status}`,
    detail: `${campaign.goal || 'Coordinate social campaign posts across selected channels.'} Window: ${formatOptionalDate(campaign.startsAt)} to ${formatOptionalDate(campaign.endsAt)}. ${campaign.brief}`,
    status: campaign.status,
    actionHref: campaign.source.href ?? '/social/calendar',
    actionLabel: campaign.source.href ? 'Open source' : 'Open calendar',
  }))
}

function competitorCards(competitors: readonly SocialCompetitorWatchItem[]): SocialOpsCard[] {
  return competitors.map((competitor) => ({
    title: competitor.label,
    meta: `${platformLabels[competitor.providerKey]} · ${competitor.velocity}`,
    detail: `${competitor.signal} Target: ${competitor.handle}.`,
    status: competitor.status,
    actionHref: competitor.sourceHref ?? '/social/trends',
    actionLabel: competitor.status === 'endpoint_pending' ? 'Prepare trends' : 'Open signal',
  }))
}

function trendCards(trends: readonly SocialTrendSignal[]): SocialOpsCard[] {
  if (!trends.length) {
    return [{
      title: 'No trend signals yet',
      meta: 'Endpoint ready',
      detail: 'Platform warnings, adapter formats, and captured trend opportunities will appear here.',
      status: 'endpoint_pending',
      actionHref: '/social/drafts',
      actionLabel: 'Open drafts',
    }]
  }

  return trends.map((trend) => ({
    title: trend.label,
    meta: `${platformLabels[trend.providerKey]} · ${trend.velocity}`,
    detail: `${trend.opportunity} Format: ${trend.format}.`,
    status: trend.status,
    actionHref: trend.sourceHref ?? '/social/drafts',
    actionLabel: 'Use signal',
  }))
}

function evergreenCards(items: readonly SocialEvergreenItem[]): SocialOpsCard[] {
  return items.map((item) => ({
    title: item.title,
    meta: `${item.cadence} · ${platformNames(item.platforms)}`,
    detail: `${item.guardrail} Next eligible: ${formatOptionalDate(item.nextEligibleAt)}.`,
    status: item.status,
    actionHref: item.sourcePostId ? '/social/calendar' : '/agents',
    actionLabel: item.sourcePostId ? 'Open source post' : 'Open workflows',
  }))
}

function adapterFor(adapters: readonly SocialPlatformAdapter[], providerKey: SocialProviderKey) {
  return adapters.find((adapter) => adapter.providerKey === providerKey)
}

function postNeedsMedia(post: SocialPost, adapters: readonly SocialPlatformAdapter[]) {
  const hasReadyMedia = post.media.some((asset) => asset.status === 'ready')
  if (hasReadyMedia) return false
  return post.platforms.some((platform) => adapterFor(adapters, platform)?.mediaRequired)
}

function approvalIsOpen(state: SocialApprovalItem['state']) {
  return !['approved', 'rejected', 'not_required'].includes(state)
}

function platformNames(platforms: readonly SocialProviderKey[]) {
  return platforms.map((platform) => platformLabels[platform]).join(', ')
}

function approvalDetail(approval: SocialApprovalItem) {
  if (approval.state === 'blocked') return 'Resolve the blocked post before approval can continue.'
  if (approval.post && postHasMissingMedia(approval.post)) return 'Ready media is required before this post can move through approval.'
  return 'Human review is required before schedule or publish.'
}

function mergeCampaigns(created: readonly SocialCampaign[], existing: readonly SocialCampaign[]) {
  const seen = new Set<string>()
  return [...created, ...existing].filter((campaign) => {
    if (seen.has(campaign.id)) return false
    seen.add(campaign.id)
    return true
  })
}

function postHasMissingMedia(post: SocialPost) {
  const hasReadyMedia = post.media.some((asset) => asset.status === 'ready')
  if (hasReadyMedia) return false
  return post.platforms.some((platform) => ['instagram', 'tiktok', 'snapchat'].includes(platform))
}

function sourceForSection(section: SocialOperationsSection, workspace: SocialWorkspace) {
  switch (section) {
    case 'accounts':
    case 'drafts':
      return workspace.resources.calendar
    case 'approvals':
      return workspace.resources.approvals
    case 'campaigns':
      return workspace.resources.campaigns
    case 'competitors':
      return workspace.resources.competitors
    case 'trends':
      return workspace.resources.trends
    case 'evergreen':
      return workspace.resources.evergreen
  }
}

function formatOptionalDate(iso?: string | null) {
  if (!iso) return 'not scheduled'
  return new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })
}

function StatusPill(props: { status: string }) {
  return (
    <span class={cn('velion-social-status', `velion-social-status--${props.status.replace(/_/g, '-')}`)}>
      <CheckCircle2 class="size-3.5" />
      {props.status.replace(/_/g, ' ')}
    </span>
  )
}
