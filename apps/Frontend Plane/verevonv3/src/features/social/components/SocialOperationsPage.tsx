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
} from '@/shared/icons'
import { createMemo, createSignal, For, Show, type Component } from 'solid-js'
import { Dynamic } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
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
import { useI18n } from '@/shared/i18n'

type TrFn = (noText: string, enText: string) => string

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

function socialSections(tr: TrFn): Record<SocialOperationsSection, SocialSectionConfig> {
  return {
    accounts: {
      title: tr('Kontoer', 'Accounts'),
      description: tr('Kanalberedskap, OAuth-status, funksjonsdekning og publiseringsbegrensninger.', 'Channel readiness, OAuth state, capability coverage, and publishing constraints.'),
      icon: Plug,
      actionHref: '/settings/integrations',
      actionLabel: tr('Administrer integrasjoner', 'Manage integrations'),
    },
    drafts: {
      title: tr('Utkast', 'Drafts'),
      description: tr('En kø for Studio-eksporter, innboksoppfølginger, kampanjevarianter og plattformomskrivinger.', 'A queue for Studio exports, inbox follow-ups, campaign variants, and platform rewrites.'),
      icon: PenLine,
      actionHref: '/studio/canvas',
      actionLabel: tr('Åpne Studio', 'Open Studio'),
    },
    approvals: {
      title: tr('Godkjenninger', 'Approvals'),
      description: tr('Manuell gjennomgangskø for planlagte innlegg, generert media og unntak for automatiske handlinger.', 'Human review queue for scheduled posts, generated media, and auto-action exceptions.'),
      icon: CheckCheck,
      actionHref: '/social/calendar',
      actionLabel: tr('Åpne kalender', 'Open calendar'),
    },
    campaigns: {
      title: tr('Kampanjer', 'Campaigns'),
      description: tr('Sosiale kampanjeplaner med datoer, kanaler, Studio-tavler og aktivt arbeidseierskap.', 'Social campaign plans with dates, channels, Studio boards, and active work ownership.'),
      icon: Megaphone,
      actionHref: '/studio/campaigns',
      actionLabel: tr('Planlegg i Studio', 'Plan in Studio'),
    },
    competitors: {
      title: tr('Konkurrentovervåking', 'Competitor watch'),
      description: tr('Spor kontoer, gjennombruddsinnlegg, kreative mønstre og meldinger verdt å gjenbruke.', 'Track accounts, breakout posts, creative patterns, and messages worth remixing.'),
      icon: Telescope,
      actionHref: '/social/trends',
      actionLabel: tr('Åpne trender', 'Open trends'),
    },
    trends: {
      title: tr('Trender', 'Trends'),
      description: tr('Virale innlegg, lagrede kroker, gjenbrukbare formater, kanaltiming og Studio-remiks-innganger.', 'Virals, saved hooks, reusable formats, channel timing, and Studio remix entry points.'),
      icon: TrendingUp,
      actionHref: '/studio/canvas',
      actionLabel: tr('Remiks i Studio', 'Remix in Studio'),
    },
    evergreen: {
      title: tr('Eviggrønn kø', 'Evergreen queue'),
      description: tr('Gjenbrukbare innlegg, tilbakevendende kampanjer, aktive arbeidsflytlenker og republiseringssperrer.', 'Reusable posts, recurring campaigns, active workflow links, and republish guardrails.'),
      icon: Repeat2,
      actionHref: '/agents',
      actionLabel: tr('Åpne arbeidsflyter', 'Open workflows'),
    },
  }
}

