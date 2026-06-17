import { A, useLocation } from '@solidjs/router'
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
} from 'lucide-solid'
import { createEffect, createMemo, createResource, createSignal, For, Show } from 'solid-js'
import { buildCalendarDays, monthLabel, postsByStatus, postsForDate } from '@/features/social/lib/social-calendar'
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

const pendingDraftKey = 'velion.social.pendingDraft'

export default function SocialCalendarPage() {
  const location = useLocation()
  const [ctx] = createResource(loadSocialContext)
  const orgId = createMemo(() => ctx()?.orgId ?? '')
  const orgScope = createMemo(() => ({ orgId: orgId(), orgLabel: ctx()?.orgLabel ?? 'Velion' }))
  const [calendarRes] = createResource(orgScope, (scope) => (
    scope.orgId
      ? loadSocialCalendarResource(scope.orgId, scope.orgLabel)
      : Promise.resolve({ data: fallbackSocialCalendar(scope.orgLabel), source: 'fallback' as const })
  ))
  const calendar = createMemo(() => calendarRes()?.data ?? fallbackSocialCalendar(ctx()?.orgLabel))
  const [posts, setPosts] = createSignal<SocialPost[]>([])
  const [selectedPostId, setSelectedPostId] = createSignal<string | null>(null)
  const [month, setMonth] = createSignal(new Date('2026-06-15T00:00:00.000Z'))
  const [draftTitle, setDraftTitle] = createSignal('Customer insight post')
  const [draftBody, setDraftBody] = createSignal('Turn the strongest support signal into a useful public update.')
  const [draftPlatforms, setDraftPlatforms] = createSignal<SocialProviderKey[]>(['linkedin', 'x'])
  const [feedback, setFeedback] = createSignal<string | null>(null)
  const [publishResult, setPublishResult] = createSignal<SocialPublishResult | null>(null)
  const [busy, setBusy] = createSignal(false)

  createEffect(() => {
    const nextCalendar = calendar()
    if (!nextCalendar) return

    const imported = readPendingDraft()
    const nextPosts = imported && !nextCalendar.posts.some((post) => post.id === imported.id)
      ? [imported, ...nextCalendar.posts]
      : nextCalendar.posts

    setPosts(nextPosts)
    setSelectedPostId((current) => current ?? nextPosts[0]?.id ?? null)
    if (imported) {
      setFeedback('Inbox conversation converted into a social draft.')
      clearPendingDraft()
    }
  })

  const accounts = createMemo(() => calendar().accounts)
  const windows = createMemo(() => calendar().recommendedWindows)
  const days = createMemo(() => buildCalendarDays(month(), new Date('2026-06-15T08:00:00.000Z')))
  const selectedPost = createMemo(() => posts().find((post) => post.id === selectedPostId()) ?? posts()[0] ?? null)
  const stats = createMemo(() => postsByStatus(posts()))
  const importedFromInbox = createMemo(() => new URLSearchParams(location.search).get('source') === 'inbox')
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
        scheduledAt: '2026-06-17T10:00:00.000Z',
      })
      setPosts((current) => [result.post, ...current])
      setSelectedPostId(result.post.id)
      setFeedback('Draft created and added to the calendar queue.')
    } catch (reason) {
      setFeedback(reason instanceof Error ? reason.message : 'Draft could not be created.')
    } finally {
      setBusy(false)
    }
  }

  const scheduleSelected = async (window: RecommendedSocialWindow) => {
    const post = selectedPost()
    if (!post || !orgId() || busy()) return
    setBusy(true)
    setFeedback(null)
    setPublishResult(null)
    try {
      const result = await scheduleSocialPost(orgId(), post.id, window.startsAt)
      setPosts((current) => current.map((candidate) => (candidate.id === post.id ? { ...candidate, ...result.post } : candidate)))
      setFeedback(`Scheduled for ${window.label}.`)
    } catch (reason) {
      setFeedback(reason instanceof Error ? reason.message : 'Post could not be scheduled.')
    } finally {
      setBusy(false)
    }
  }

  const publishSelected = async () => {
    const post = selectedPost()
    if (!post || !orgId() || busy()) return
    setBusy(true)
    setFeedback(null)
    try {
      const result = await publishSocialPost(orgId(), post.id)
      setPosts((current) => current.map((candidate) => (candidate.id === post.id ? { ...candidate, ...result.post } : candidate)))
      setPublishResult(result.result)
      setFeedback(result.result.status === 'blocked'
        ? 'Publish blocked. Resolve the provider requirements below before queueing.'
        : 'Publish intent accepted. Provider adapters will handle platform-specific publishing.')
    } catch (reason) {
      setFeedback(reason instanceof Error ? reason.message : 'Post could not be published.')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div class="velion-social-page">
      <header class="velion-social-hero">
        <div>
          <span class="velion-social-kicker">
            <Sparkles class="size-4" />
            AI-native social operations
          </span>
          <h1>Social calendar</h1>
          <p>
            Plan, draft, approve, schedule, and publish social posts from support signals, knowledge updates,
            and campaign work without leaving Velion.
          </p>
        </div>
        <div class="velion-social-hero__metrics">
          <Metric label="Drafts" value={stats().draft} />
          <Metric label="Scheduled" value={stats().scheduled} />
          <Metric label="Publishing" value={stats().publishing} />
        </div>
      </header>

      <Show when={importedFromInbox()}>
        <div class="velion-social-inbox-banner">
          <MessageSquareReply class="size-4" />
          <span>Inbox context is active. Use the imported draft or create a follow-up post from the selected conversation.</span>
          <A href="/inbox?view=social">Back to social inbox</A>
        </div>
      </Show>

      <Show when={feedback()}>
        {(message) => <p class="velion-social-feedback">{message()}</p>}
      </Show>

      <Show when={calendarRes()?.source === 'fallback'}>
        <div class="velion-social-unavailable-banner">
          <AlertCircle class="size-4" />
          <span>Social calendar is using fallback data because the org-scoped social gateway is unavailable or no organization scope was resolved.</span>
          <A href="/social/accounts">Check accounts</A>
        </div>
      </Show>

      <section class="velion-social-grid">
        <aside class="velion-social-accounts">
          <div class="velion-social-panel-title">
            <Users class="size-4" />
            <span>Accounts</span>
          </div>
          <Show
            when={accounts().length > 0}
            fallback={<EmptyState title="No accounts connected" detail="Connect social providers before scheduling or publishing posts." />}
          >
            <For each={accounts()}>
              {(account) => <AccountCard account={account} />}
            </For>
          </Show>
          <div class="velion-social-account-note">
            <UploadCloud class="size-4" />
            Provider OAuth and media publishing are exposed as integration-core capabilities and will run through approved adapters.
          </div>
        </aside>

        <main class="velion-social-calendar-panel">
          <div class="velion-social-calendar-toolbar">
            <div>
              <span>Planner</span>
              <h2>{monthLabel(month())}</h2>
            </div>
            <div class="velion-social-calendar-toolbar__actions">
              <button type="button" onClick={() => setMonth(new Date(Date.UTC(month().getUTCFullYear(), month().getUTCMonth() - 1, 1)))}>
                Previous
              </button>
              <button type="button" onClick={() => setMonth(new Date('2026-06-15T00:00:00.000Z'))}>
                Today
              </button>
              <button type="button" onClick={() => setMonth(new Date(Date.UTC(month().getUTCFullYear(), month().getUTCMonth() + 1, 1)))}>
                Next
              </button>
            </div>
          </div>

          <div class="velion-social-weekdays">
            <For each={['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']}>
              {(day) => <span>{day}</span>}
            </For>
          </div>
          <div class="velion-social-calendar">
            <For each={days()}>
              {(day) => {
                const dayPosts = () => postsForDate(posts(), day.isoDate)
                return (
                  <button
                    type="button"
                    class={cn('velion-social-day', !day.inMonth && 'velion-social-day--muted', day.isToday && 'velion-social-day--today')}
                    onClick={() => {
                      const first = dayPosts()[0]
                      if (first) setSelectedPostId(first.id)
                    }}
                  >
                    <span class="velion-social-day__number">{day.dayNumber}</span>
                    <div class="velion-social-day__posts">
                      <For each={dayPosts()}>
                        {(post) => (
                          <span class={cn('velion-social-day-post', selectedPostId() === post.id && 'velion-social-day-post--active')}>
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
            <EmptyState title="No scheduled social posts" detail="Create a draft or convert an inbox conversation to seed the calendar." />
          </Show>
        </main>

        <aside class="velion-social-composer">
          <div class="velion-social-panel-title">
            <Plus class="size-4" />
            <span>Compose</span>
          </div>
          <label>
            <span>Title</span>
            <input value={draftTitle()} onInput={(event) => setDraftTitle(event.currentTarget.value)} />
          </label>
          <label>
            <span>Post copy</span>
            <textarea rows={7} value={draftBody()} onInput={(event) => setDraftBody(event.currentTarget.value)} />
          </label>
          <div class="velion-social-platform-toggle">
            <For each={composerPlatforms}>
              {(platform) => (
                <button
                  type="button"
                  classList={{ 'is-active': draftPlatforms().includes(platform) }}
                  onClick={() => togglePlatform(platform)}
                >
                  {platformLabels[platform]}
                </button>
              )}
            </For>
          </div>
          <button type="button" disabled={!canCreateDraft()} onClick={() => void addDraft()} class="velion-social-primary-button">
            <Send class="size-4" />
            {busy() ? 'Working...' : 'Create draft'}
          </button>

          <Show when={selectedPost()}>
            {(post) => (
              <div class="velion-social-selected">
                <span class="velion-social-selected__eyebrow">Selected post</span>
                <h3>{post().title}</h3>
                <p>{post().body}</p>
                <div class="velion-social-selected__meta">
                  <StatusPill status={post().status} />
                  <span>{formatDateTime(post().scheduledAt)}</span>
                </div>
                <Show when={post().previews?.length}>
                  <div class="velion-social-preview-list">
                    <For each={post().previews ?? []}>
                      {(preview) => (
                        <article class="velion-social-preview-card">
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
                <div class="velion-social-window-list">
                  <Show
                    when={windows().length > 0}
                    fallback={<EmptyState title="No suggested windows" detail="Scheduling suggestions will appear after the calendar endpoint returns them." />}
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
                </div>
                <button type="button" disabled={busy()} onClick={() => void publishSelected()} class="velion-social-publish-button">
                  <Rocket class="size-4" />
                  Publish with approval
                </button>
                <Show when={publishResult()}>
                  {(result) => (
                    <div class="velion-social-publish-result">
                      <strong>Adapter result: {result().status}</strong>
                      <span>Idempotency key: {result().idempotencyKey}</span>
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
            <EmptyState title="No post selected" detail="Select a post from the calendar or create a new draft." />
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
    <div class="velion-social-empty-state">
      <strong>{props.title}</strong>
      <span>{props.detail}</span>
    </div>
  )
}

function AccountCard(props: { account: SocialAccount }) {
  return (
    <article class="velion-social-account-card" style={{ '--account-accent': props.account.accent }}>
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
    <span class={cn('velion-social-status', `velion-social-status--${props.status.replace(/_/g, '-')}`)}>
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
