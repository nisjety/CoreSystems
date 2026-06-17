
import { A } from '@solidjs/router'
import { Activity, ArrowUpRight, CloudSun, MessageSquare, Newspaper, RefreshCw, Wind } from 'lucide-solid'
import { createEffect, createSignal, For, Match, onCleanup, onMount, Show, Switch, type JSX } from 'solid-js'
import type { DashboardCard } from '@/features/dashboard/home/dashboard-cards'
import {
  formatRelativeNorwegianTime,
  loadDashboardInformationSnapshot,
  loadNews,
  loadTraffic,
  loadWeather,
  newsCategoryOptions,
  type InformationNewsPayload,
  type InformationTrafficPayload,
  type InformationWeatherPayload,
  weatherGlyph,
} from '@/shared/api/information-client'
import { cn } from '@/shared/lib/cn'

const aboveFoldDashboardCardIds = new Set(['weather', 'traffic', 'news'])

type DashboardInformationSnapshot = Awaited<ReturnType<typeof loadDashboardInformationSnapshot>>
type LiveInformationLoadResult<T> = {
  data: T
  usingLiveLocation: boolean
}

function createLiveInformationState<T>(config: {
  fallbackError: string
  load: (forceRefreshLocation?: boolean) => Promise<LiveInformationLoadResult<T>>
  snapshot: (snapshot: DashboardInformationSnapshot) => { data: T | null; error: string | null }
}) {
  const [data, setData] = createSignal<T | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(true)
  const [usingLiveLocation, setUsingLiveLocation] = createSignal(false)
  let cancelled = false

  const refresh = async (forceRefreshLocation = false) => {
    setLoading(true)
    setError(null)
    try {
      const result = await config.load(forceRefreshLocation)
      if (cancelled) return
      setData(() => result.data)
      setUsingLiveLocation(result.usingLiveLocation)
    } catch (reason) {
      if (!cancelled) setError(reason instanceof Error ? reason.message : config.fallbackError)
    } finally {
      if (!cancelled) setLoading(false)
    }
  }

  const loadSnapshot = async () => {
    try {
      const snapshot = await loadDashboardInformationSnapshot()
      if (cancelled) return
      const initial = config.snapshot(snapshot)
      setUsingLiveLocation(snapshot.usingLiveLocation)
      setData(() => initial.data)
      setError(initial.error)
      setLoading(false)
    } catch {
      void refresh()
    }
  }

  onMount(() => {
    void loadSnapshot()
  })

  onCleanup(() => {
    cancelled = true
  })

  return {
    data,
    error,
    loading,
    refresh,
    usingLiveLocation,
  }
}

export function DashboardCardsRail(props: {
  hidden: boolean
  hideNext: boolean
  onNextPage: () => void
  onPrompt: (card: DashboardCard) => void
  visibleCards: DashboardCard[]
}) {
  return (
    <section
      class={cn(
        'velion-home-cards velion-home-band velion-home-band-bottom mx-auto flex min-h-0 w-full max-w-5xl flex-col justify-start px-4 transition-opacity duration-200',
        props.hidden && 'velion-home-cards--hidden',
      )}
    >
      <div class="velion-home-card-grid velion-card-page grid grid-cols-1 gap-4 lg:grid-cols-3">
        <For each={props.visibleCards}>
          {(card) => <DashboardImageCard card={card} onPrompt={props.onPrompt} />}
        </For>
      </div>

      <Show when={!props.hideNext}>
        <button
          type="button"
          onClick={() => props.onNextPage()}
          class="velion-home-next mx-auto flex size-12 items-center justify-center transition-transform duration-300 hover:scale-105 active:scale-95"
          aria-label="Vis neste kortside"
          title="Vis neste kortside"
        >
          <span aria-hidden="true">
            <span class="dashboard-home__next-line dashboard-home__next-line--left" />
            <span class="dashboard-home__next-line dashboard-home__next-line--right" />
          </span>
        </button>
      </Show>
    </section>
  )
}

function DashboardImageCard(props: {
  card: DashboardCard
  onPrompt: (card: DashboardCard) => void
}) {
  const aboveFold = () => aboveFoldDashboardCardIds.has(props.card.id)

  return (
    <Switch>
      <Match when={props.card.id === 'weather'}>
        <WeatherDashboardCard card={props.card} onPrompt={props.onPrompt} />
      </Match>
      <Match when={props.card.id === 'traffic'}>
        <TrafficDashboardCard card={props.card} onPrompt={props.onPrompt} />
      </Match>
      <Match when={props.card.id === 'news'}>
        <NewsDashboardCard card={props.card} onPrompt={props.onPrompt} />
      </Match>
      <Match when={true}>
        <div class="velion-dashboard-card">
          <div class="velion-dashboard-card-label">
            {props.card.category}
          </div>

          <A href={props.card.href} class="block">
            <div class="velion-dashboard-card-media">
              <img
                src={props.card.image ?? '/imagens/arched-corridor-1.jpeg'}
                alt={props.card.title}
                loading={aboveFold() ? 'eager' : 'lazy'}
                class="dashboard-card-image"
              />
              <div class="dashboard-card-gradient" />
              <div class="velion-dashboard-card-copy">
                <h3>{props.card.title}</h3>
                <p>{props.card.description}</p>
              </div>
            </div>
          </A>

          <button
            type="button"
            onClick={() => props.onPrompt(props.card)}
            class="velion-dashboard-card-action"
            aria-label={`Start chat for ${props.card.title}`}
            title={`Start chat for ${props.card.title}`}
          >
            <MessageSquare class="size-3.5" />
            Chat
          </button>
        </div>
      </Match>
    </Switch>
  )
}