export default function SocialOperationsPage(props: { section: SocialOperationsSection }) {
  const i18n = useI18n()
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
  const config = createMemo(() => socialSections(i18n.tr)[props.section])
  const resourceSource = createMemo(() => sourceForSection(props.section, effectiveWorkspace()))
  const sectionUsesDerivedData = createMemo(
    () => resourceSource() === 'fallback' && workspace()?.source !== 'fallback',
  )
  const metrics = createMemo(() => buildSectionMetrics(props.section, effectiveWorkspace(), i18n.tr))
  const cards = createMemo(() => buildSectionCards(props.section, effectiveWorkspace(), i18n.tr))

  const createCampaign = async (event: SubmitEvent) => {
    event.preventDefault()
    if (campaignBusy()) return

    const orgId = currentWorkspace().context.orgId
    const name = campaignName().trim()
    if (!orgId) {
      setCampaignFeedback(i18n.tr('Kampanjeoppretting krever en organisasjonsscopet sosial økt.', 'Campaign creation needs an organization-scoped social session.'))
      return
    }
    if (!name) {
      setCampaignFeedback(i18n.tr('Kampanjenavn er påkrevd.', 'Campaign name is required.'))
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
      setCampaignFeedback(i18n.tr('Kampanje opprettet.', 'Campaign created.'))
    } catch (reason) {
      setCampaignFeedback(reason instanceof Error ? reason.message : i18n.tr('Kampanjen kunne ikke opprettes.', 'Campaign could not be created.'))
    } finally {
      setCampaignBusy(false)
    }
  }

  return (
    <div class="verevon-social-ops-page">
      <section class="verevon-social-ops-hero">
        <div>
          <span class="verevon-social-kicker">
            <Dynamic component={config().icon} size={14} />
            {i18n.tr('Sosiale operasjoner', 'Social operations')}
          </span>
          <h1>{config().title}</h1>
          <p>{config().description}</p>
        </div>
        <a href={config().actionHref} link>{config().actionLabel}</a>
      </section>

      <Show when={!workspace()}>
        <p class="verevon-social-ops-state">
          <Clock3 size={16} />
          {i18n.tr('Laster organisasjonsscopet sosialt arbeidsområde …', 'Loading organization-scoped social workspace...')}
        </p>
      </Show>

      <Show when={workspace()?.source === 'fallback'}>
        <p class="verevon-social-ops-state verevon-social-ops-state--warning">
          <AlertCircle size={16} />
          {i18n.tr('Sosiale poster er utilgjengelige fordi den org-scopede sosiale gatewayen er utilgjengelig, eller ingen organisasjonsscope ble løst.', 'Social records are unavailable because the org-scoped social gateway is unavailable or no organization scope was resolved.')}
        </p>
      </Show>

      <Show when={sectionUsesDerivedData()}>
        <p class="verevon-social-ops-state verevon-social-ops-state--warning">
          <AlertCircle size={16} />
          {i18n.tr(`Ingen databaseforankrede ${config().title.toLowerCase()}-poster ble lastet fordi det dedikerte sosiale endepunktet er utilgjengelig.`, `No database-backed ${config().title.toLowerCase()} records were loaded because the dedicated social endpoint is unavailable.`)}
        </p>
      </Show>

      <section class="verevon-social-ops-metrics" aria-label={i18n.tr(`${config().title} nøkkeltall`, `${config().title} metrics`)}>
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
        <form class="verevon-social-ops-create" onSubmit={(event) => void createCampaign(event)}>
          <div>
            <h2>{i18n.tr('Opprett kampanje', 'Create campaign')}</h2>
            <p>{i18n.tr('Utkastkampanje', 'Draft campaign')}</p>
          </div>
          <label>
            <span>{i18n.tr('Navn', 'Name')}</span>
            <input value={campaignName()} onInput={(event) => setCampaignName(event.currentTarget.value)} />
          </label>
          <label>
            <span>{i18n.tr('Mål', 'Goal')}</span>
            <input value={campaignGoal()} onInput={(event) => setCampaignGoal(event.currentTarget.value)} />
          </label>
          <label>
            <span>Brief</span>
            <textarea rows={3} value={campaignBrief()} onInput={(event) => setCampaignBrief(event.currentTarget.value)} />
          </label>
          <button type="submit" disabled={campaignBusy()}>
            {campaignBusy() ? i18n.tr('Oppretter …', 'Creating...') : i18n.tr('Opprett', 'Create')}
          </button>
          <Show when={campaignFeedback()}>
            {(message) => <p class="verevon-social-ops-create__feedback">{message()}</p>}
          </Show>
        </form>
      </Show>

      <section class="verevon-social-ops-grid" aria-label={i18n.tr(`${config().title} arbeidsområde`, `${config().title} workspace`)}>
        <For each={cards()}>
          {(card) => (
            <article class="verevon-social-ops-card">
              <div>
                <Clock3 size={16} />
                <span>{card.meta}</span>
              </div>
              <h2>{card.title}</h2>
              <p>{card.detail}</p>
              <footer class="verevon-social-ops-card__footer">
                <Show when={card.status}>
                  {(status) => <StatusPill status={status()} />}
                </Show>
                <Show when={card.actionHref}>
                  {(href) => <a href={href()} link>{card.actionLabel ?? i18n.tr('Åpne', 'Open')}</a>}
                </Show>
              </footer>
            </article>
          )}
        </For>
      </section>

      <section class="verevon-social-ops-next">
        <Sparkles size={18} />
        <div>
          <h2>{i18n.tr('Systemlenke', 'System link')}</h2>
          <p>
            {i18n.tr(
              'Kontoer og innlegg lastes fra det org-scopede sosiale API-et. Studio oppretter, Sosial planlegger, Innboks konverterer samtaler, Agenter vil automatisere eviggrønne arbeidsflyter, og Innsikt måler resultater.',
              'Accounts and posts are loaded from the org-scoped social API. Studio creates, Social schedules, Inbox converts conversations, Agents will automate evergreen workflows, and Insights measures outcomes.',
            )}
          </p>
        </div>
      </section>
    </div>
  )
}

