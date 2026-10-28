import { createMemo, createSignal, For, Show } from 'solid-js'
import { Bell, Eye, Loader, Plus, ScanSearch, Telescope, Trash2 } from '@/shared/icons'
import { createResource } from '@/shared/lib/create-resource-compat'
import { translateApiError, useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonSegmented, VerevonSegmentedButton } from '@/shared/ui/verevon/VerevonSegmented'
import {
  checkUrlNow,
  createMonitor,
  deleteMonitor,
  getChangeHistory,
  type ChangeMonitor,
  type ChangeRecord,
  type MonitorPreset,
} from '@/shared/api/monitoring-client'
import { changeCount, changeTimeline, shortFingerprint } from '@/features/ingestions/lib/monitoring'
import { describeCron } from '@/features/ingestions/lib/cron'
import { EmptyState, NeedsBackend, relativeTime } from '@/features/ingestions/components/ingestions-ui'

const presets: MonitorPreset[] = ['hourly', 'daily', 'weekly']

function presetLabel(preset: MonitorPreset, tr: (no: string, en: string) => string) {
  switch (preset) {
    case 'hourly':
      return tr('Hver time', 'Hourly')
    case 'daily':
      return tr('Daglig', 'Daily')
    case 'weekly':
      return tr('Ukentlig', 'Weekly')
  }
}

/**
 * Browse AI-style monitoring: recurring monitors (Quarry change_monitor
 * schedules), an on-demand check, and the baseline timeline of the watched page
 * with changed/unchanged markers. Quarry stores fingerprints only, so there is no
 * text diff to show; the timeline says when content changed, not how.
 */