function WeatherDashboardCard(props: { card: DashboardCard; onPrompt: (card: DashboardCard) => void }) {
  const weather = createLiveInformationState<InformationWeatherPayload>({
    fallbackError: 'Kunne ikke hente værdata.',
    load: loadWeather,
    snapshot: (snapshot) => ({
      data: snapshot.weather,
      error: snapshot.weatherError,
    }),
  })

  return (
    <InformationCardShell
      card={props.card}
      onPrompt={props.onPrompt}
      action={<RefreshButton label="Oppdater vær" loading={weather.loading()} onClick={() => void weather.refresh(true)} />}
    >
      <InformationCardState
        data={weather.data()}
        error={weather.error()}
        loading={weather.loading()}
        render={(payload) => (
          <>
            <div class="dashboard-info-card__weather">
              <div>
                <p>{payload.current.location}</p>
                <small>{weather.usingLiveLocation() ? 'Din posisjon' : 'Standard plassering'}</small>
                <div>
                  <span>{weatherGlyph(payload.current.condition)}</span>
                  <div class="dashboard-info-card__weather-readout">
                    <strong>{payload.current.temperature}°</strong>
                    <small>{payload.current.condition}</small>
                  </div>
                </div>
              </div>
              <div class="dashboard-info-card__updated">
                <small>Oppdatert</small>
                <strong>{formatRelativeNorwegianTime(payload.current.lastUpdated)}</strong>
              </div>
            </div>
            <div class="dashboard-info-card__metrics">
              <MetricPill icon={<Wind class="size-3.5" />} label="Vind" value={`${payload.current.windSpeed} m/s`} />
              <MetricPill icon={<CloudSun class="size-3.5" />} label="Fukt" value={`${payload.current.humidity}%`} />
              <MetricPill icon={<Activity class="size-3.5" />} label="Trykk" value={`${payload.current.pressure} hPa`} />
            </div>
          </>
        )}
      />
    </InformationCardShell>
  )
}

function TrafficDashboardCard(props: { card: DashboardCard; onPrompt: (card: DashboardCard) => void }) {
  const traffic = createLiveInformationState<InformationTrafficPayload>({
    fallbackError: 'Kunne ikke hente trafikkdata.',
    load: loadTraffic,
    snapshot: (snapshot) => ({
      data: snapshot.traffic,
      error: snapshot.trafficError,
    }),
  })

  return (
    <InformationCardShell
      card={props.card}
      onPrompt={props.onPrompt}
      action={<RefreshButton label="Oppdater trafikk" loading={traffic.loading()} onClick={() => void traffic.refresh(true)} />}
    >
      <InformationCardState
        data={traffic.data()}
        error={traffic.error()}
        loading={traffic.loading()}
        render={(payload) => (
          <div class="dashboard-info-list">
            <div class="dashboard-info-row">
              <span>
                <strong>Statens vegvesen</strong>
                <small>{payload.data.length} aktive målepunkter · {traffic.usingLiveLocation() ? 'Nær deg' : 'Oslo-område'}</small>
              </span>
              <em>{formatRelativeNorwegianTime(payload.timestamp)}</em>
            </div>
            <For each={payload.data.slice(0, 2)}>
              {(station) => (
                <div class="dashboard-info-row">
                  <span>
                    <strong>{station.name}</strong>
                    <small>{station.roadReference} · {station.county}</small>
                  </span>
                  <em class="dashboard-info-row__metric">
                    {station.averageSpeed} km/t
                    <small>{station.trafficVolume.toLocaleString('nb-NO')} biler</small>
                  </em>
                </div>
              )}
            </For>
          </div>
        )}
      />
    </InformationCardShell>
  )
}