function buildSectionMetrics(
  section: SocialOperationsSection,
  workspace: SocialWorkspace,
  tr: TrFn,
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
        { label: tr('Sporede leverandører', 'Tracked providers'), value: String(accounts.length) },
        { label: tr('Tilkoblet', 'Connected'), value: String(connectedAccounts) },
        { label: tr('Kan publisere', 'Publish capable'), value: String(publishCapableAccounts) },
      ]
    case 'drafts':
      return [
        { label: tr('Utkast', 'Drafts'), value: String(draftPosts.length) },
        { label: tr('Fra innboks', 'From inbox'), value: String(posts.filter((post) => post.source.kind === 'inbox' && post.status === 'draft').length) },
        { label: tr('Trenger media', 'Needs media'), value: String(draftPosts.filter((post) => postNeedsMedia(post, adapters)).length) },
      ]
    case 'approvals':
      return [
        { label: tr('Venter', 'Waiting'), value: String(waitingApprovals) },
        { label: tr('Godkjent', 'Approved'), value: String(workspace.approvals.filter((approval) => approval.state === 'approved').length) },
        { label: tr('Blokkert', 'Blocked'), value: String(workspace.approvals.filter((approval) => approval.state === 'blocked').length) },
      ]
    case 'campaigns':
      return [
        { label: tr('Kampanjer', 'Campaigns'), value: String(workspace.campaigns.length) },
        { label: tr('Aktive', 'Active'), value: String(workspace.campaigns.filter((campaign) => campaign.status === 'active').length) },
        { label: tr('Utkast', 'Draft'), value: String(workspace.campaigns.filter((campaign) => campaign.status === 'draft').length) },
      ]
    case 'competitors':
      return [
        { label: tr('Overvåkningslister', 'Watchlists'), value: String(workspace.competitors.length) },
        { label: tr('Endepunkt', 'Endpoint'), value: workspace.resources.competitors === 'live' ? tr('Live', 'Live') : tr('Venter', 'Pending') },
        { label: tr('Klare kanaler', 'Ready channels'), value: String(connectedAccounts) },
      ]
    case 'trends':
      return [
        { label: tr('Signaler', 'Signals'), value: String(workspace.trends.length) },
        { label: tr('Klar', 'Ready'), value: String(workspace.trends.filter((trend) => trend.status === 'ready').length) },
        { label: tr('Trenger media', 'Needs media'), value: String(postsNeedingMedia) },
      ]
    case 'evergreen':
      return [
        { label: tr('Kandidater', 'Candidates'), value: String(workspace.evergreen.length) },
        { label: tr('Klar', 'Ready'), value: String(workspace.evergreen.filter((item) => item.status === 'ready').length) },
        { label: tr('Publiseringsadaptere', 'Publish adapters'), value: String(supportedOrganicAdapters) },
      ]
  }
}

