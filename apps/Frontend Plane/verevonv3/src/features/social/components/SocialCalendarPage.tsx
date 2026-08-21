import { useLocation } from '@solidjs/router'
import {
  AlertCircle,
  CheckCircle2,
  Clock3,
  MessageSquareReply,
  Plus,
  Rocket,
  Send,
  Sparkles,
  UploadCloud,
  Users,
} from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  buildCalendarDays,
  currentCalendarMonth,
  defaultDraftScheduledAt,
  monthLabel,
  postsByStatus,
  postsForDate,
} from '@/features/social/lib/social-calendar'
import {
  composerPlatforms,
  fallbackSocialCalendar,
  loadSocialCalendarResource,
  loadSocialContext,
  platformLabels,
} from '@/features/social/lib/social-workspace'
import {
  createSocialPost,
  publishSocialPost,
  scheduleSocialPost,
  type RecommendedSocialWindow,
  type SocialAccount,
  type SocialPost,
  type SocialPublishResult,
  type SocialProviderKey,
} from '@/shared/api/social-client'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'

const pendingDraftKey = 'verevon.social.pendingDraft'

export default function SocialCalendarPage() {
  const i18n = useI18n()
  const location = useLocation()
  const [ctx] = createResource(loadSocialContext)
  const orgId = createMemo(() => ctx()?.orgId ?? '')
  const orgScope = createMemo(() => ({ orgId: orgId(), orgLabel: ctx()?.orgLabel ?? 'Verevon' }))
  const [calendarRes] = createResource(orgScope, (scope) => (
    scope.orgId
      ? loadSocialCalendarResource(scope.orgId, scope.orgLabel)
      : Promise.resolve({ data: fallbackSocialCalendar(scope.orgLabel), source: 'fallback' as const })
  ))
  const calendar = createMemo(() => calendarRes()?.data ?? fallbackSocialCalendar(ctx()?.orgLabel))
  const [posts, setPosts] = createSignal<SocialPost[]>([])
  const [selectedPostId, setSelectedPostId] = createSignal<string | null>(null)
  const [month, setMonth] = createSignal(currentCalendarMonth())
  const [draftTitle, setDraftTitle] = createSignal(i18n.tr('Kundeinnsikts-innlegg', 'Customer insight post'))
  const [draftBody, setDraftBody] = createSignal(i18n.tr('Gjør det sterkeste supportsignalet om til en nyttig offentlig oppdatering.', 'Turn the strongest support signal into a useful public update.'))
  const [draftPlatforms, setDraftPlatforms] = createSignal<SocialProviderKey[]>(['linkedin', 'x'])
  const [feedback, setFeedback] = createSignal<string | null>(null)
  const [publishResult, setPublishResult] = createSignal<SocialPublishResult | null>(null)
  const [busy, setBusy] = createSignal(false)

  createEffect(
    () => calendar(),
    (nextCalendar) => {
      if (!nextCalendar) return

      const imported = readPendingDraft()
      const nextPosts = imported && !nextCalendar.posts.some((post) => post.id === imported.id)
        ? [imported, ...nextCalendar.posts]
        : nextCalendar.posts

      setPosts(nextPosts)
      setSelectedPostId((current) => current ?? nextPosts[0]?.id ?? null)
      if (imported) {
        setFeedback(i18n.tr('Innbokssamtale konvertert til et sosialt utkast.', 'Inbox conversation converted into a social draft.'))
        clearPendingDraft()
      }
    },
  )

  const accounts = createMemo(() => calendar().accounts)
  const windows = createMemo(() => calendar().recommendedWindows)
  const days = createMemo(() => buildCalendarDays(month()))
  const selectedPost = createMemo(() => posts().find((post) => post.id === selectedPostId()) ?? posts()[0] ?? null)
  const stats = createMemo(() => postsByStatus(posts()))
  const importedFromInbox = createMemo(() => new URLSearchParams(location.search).get('source') === 'inbox')
  const approvalNeedsReview = createMemo(() => {
    const post = selectedPost()
    return Boolean(post?.approval.required && !['approved', 'not_required'].includes(post.approval.state))
  })
  const canCreateDraft = createMemo(() => Boolean(
    orgId() &&
    draftTitle().trim() &&
    draftBody().trim() &&
    draftPlatforms().length > 0 &&
    !busy(),
  ))

  const togglePlatform = (platform: SocialProviderKey) => {
    const current = draftPlatforms()
    if (current.includes(platform)) {
      if (current.length > 1) setDraftPlatforms(current.filter((candidate) => candidate !== platform))
      return
    }
    setDraftPlatforms([...current, platform])
  }

  const addDraft = async () => {
    if (!canCreateDraft()) return
    setBusy(true)
    setFeedback(null)
    setPublishResult(null)
    try {
      const result = await createSocialPost(orgId(), {
        title: draftTitle(),
        body: draftBody(),
        platforms: draftPlatforms(),
        scheduledAt: defaultDraftScheduledAt(),
      })
      setPosts((current) => [result.post, ...current])
      setSelectedPostId(result.post.id)
      setFeedback(i18n.tr('Utkast opprettet og lagt til i kalenderkøen.', 'Draft created and added to the calendar queue.'))
    } catch (reason) {
      setFeedback(reason instanceof Error ? reason.message : i18n.tr('Utkastet kunne ikke opprettes.', 'Draft could not be created.'))
    } finally {
      setBusy(false)
    }
  }

  const scheduleSelected = async (window: RecommendedSocialWindow) => {
    const post = selectedPost()
    if (!post || !orgId() || busy()) return
    if (approvalNeedsReview()) {
      setFeedback(i18n.tr('Gjennomgå og godkjenn innlegget før du planlegger det.', 'Review and approve this post before scheduling it.'))
      return
    }
    setBusy(true)
    setFeedback(null)
    setPublishResult(null)
    try {
      const result = await scheduleSocialPost(orgId(), post.id, window.startsAt)
      setPosts((current) => current.map((candidate) => (candidate.id === post.id ? { ...candidate, ...result.post } : candidate)))
      setFeedback(i18n.tr(`Planlagt for ${window.label}.`, `Scheduled for ${window.label}.`))
    } catch (reason) {
      setFeedback(reason instanceof Error ? reason.message : i18n.tr('Innlegget kunne ikke planlegges.', 'Post could not be scheduled.'))
    } finally {
      setBusy(false)
    }
  }

  const publishSelected = async () => {
    const post = selectedPost()
    if (!post || !orgId() || busy()) return
    if (approvalNeedsReview()) {
      setFeedback(i18n.tr('Gjennomgå og godkjenn innlegget før du publiserer det.', 'Review and approve this post before publishing it.'))
      return
    }
    setBusy(true)
    setFeedback(null)
    try {
      const result = await publishSocialPost(orgId(), post.id)
      setPosts((current) => current.map((candidate) => (candidate.id === post.id ? { ...candidate, ...result.post } : candidate)))
      setPublishResult(result.result)
      setFeedback(result.result.status === 'blocked'
        ? i18n.tr('Publisering blokkert. Løs kravene fra leverandøren nedenfor før du legger i kø.', 'Publish blocked. Resolve the provider requirements below before queueing.')
        : i18n.tr('Publiseringsforespørsel akseptert. Leverandøradaptere håndterer plattformspesifikk publisering.', 'Publish intent accepted. Provider adapters will handle platform-specific publishing.'))
    } catch (reason) {
      setFeedback(reason instanceof Error ? reason.message : i18n.tr('Innlegget kunne ikke publiseres.', 'Post could not be published.'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div class="verevon-social-page">
      <header class="verevon-social-hero">
        <div>
          <span class="verevon-social-kicker">
            <Sparkles class="size-4" />
            {i18n.tr('AI-native sosiale operasjoner', 'AI-native social operations')}
          </span>
          <h1>{i18n.tr('Sosial kalender', 'Social calendar')}</h1>
          <p>
            {i18n.tr(
              'Planlegg, utkast, godkjenn, planlegg og publiser sosiale innlegg fra supportsignaler, kunnskapsoppdateringer og kampanjearbeid uten å forlate Verevon.',
              'Plan, draft, approve, schedule, and publish social posts from support signals, knowledge updates, and campaign work without leaving Verevon.',
            )}
          </p>
        </div>
        <div class="verevon-social-hero__metrics">
          <Metric label={i18n.tr('Utkast', 'Drafts')} value={stats().draft} />
          <Metric label={i18n.tr('Planlagt', 'Scheduled')} value={stats().scheduled} />
          <Metric label={i18n.tr('Publiserer', 'Publishing')} value={stats().publishing} />
        </div>
      </header>

      <Show when={importedFromInbox()}>
        <div class="verevon-social-inbox-banner">
          <MessageSquareReply class="size-4" />
          <span>{i18n.tr('Innbokskontekst er aktiv. Bruk det importerte utkastet eller opprett et oppfølgingsinnlegg fra den valgte samtalen.', 'Inbox context is active. Use the imported draft or create a follow-up post from the selected conversation.')}</span>
          <a href="/inbox?view=social" link>{i18n.tr('Tilbake til sosial innboks', 'Back to social inbox')}</a>
        </div>
      </Show>

      <Show when={feedback()}>
        {(message) => <p class="verevon-social-feedback">{message()}</p>}
      </Show>

      <Show when={calendarRes()?.source === 'fallback'}>
        <div class="verevon-social-unavailable-banner">
          <AlertCircle class="size-4" />
          <span>{i18n.tr('Sosial kalender bruker reservedata fordi den org-scopede sosiale gatewayen er utilgjengelig, eller ingen organisasjonsscope ble løst.', 'Social calendar is using fallback data because the org-scoped social gateway is unavailable or no organization scope was resolved.')}</span>
          <a href="/social/accounts" link>{i18n.tr('Sjekk kontoer', 'Check accounts')}</a>
        </div>
      </Show>

      <section class="verevon-social-grid">
        <aside class="verevon-social-accounts">
          <div class="verevon-social-panel-title">
            <Users class="size-4" />
            <span>{i18n.tr('Kontoer', 'Accounts')}</span>
          </div>
          <Show
            when={accounts().length > 0}
            fallback={<EmptyState title={i18n.tr('Ingen kontoer tilkoblet', 'No accounts connected')} detail={i18n.tr('Koble til sosiale leverandører før planlegging eller publisering av innlegg.', 'Connect social providers before scheduling or publishing posts.')} />}
          >
            <For each={accounts()}>
              {(account) => <AccountCard account={account} />}
            </For>
          </Show>
          <div class="verevon-social-account-note">
            <UploadCloud class="size-4" />
            {i18n.tr('Leverandør-OAuth og mediapublisering eksponeres som integration-core-funksjoner og kjøres gjennom godkjente adaptere.', 'Provider OAuth and media publishing are exposed as integration-core capabilities and will run through approved adapters.')}
          </div>
        </aside>

        <main class="verevon-social-calendar-panel">
          <div class="verevon-social-calendar-toolbar">
            <div>
              <span>{i18n.tr('Planlegger', 'Planner')}</span>
              <h2>{monthLabel(month())}</h2>
            </div>
            <div class="verevon-social-calendar-toolbar__actions">
              <button type="button" onClick={() => setMonth(new Date(Date.UTC(month().getUTCFullYear(), month().getUTCMonth() - 1, 1)))}>
                {i18n.tr('Forrige', 'Previous')}
              </button>
              <button type="button" onClick={() => setMonth(currentCalendarMonth())}>
                {i18n.tr('I dag', 'Today')}
              </button>
              <button type="button" onClick={() => setMonth(new Date(Date.UTC(month().getUTCFullYear(), month().getUTCMonth() + 1, 1)))}>
                {i18n.tr('Neste', 'Next')}
              </button>
            </div>
          </div>

          <div class="verevon-social-weekdays">
            <For each={i18n.tr('Man,Tir,Ons,Tor,Fre,Lør,Søn', 'Mon,Tue,Wed,Thu,Fri,Sat,Sun').split(',')}>
              {(day) => <span>{day}</span>}
            </For>
          </div>
          <div class="verevon-social-calendar">
            <For each={days()}>
              {(day) => {
                const dayPosts = () => postsForDate(posts(), day.isoDate)
                return (
                  <button
                    type="button"
                    class={cn('verevon-social-day', !day.inMonth && 'verevon-social-day--muted', day.isToday && 'verevon-social-day--today')}
                    onClick={() => {
                      const first = dayPosts()[0]
                      if (first) setSelectedPostId(first.id)
                    }}
                  >
                    <span class="verevon-social-day__number">{day.dayNumber}</span>
                    <div class="verevon-social-day__posts">
                      <For each={dayPosts()}>
                        {(post) => (
                          <span class={cn('verevon-social-day-post', selectedPostId() === post.id && 'verevon-social-day-post--active')}>
                            {post.title}
                          </span>
                        )}
                      </For>
                    </div>
                  </button>
                )
              }}
            </For>
          </div>
          <Show when={posts().length === 0}>
            <EmptyState title={i18n.tr('Ingen planlagte sosiale innlegg', 'No scheduled social posts')} detail={i18n.tr('Opprett et utkast eller konverter en innbokssamtale for å fylle kalenderen.', 'Create a draft or convert an inbox conversation to seed the calendar.')} />
          </Show>
        </main>

        <aside class="verevon-social-composer">
          <div class="verevon-social-panel-title">
            <Plus class="size-4" />
            <span>{i18n.tr('Skriv', 'Compose')}</span>
          </div>
          <label>
            <span>{i18n.tr('Tittel', 'Title')}</span>
            <input value={draftTitle()} onInput={(event) => setDraftTitle(event.currentTarget.value)} />
          </label>
          <label>
            <span>{i18n.tr('Innleggstekst', 'Post copy')}</span>
            <textarea rows={7} value={draftBody()} onInput={(event) => setDraftBody(event.currentTarget.value)} />
          </label>
          <div class="verevon-social-platform-toggle">
            <For each={composerPlatforms}>
              {(platform) => (
                <button
                  type="button"
                  class={{ 'is-active': draftPlatforms().includes(platform) }}
                  onClick={() => togglePlatform(platform)}
                >
                  {platformLabels[platform]}
                </button>
              )}
            </For>
          </div>
          <button type="button" disabled={!canCreateDraft()} onClick={() => void addDraft()} class="verevon-social-primary-button">
            <Send class="size-4" />
            {busy() ? i18n.tr('Arbeider …', 'Working...') : i18n.tr('Opprett utkast', 'Create draft')}
          </button>

          <Show when={selectedPost()}>
            {(post) => (
              <div class="verevon-social-selected">
                <span class="verevon-social-selected__eyebrow">{i18n.tr('Valgt innlegg', 'Selected post')}</span>
                <h3>{post().title}</h3>
                <p>{post().body}</p>
                <div class="verevon-social-selected__meta">
                  <StatusPill status={post().status} />
                  <span>{formatDateTime(post().scheduledAt)}</span>
                </div>
                <Show when={post().previews?.length}>
                  <div class="verevon-social-preview-list">
                    <For each={post().previews ?? []}>
                      {(preview) => (
                        <article class="verevon-social-preview-card">
                          <div>
                            <strong>{preview.label}</strong>
                            <span>{preview.characterCount}/{preview.maxCharacters}</span>
                          </div>
                          <p>{preview.text}</p>
                          <Show when={preview.warnings.length}>
                            <ul>
                              <For each={preview.warnings}>
                                {(warning) => <li>{warning}</li>}
                              </For>
                            </ul>
                          </Show>
                        </article>
                      )}
                    </For>
                  </div>
                </Show>
                <div class="verevon-social-window-list">
                  <Show
                    when={approvalNeedsReview()}
                    fallback={(
                      <Show
                        when={windows().length > 0}
                        fallback={<EmptyState title={i18n.tr('Ingen foreslåtte tidsvinduer', 'No suggested windows')} detail={i18n.tr('Planleggingsforslag vises etter at kalenderendepunktet returnerer dem.', 'Scheduling suggestions will appear after the calendar endpoint returns them.')} />}
                      >
                        <For each={windows()}>
                          {(window) => (
                            <button type="button" disabled={busy()} onClick={() => void scheduleSelected(window)}>
                              <Clock3 class="size-4" />
                              <span>
                                <strong>{window.label}</strong>
                                <small>{window.reason}</small>
                              </span>
                            </button>
                          )}
                        </For>
                      </Show>
                    )}
                  >
                    <p class="verevon-social-approval-note">
                      {i18n.tr('Planlegging åpnes etter menneskelig godkjenning.', 'Scheduling becomes available after human approval.')}
                    </p>
                  </Show>
                </div>
                <Show
                  when={approvalNeedsReview()}
                  fallback={(
                    <button type="button" disabled={busy()} onClick={() => void publishSelected()} class="verevon-social-publish-button">
                      <Rocket class="size-4" />
                      {i18n.tr('Publiser', 'Publish')}
                    </button>
                  )}
                >
                  <a href="/social/approvals" link class="verevon-social-publish-button">
                    <CheckCircle2 class="size-4" />
                    {i18n.tr('Gjennomgå godkjenning', 'Review approval')}
                  </a>
                </Show>
                <Show when={publishResult()}>
                  {(result) => (
                    <div class="verevon-social-publish-result">
                      <strong>{i18n.tr(`Adapterresultat: ${result().status}`, `Adapter result: ${result().status}`)}</strong>
                      <span>{i18n.tr(`Idempotensnøkkel: ${result().idempotencyKey}`, `Idempotency key: ${result().idempotencyKey}`)}</span>
                      <For each={result().attempts}>
                        {(attempt) => (
                          <article>
                            <div>
                              <StatusPill status={attempt.status} />
                              <span>{attempt.label}</span>
                            </div>
                            <p>{attempt.message}</p>
                            <small>{attempt.mode} · {attempt.endpoint}</small>
                          </article>
                        )}
                      </For>
                    </div>
                  )}
                </Show>
              </div>
            )}
          </Show>
          <Show when={!selectedPost()}>
            <EmptyState title={i18n.tr('Ingen innlegg valgt', 'No post selected')} detail={i18n.tr('Velg et innlegg fra kalenderen eller opprett et nytt utkast.', 'Select a post from the calendar or create a new draft.')} />
          </Show>
        </aside>
      </section>
    </div>
  )
}

function Metric(props: { label: string; value: number }) {
  return (
    <div>
      <strong>{props.value}</strong>
      <span>{props.label}</span>
    </div>
  )
}

function EmptyState(props: { title: string; detail: string }) {
  return (
    <div class="verevon-social-empty-state">
      <strong>{props.title}</strong>
      <span>{props.detail}</span>
    </div>
  )
}

function AccountCard(props: { account: SocialAccount }) {
  return (
    <article class="verevon-social-account-card" style={{ '--account-accent': props.account.accent }}>
      <div>
        <span />
        <strong>{props.account.label}</strong>
        <small>{props.account.handle}</small>
      </div>
      <StatusPill status={props.account.status} />
    </article>
  )
}

function StatusPill(props: { status: string }) {
  return (
    <span class={cn('verevon-social-status', `verevon-social-status--${props.status.replace(/_/g, '-')}`)}>
      <CheckCircle2 class="size-3.5" />
      {props.status.replace(/_/g, ' ')}
    </span>
  )
}

function formatDateTime(iso: string) {
  return new Date(iso).toLocaleString('en-GB', {
    day: 'numeric',
    month: 'short',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function readPendingDraft() {
  try {
    const raw = window.sessionStorage.getItem(pendingDraftKey)
    if (!raw) return null
    return JSON.parse(raw) as SocialPost
  } catch {
    return null
  }
}

function clearPendingDraft() {
  try {
    window.sessionStorage.removeItem(pendingDraftKey)
  } catch {
    // Session storage is an optional handoff convenience.
  }
}