function NewsDashboardCard(props: { card: DashboardCard; onPrompt: (card: DashboardCard) => void }) {
  type NewsCategory = typeof newsCategoryOptions[number]['value']
  const [category, setCategory] = createSignal<NewsCategory>('all')
  const [data, setData] = createSignal<InformationNewsPayload | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(true)
  let cancelled = false

  const load = async (nextCategory: NewsCategory) => {
    setLoading(true)
    setError(null)
    try {
      const payload = await loadNews(nextCategory)
      if (cancelled) return
      setData(payload)
    } catch (reason) {
      if (!cancelled) setError(reason instanceof Error ? reason.message : 'Kunne ikke hente nyheter.')
    } finally {
      if (!cancelled) setLoading(false)
    }
  }

  const loadSharedNews = async () => {
    setLoading(true)
    setError(null)
    try {
      const snapshot = await loadDashboardInformationSnapshot()
      if (cancelled) return
      setData(snapshot.news)
      setError(snapshot.newsError)
    } catch (reason) {
      if (!cancelled) setError(reason instanceof Error ? reason.message : 'Kunne ikke hente nyheter.')
    } finally {
      if (!cancelled) setLoading(false)
    }
  }

  createEffect(() => {
    const selected = category()
    if (selected === 'all') {
      void loadSharedNews()
      return
    }
    void load(selected)
  })

  onCleanup(() => {
    cancelled = true
  })

  return (
    <InformationCardShell
      card={props.card}
      onPrompt={props.onPrompt}
      action={<RefreshButton label="Oppdater nyheter" loading={loading()} onClick={() => void load(category())} />}
    >
      <div class="dashboard-info-list">
        <select
          aria-label="Filtrer nyheter"
          class="dashboard-info-card__select"
          value={category()}
          onChange={(event) => setCategory(event.currentTarget.value as NewsCategory)}
        >
          <For each={newsCategoryOptions}>
            {(option) => <option value={option.value}>{option.label}</option>}
          </For>
        </select>

        <InformationCardState
          data={data()}
          error={error()}
          loading={loading()}
          render={(payload) => (
            <div class="dashboard-info-scroll">
              <For each={payload.articles}>
                {(article) => (
                  <a
                    href={article.link}
                    target="_blank"
                    rel="noopener noreferrer"
                    class="dashboard-info-row dashboard-info-row--link"
                  >
                    <span>
                      <strong>{article.title}</strong>
                      <small>{article.source} · {formatRelativeNorwegianTime(article.publishDate)}</small>
                    </span>
                    <em><ArrowUpRight class="size-3.5" /></em>
                  </a>
                )}
              </For>
            </div>
          )}
        />
      </div>
    </InformationCardShell>
  )
}

function InformationCardState<T>(props: {
  data: T | null
  error: string | null
  loading: boolean
  render: (payload: T) => JSX.Element
  skeletonLines?: number
}) {
  return (
    <>
      <Show when={props.loading && !props.data}>
        <InformationSkeleton lines={props.skeletonLines ?? 3} />
      </Show>
      <Show when={!props.loading && props.error}>
        {(message) => <InformationError text={message()} />}
      </Show>
      <Show when={props.data}>
        {(payload) => props.render(payload())}
      </Show>
    </>
  )
}

function InformationCardShell(props: {
  action: JSX.Element
  card: DashboardCard
  children: JSX.Element
  onPrompt: (card: DashboardCard) => void
}) {
  return (
    <div class="velion-dashboard-card dashboard-info-card">
      <div class="velion-dashboard-card-label">
        {props.card.category}
      </div>

      <div class="dashboard-info-card__surface">
        <div class="dashboard-info-card__head">
          <div class="min-w-0">
            <h3>{props.card.title}</h3>
          </div>
          {props.action}
        </div>
        {props.children}
      </div>

      <button
        type="button"
        onClick={() => props.onPrompt(props.card)}
        class="dashboard-info-card__ask"
        aria-label={`Start chat for ${props.card.title}`}
        title={`Start chat for ${props.card.title}`}
      >
        <Newspaper class="size-3.5" />
        Ask Velion
      </button>
    </div>
  )
}

function RefreshButton(props: { label: string; loading?: boolean; onClick?: () => void }) {
  return (
    <button
      type="button"
      class="dashboard-info-card__refresh"
      aria-label={props.label}
      title={props.label}
      onClick={(event) => {
        event.preventDefault()
        event.stopPropagation()
        props.onClick?.()
      }}
    >
      <RefreshCw class={cn('size-4', props.loading && 'dashboard-info-card__refresh-icon--spinning')} />
    </button>
  )
}

function MetricPill(props: { icon: JSX.Element; label: string; value: string }) {
  return (
    <div class="dashboard-info-metric">
      <div>
        {props.icon}
        <span>{props.label}</span>
      </div>
      <p>{props.value}</p>
    </div>
  )
}

function InformationSkeleton(props: { lines: number }) {
  return (
    <div class="dashboard-info-skeleton">
      <For each={Array.from({ length: props.lines })}>
        {() => <div />}
      </For>
    </div>
  )
}

function InformationError(props: { text: string }) {
  return <div class="dashboard-info-error">{props.text}</div>
}