function buildSectionCards(
  section: SocialOperationsSection,
  workspace: SocialWorkspace,
  tr: TrFn,
): SocialOpsCard[] {
  const calendar = workspace.calendar
  const adapters = workspace.adapters

  switch (section) {
    case 'accounts':
      return accountCards(calendar.accounts, adapters, tr)
    case 'drafts':
      return postCards(
        calendar.posts.filter((post) => post.status === 'draft'),
        adapters,
        tr('Ingen utkast ennå', 'No drafts yet'),
        tr('Opprett i Studio eller konverter en innbokssamtale for å fylle denne køen.', 'Create in Studio or convert an inbox conversation to seed this queue.'),
        tr,
      )
    case 'approvals':
      return approvalCards(workspace.approvals, tr)
    case 'campaigns':
      return campaignCards(workspace.campaigns, tr)
    case 'competitors':
      return competitorCards(workspace.competitors, tr)
    case 'trends':
      return trendCards(workspace.trends, tr)
    case 'evergreen':
      return evergreenCards(workspace.evergreen, tr)
  }
}

function accountCards(
  accounts: readonly SocialAccount[],
  adapters: readonly SocialPlatformAdapter[],
  tr: TrFn,
): SocialOpsCard[] {
  if (!accounts.length) {
    return [{
      title: tr('Ingen sosiale kontoer tilkoblet', 'No social accounts connected'),
      meta: tr('Integrasjon kreves', 'Integration required'),
      detail: tr('Koble til LinkedIn, X, Instagram, Facebook, TikTok eller Snapchat under organisasjonsinnstillinger før publisering.', 'Connect LinkedIn, X, Instagram, Facebook, TikTok, or Snapchat under organization settings before publishing.'),
      status: 'needs_oauth',
      actionHref: '/settings/integrations',
      actionLabel: tr('Koble til kontoer', 'Connect accounts'),
    }]
  }

  return accounts.map((account) => {
    const adapter = adapterFor(adapters, account.providerKey)
    const missing = adapter?.requiredCapabilities.filter((capability) => !account.capabilities.includes(capability)) ?? []
    const readiness = missing.length ? tr(`Mangler ${missing.join(', ')}`, `Missing ${missing.join(', ')}`) : tr('Nødvendige funksjoner er på plass', 'Required capabilities present')

    return {
      title: account.label,
      meta: `${platformLabels[account.providerKey]} · ${account.handle}`,
      detail: `${readiness}. ${adapter ? `${adapter.mode} via ${adapter.endpoint}.` : tr('Ingen adapter registrert.', 'No adapter registered.')}`,
      status: account.status,
      actionHref: account.status === 'connected' ? '/social/calendar' : '/settings/integrations',
      actionLabel: account.status === 'connected' ? tr('Bruk i kalender', 'Use in calendar') : tr('Koble til', 'Connect'),
    }
  })
}