export function IngestionsMonitoringView(props: {
  monitors: ChangeMonitor[] | null
  onMonitorsChanged: () => Promise<void>
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [urlInput, setUrlInput] = createSignal('')
  const [preset, setPreset] = createSignal<MonitorPreset>('daily')
  const [watchedUrl, setWatchedUrl] = createSignal<string | null>(null)
  const [checking, setChecking] = createSignal(false)
  const [saving, setSaving] = createSignal(false)
  const [removingId, setRemovingId] = createSignal<string | null>(null)
  const [lastResult, setLastResult] = createSignal<ChangeRecord | null>(null)
  const [error, setError] = createSignal<string | null>(null)

  // On-demand history for the committed URL. Empty until baselines accrue —
  // every row traces to a real edge baseline; nothing is synthesized.
  const [history, { refetch: refetchHistory }] = createResource(watchedUrl, (url) => getChangeHistory(url))
  const timeline = createMemo(() => changeTimeline(history() ?? []))

  async function check() {
    const candidate = urlInput().trim()
    if (!candidate) {
      setError(tr('Angi en URL som skal sjekkes.', 'Enter a URL to check.'))
      return
    }
    setError(null)
    setChecking(true)
    try {
      setWatchedUrl(candidate)
      const result = await checkUrlNow(candidate)
      setLastResult(result)
      // The check may have produced a new baseline upstream; refresh history.
      void refetchHistory()
    } catch (caught) {
      setError(translateApiError(caught, tr, { no: 'Sjekken kunne ikke fullføres.', en: 'Check could not be completed.' }))
    } finally {
      setChecking(false)
    }
  }

  async function addMonitor() {
    const candidate = urlInput().trim()
    if (!candidate) {
      setError(tr('Angi en URL som skal overvåkes.', 'Enter a URL to monitor.'))
      return
    }
    setError(null)
    setSaving(true)
    try {
      await createMonitor(candidate, preset())
      await props.onMonitorsChanged()
    } catch (caught) {
      setError(translateApiError(caught, tr, { no: 'Overvåkeren kunne ikke opprettes.', en: 'The monitor could not be created.' }))
    } finally {
      setSaving(false)
    }
  }

  async function removeMonitor(id: string) {
    setError(null)
    setRemovingId(id)
    try {
      await deleteMonitor(id)
      await props.onMonitorsChanged()
    } catch (caught) {
      setError(translateApiError(caught, tr, { no: 'Overvåkeren kunne ikke fjernes.', en: 'The monitor could not be removed.' }))
    } finally {
      setRemovingId(null)
    }
  }

  function watch(url: string) {
    setUrlInput(url)
    setWatchedUrl(url)
  }

  return (
    <section class="ingestions-monitoring">
      <form
        class="verevon-panel ingestions-card ingestions-composer"
        onSubmit={(event) => {
          event.preventDefault()
          void check()
        }}
      >
        <div>
          <h2>{tr('Sjekk en side for endringer', 'Check a page for changes')}</h2>
          <p>
            {tr(
              'Verevon henter siden, fingeravtrykker innholdet og sammenligner med forrige basislinje. Gjør det til en overvåker for å sjekke automatisk.',
              'Verevon fetches the page, fingerprints its content and compares it with the last baseline. Turn it into a monitor to check automatically.',
            )}
          </p>
        </div>
        <label class="ingestions-field">
          {tr('Side-URL', 'Page URL')}
          <VerevonInput
            value={urlInput()}
            onInput={(event) => setUrlInput(event.currentTarget.value)}
            placeholder="https://example.com/pricing"
          />
        </label>
        <Button variant="primary" fullWidth type="submit" disabled={checking()}>
          <ScanSearch class={cn('size-4', checking() && 'ingestions-spin')} strokeWidth={1.9} />
          {checking() ? tr('Sjekker …', 'Checking…') : tr('Sjekk nå', 'Check now')}
        </Button>

        <div class="ingestions-monitor-maker">
          <span class="ingestions-muted-label">{tr('Sjekk automatisk', 'Check automatically')}</span>
          <VerevonSegmented aria-label={tr('Hvor ofte', 'How often')}>
            <For each={presets}>
              {(entry) => (
                <VerevonSegmentedButton selected={preset() === entry} onClick={() => setPreset(entry)}>
                  {presetLabel(entry, tr)}
                </VerevonSegmentedButton>
              )}
            </For>
          </VerevonSegmented>
          <Button variant="secondary" fullWidth disabled={saving()} onClick={() => void addMonitor()}>
            <Show when={saving()} fallback={<Plus class="size-4" strokeWidth={1.9} />}>
              <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
            </Show>
            {tr('Opprett overvåker', 'Create monitor')}
          </Button>
          <small class="ingestions-field__help">
            {tr('Du får et varsel i appen når innholdet endrer seg.', 'You get an in-app notification when the content changes.')}
          </small>
        </div>

        <Show when={error()}>
          {(message) => (
            <div class="ingestions-alert" role="alert">
              {message()}
            </div>
          )}
        </Show>

        <Show when={lastResult()}>
          {(result) => (
            <div class="ingestions-code-card">
              <div class="ingestions-title-row">
                <Telescope class="size-4" strokeWidth={1.9} />
                <h3>{tr('Siste sjekk', 'Latest check')}</h3>
                <ChangeStatusBadge status={result().status} />
              </div>
              <div class="ingestions-meta-row ingestions-meta-row--spread">
                <span class="ingestions-truncate">{result().sourceUrl}</span>
                <span>{relativeTime(result().checkedAt, i18n)}</span>
              </div>
              <Show
                when={result().prevBaseline}
                fallback={
                  <p>
                    {tr(
                      'Ingen tidligere basislinje — dette er første gang dette arbeidsområdet har sjekket denne siden.',
                      'No earlier baseline — this is the first time this workspace has checked this page.',
                    )}
                  </p>
                }
              >
                {(prev) => (
                  <p>
                    {tr('Forrige basislinje ble fanget', 'Previous baseline captured')} {relativeTime(prev().capturedAt, i18n)}.
                  </p>
                )}
              </Show>
            </div>
          )}
        </Show>
      </form>

      <div class="ingestions-monitoring__side">
        <section class="verevon-panel ingestions-card">
          <div class="ingestions-card__header">
            <div>
              <h2>{tr('Overvåkere', 'Monitors')}</h2>
              <p>{tr('Sider Verevon sjekker for endringer på fast frekvens.', 'Pages Verevon checks for changes on a fixed cadence.')}</p>
            </div>
          </div>
          <Show
            when={props.monitors}
            fallback={<div class="ingestions-empty-box">{tr('Overvåkere kunne ikke lastes.', 'Monitors could not be loaded.')}</div>}
          >
            {(monitors) => (
              <div class="ingestions-card-list">
                <For
                  each={monitors()}
                  fallback={
                    <EmptyState icon={<Bell class="size-5" strokeWidth={1.8} />} title={tr('Ingen overvåkere ennå.', 'No monitors yet.')}>
                      {tr('Lim inn en URL, velg frekvens og opprett en overvåker.', 'Paste a URL, pick a cadence and create a monitor.')}
                    </EmptyState>
                  }
                >
                  {(monitor) => (
                    <article class={cn('ingestions-list-card', watchedUrl() === monitor.url && 'ingestions-list-card--selected')}>
                      <div class="ingestions-title-row ingestions-title-row--spread">
                        <div class="ingestions-min-width">
                          <h3 class="ingestions-truncate">{monitor.url}</h3>
                          <p>
                            {describeCron(monitor.cron, tr) ?? monitor.cron}
                            {' · '}
                            {monitor.lastRunAt
                              ? `${tr('sist', 'last')} ${relativeTime(monitor.lastRunAt, i18n)}`
                              : tr('ikke kjørt ennå', 'not run yet')}
                          </p>
                        </div>
                        <span class={cn('ingestions-status', monitor.enabled ? 'ingestions-status--success' : 'ingestions-status--neutral')}>
                          {monitor.enabled ? tr('aktiv', 'active') : tr('pauset', 'paused')}
                        </span>
                      </div>
                      <div class="ingestions-actions-row">
                        <Button size="xs" onClick={() => watch(monitor.url)}>
                          <Eye class="size-4" strokeWidth={1.9} />
                          {tr('Vis historikk', 'Show history')}
                        </Button>
                        <Button size="xs" variant="ghost" disabled={removingId() === monitor.id} onClick={() => void removeMonitor(monitor.id)}>
                          <Show when={removingId() === monitor.id} fallback={<Trash2 class="size-4" strokeWidth={1.9} />}>
                            <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
                          </Show>
                          {tr('Stopp', 'Stop')}
                        </Button>
                      </div>
                    </article>
                  )}
                </For>
              </div>
            )}
          </Show>
        </section>

        <section class="verevon-panel ingestions-card ingestions-runs-panel">
          <div class="ingestions-card__header">
            <div>
              <h2>{tr('Endringshistorikk', 'Change history')}</h2>
              <p>
                <Show
                  when={watchedUrl()}
                  fallback={tr('Fangede basislinjer, nyeste først.', 'Captured baselines, newest first.')}
                >
                  {(url) => <>{url()}</>}
                </Show>
              </p>
            </div>
            <Show when={timeline().length > 0}>
              <span class="ingestions-status ingestions-status--info">
                {tr(`${changeCount(timeline())} endringer`, `${changeCount(timeline())} changes`)}
              </span>
            </Show>
          </div>
          <Show
            when={watchedUrl()}
            fallback={<div class="ingestions-empty-box">{tr('Kjør en sjekk for å se basislinjehistorikken til denne siden.', "Run a check to see this page's baseline history.")}</div>}
          >
            <Show
              when={!history.loading}
              fallback={<div class="ingestions-empty-box">{tr('Laster historikk …', 'Loading history…')}</div>}
            >
              <ol class="ingestions-timeline">
                <For
                  each={timeline()}
                  fallback={<li class="ingestions-muted">{tr('Ingen basislinjer registrert for denne siden ennå.', 'No baselines recorded for this page yet.')}</li>}
                >
                  {(entry) => (
                    <li class={cn('ingestions-timeline__item', `ingestions-timeline__item--${entry.change}`)}>
                      <span class="ingestions-timeline__dot" aria-hidden="true" />
                      <div class="ingestions-min-width">
                        <strong>
                          {entry.change === 'changed'
                            ? tr('Innhold endret', 'Content changed')
                            : entry.change === 'unchanged'
                              ? tr('Uendret', 'Unchanged')
                              : tr('Første basislinje', 'First baseline')}
                        </strong>
                        <small>
                          {relativeTime(entry.capturedAt, i18n)} · <span class="ingestions-mono">{shortFingerprint(entry.fingerprint)}</span>
                          {entry.runId ? ` · ${entry.runId}` : ''}
                        </small>
                      </div>
                    </li>
                  )}
                </For>
              </ol>
              <div class="ingestions-pill-row">
                <NeedsBackend
                  label={tr('Tekstdiff', 'Text diff')}
                  reason={tr('Quarry lagrer bare fingeravtrykk per basislinje, så det finnes ingen tekst å sammenligne ennå.', 'Quarry stores only a fingerprint per baseline, so there is no text to compare yet.')}
                />
                <NeedsBackend
                  label={tr('E-post/webhook', 'Email/webhook')}
                  reason={tr('Endringsvarsler går bare i appen i dag.', 'Change alerts go in-app only today.')}
                />
              </div>
            </Show>
          </Show>
        </section>
      </div>
    </section>
  )
}

function ChangeStatusBadge(props: { status: ChangeRecord['status'] }) {
  const tone = () => {
    if (props.status === 'changed' || props.status === 'unreachable') return 'ingestions-status--warning'
    if (props.status === 'new') return 'ingestions-status--info'
    return 'ingestions-status--success'
  }
  return <span class={cn('ingestions-status', tone())}>{props.status}</span>
}