function postCards(
  posts: readonly SocialPost[],
  adapters: readonly SocialPlatformAdapter[],
  emptyTitle: string,
  emptyDetail: string,
  tr: TrFn,
): SocialOpsCard[] {
  if (!posts.length) {
    return [{
      title: emptyTitle,
      meta: tr('Kø er tom', 'Queue empty'),
      detail: emptyDetail,
      status: 'scheduled',
      actionHref: '/social/calendar',
      actionLabel: tr('Åpne kalender', 'Open calendar'),
    }]
  }

  return posts.map((post) => ({
    title: post.title,
    meta: `${post.source.label} · ${platformNames(post.platforms)}`,
    detail: postNeedsMedia(post, adapters)
      ? tr(`${post.body} Mediaførste plattformer trenger et klart bilde eller en video før publisering.`, `${post.body} Media-first platforms need a ready image or video before publish.`)
      : post.body,
    status: post.status,
    actionHref: post.source.href ?? '/social/calendar',
    actionLabel: post.source.href ? tr('Åpne kilde', 'Open source') : tr('Åpne kalender', 'Open calendar'),
  }))
}

function approvalCards(approvals: readonly SocialApprovalItem[], tr: TrFn): SocialOpsCard[] {
  if (!approvals.length) {
    return [{
      title: tr('Ingen godkjenningsblokkeringer', 'No approval blockers'),
      meta: tr('Kø er tom', 'Queue empty'),
      detail: tr('Innlegg som krever manuell gjennomgang, mediarettelser eller policyavgjørelser vises her.', 'Posts that require human review, media fixes, or policy decisions will appear here.'),
      status: 'approved',
      actionHref: '/social/calendar',
      actionLabel: tr('Åpne kalender', 'Open calendar'),
    }]
  }

  return approvals.map((approval) => ({
    title: approval.post?.title ?? tr(`Godkjenning ${approval.id}`, `Approval ${approval.id}`),
    meta: `${approval.campaignId ? tr('Kampanjegjennomgang', 'Campaign review') : tr('Innleggsgjennomgang', 'Post review')} · ${formatOptionalDate(approval.dueAt, tr)}`,
    detail: tr(
      `${approval.decisionReason || approvalDetail(approval, tr)} Kilde: ${approval.post?.source.label ?? 'Sosial godkjenningskø'}. Kanaler: ${platformNames(approval.post?.platforms ?? [])}.`,
      `${approval.decisionReason || approvalDetail(approval, tr)} Source: ${approval.post?.source.label ?? 'Social approval queue'}. Channels: ${platformNames(approval.post?.platforms ?? [])}.`,
    ),
    status: approval.state,
    actionHref: approval.post?.source.href ?? '/social/calendar',
    actionLabel: approval.post?.source.href ? tr('Åpne kilde', 'Open source') : tr('Gjennomgå innlegg', 'Review post'),
  }))
}

function campaignCards(campaigns: readonly SocialCampaign[], tr: TrFn): SocialOpsCard[] {
  if (!campaigns.length) {
    return [{
      title: tr('Ingen kampanjeinnlegg tilkoblet', 'No campaign posts connected'),
      meta: tr('Kalenderavledet kø', 'Calendar-derived queue'),
      detail: tr('Planlegg kampanjepakker i Studio, og send deretter innlegg til Sosial-utkast og godkjenninger. Siden bruker /api/v1/social/campaigns når tilgjengelig.', 'Plan campaign packs in Studio, then send posts into Social drafts and approvals. The page consumes /api/v1/social/campaigns when available.'),
      status: 'draft',
      actionHref: '/social/drafts',
      actionLabel: tr('Åpne utkast', 'Open drafts'),
    }]
  }

  return campaigns.map((campaign) => ({
    title: campaign.name,
    meta: `${platformNames(campaign.platforms)} · ${campaign.status}`,
    detail: tr(
      `${campaign.goal || 'Koordiner sosiale kampanjeinnlegg på tvers av valgte kanaler.'} Vindu: ${formatOptionalDate(campaign.startsAt, tr)} til ${formatOptionalDate(campaign.endsAt, tr)}. ${campaign.brief}`,
      `${campaign.goal || 'Coordinate social campaign posts across selected channels.'} Window: ${formatOptionalDate(campaign.startsAt, tr)} to ${formatOptionalDate(campaign.endsAt, tr)}. ${campaign.brief}`,
    ),
    status: campaign.status,
    actionHref: campaign.source.href ?? '/social/calendar',
    actionLabel: campaign.source.href ? tr('Åpne kilde', 'Open source') : tr('Åpne kalender', 'Open calendar'),
  }))
}

function competitorCards(competitors: readonly SocialCompetitorWatchItem[], tr: TrFn): SocialOpsCard[] {
  return competitors.map((competitor) => ({
    title: competitor.label,
    meta: `${platformLabels[competitor.providerKey]} · ${competitor.velocity}`,
    detail: tr(`${competitor.signal} Mål: ${competitor.handle}.`, `${competitor.signal} Target: ${competitor.handle}.`),
    status: competitor.status,
    actionHref: competitor.sourceHref ?? '/social/trends',
    actionLabel: competitor.status === 'endpoint_pending' ? tr('Forbered trender', 'Prepare trends') : tr('Åpne signal', 'Open signal'),
  }))
}

function trendCards(trends: readonly SocialTrendSignal[], tr: TrFn): SocialOpsCard[] {
  if (!trends.length) {
    return [{
      title: tr('Ingen trendsignaler ennå', 'No trend signals yet'),
      meta: tr('Endepunkt klart', 'Endpoint ready'),
      detail: tr('Plattformvarsler, adapterformater og fangede trendmuligheter vises her.', 'Platform warnings, adapter formats, and captured trend opportunities will appear here.'),
      status: 'endpoint_pending',
      actionHref: '/social/drafts',
      actionLabel: tr('Åpne utkast', 'Open drafts'),
    }]
  }

  return trends.map((trend) => ({
    title: trend.label,
    meta: `${platformLabels[trend.providerKey]} · ${trend.velocity}`,
    detail: `${trend.opportunity} Format: ${trend.format}.`,
    status: trend.status,
    actionHref: trend.sourceHref ?? '/social/drafts',
    actionLabel: tr('Bruk signal', 'Use signal'),
  }))
}

function evergreenCards(items: readonly SocialEvergreenItem[], tr: TrFn): SocialOpsCard[] {
  return items.map((item) => ({
    title: item.title,
    meta: `${item.cadence} · ${platformNames(item.platforms)}`,
    detail: tr(`${item.guardrail} Neste kvalifisert: ${formatOptionalDate(item.nextEligibleAt, tr)}.`, `${item.guardrail} Next eligible: ${formatOptionalDate(item.nextEligibleAt, tr)}.`),
    status: item.status,
    actionHref: item.sourcePostId ? '/social/calendar' : '/agents',
    actionLabel: item.sourcePostId ? tr('Åpne kildeinnlegg', 'Open source post') : tr('Åpne arbeidsflyter', 'Open workflows'),
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

function approvalDetail(approval: SocialApprovalItem, tr: TrFn) {
  if (approval.state === 'blocked') return tr('Løs det blokkerte innlegget før godkjenningen kan fortsette.', 'Resolve the blocked post before approval can continue.')
  if (approval.post && postHasMissingMedia(approval.post)) return tr('Klart media kreves før dette innlegget kan gå gjennom godkjenning.', 'Ready media is required before this post can move through approval.')
  return tr('Manuell gjennomgang kreves før planlegging eller publisering.', 'Human review is required before schedule or publish.')
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

function formatOptionalDate(iso: string | null | undefined, tr: TrFn) {
  if (!iso) return tr('ikke planlagt', 'not scheduled')
  return new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })
}

function StatusPill(props: { status: string }) {
  return (
    <span class={cn('verevon-social-status', `verevon-social-status--${props.status.replace(/_/g, '-')}`)}>
      <CheckCircle2 class="size-3.5" />
      {props.status.replace(/_/g, ' ')}
    </span>
  )
}
