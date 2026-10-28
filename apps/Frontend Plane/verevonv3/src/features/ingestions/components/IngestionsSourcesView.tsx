import { For, Show } from 'solid-js'
import { ArrowUpRight, Loader, Plus, Trash2 } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonSelect } from '@/shared/ui/verevon/VerevonSelect'
import type { ProfilePayload, SourcePayload } from '@/shared/api/ingestions-client'
import { StatusBadge, relativeTime } from '@/features/ingestions/components/ingestions-ui'

export type SourceFormState = {
  name: string
  url: string
  kind: string
  monitor: boolean
}

export function SourcesPanel(props: {
  sources: SourcePayload | null
  form: SourceFormState
  onFormChange: (patch: Partial<SourceFormState>) => void
  onCreate: () => Promise<void>
  creating: boolean
  deletingId: string | null
  onDelete: (id: string) => Promise<void>
}) {
  const i18n = useI18n()
  const canSubmit = () =>
    !props.creating && props.form.name.trim().length > 0 && props.form.url.trim().length > 0

  return (
    <section class="ingestions-sources-grid">
      <div class="verevon-panel ingestions-card">
        <div class="ingestions-card__header">
          <div>
            <h2>{i18n.tr('Tilkoblede kilder', 'Connected sources')}</h2>
            <p>{i18n.tr('Integrasjonsbasert kunnskap og nettsted-mål for innhenting synlige for Verevon.', 'Integration-backed knowledge and website ingestion targets visible to Verevon.')}</p>
          </div>
          <a href="/knowledge" link class="ingestions-inline-link">
            {i18n.tr('Åpne Kunnskap', 'Open Knowledge')}
            <ArrowUpRight class="size-4" strokeWidth={1.9} />
          </a>
        </div>
        <div class="ingestions-card-list">
          <For
            each={props.sources?.integrations ?? []}
            fallback={<div class="ingestions-empty-box">{i18n.tr('Ingen tilkoblede integrasjoner ennå.', 'No connected integrations yet.')}</div>}
          >
            {(source) => (
              <article class="ingestions-list-card">
                <div class="ingestions-title-row ingestions-title-row--spread">
                  <div>
                    <h3>{source.title}</h3>
                    <p>
                      {source.provider} - {source.detail}
                    </p>
                  </div>
                  <StatusBadge status={source.status} />
                </div>
                <Show when={source.capabilities.length > 0}>
                  <div class="ingestions-chip-row">
                    <For each={source.capabilities.slice(0, 4)}>
                      {(capability) => <span>{capability}</span>}
                    </For>
                  </div>
                </Show>
              </article>
            )}
          </For>
        </div>
      </div>
      <div class="verevon-panel ingestions-card">
        <h2>{i18n.tr('Sporede nettkilder', 'Tracked web sources')}</h2>
        <p>{i18n.tr('Varige kilderessurser registrert i Quarry for gjentakende oppdatering og gjennomgang.', 'Durable source resources registered in Quarry for recurring refresh and review.')}</p>
        <form
          class="ingestions-source-form"
          onSubmit={(event) => {
            event.preventDefault()
            void props.onCreate()
          }}
        >
          <label class="ingestions-field">
            {i18n.tr('Navn', 'Name')}
            <VerevonInput
              value={props.form.name}
              onInput={(event) => props.onFormChange({ name: event.currentTarget.value })}
              placeholder={i18n.tr('Acme prisside', 'Acme pricing page')}
              disabled={props.creating}
            />
          </label>
          <label class="ingestions-field">
            {i18n.tr('URL', 'URL')}
            <VerevonInput
              value={props.form.url}
              onInput={(event) => props.onFormChange({ url: event.currentTarget.value })}
              placeholder="https://example.com/pricing"
              disabled={props.creating}
            />
          </label>
          <label class="ingestions-field">
            {i18n.tr('Type', 'Kind')}
            <VerevonSelect
              value={props.form.kind}
              onChange={(event) => props.onFormChange({ kind: event.currentTarget.value })}
              disabled={props.creating}
            >
              <option value="crawl">{i18n.tr('Crawl', 'Crawl')}</option>
              <option value="scrape">{i18n.tr('Scrape', 'Scrape')}</option>
              <option value="search">{i18n.tr('Søk', 'Search')}</option>
            </VerevonSelect>
          </label>
          <label class="ingestions-checkbox-field">
            <input
              type="checkbox"
              checked={props.form.monitor}
              onChange={(event) => props.onFormChange({ monitor: event.currentTarget.checked })}
              disabled={props.creating}
            />
            {i18n.tr('Overvåk for endringer (daglig)', 'Monitor for changes (daily)')}
          </label>
          <Button variant="primary" fullWidth type="submit" disabled={!canSubmit()}>
            <Show
              when={props.creating}
              fallback={
                <>
                  <Plus class="size-4" strokeWidth={1.9} />
                  {i18n.tr('Legg til kilde', 'Add source')}
                </>
              }
            >
              <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
              {i18n.tr('Legger til …', 'Adding…')}
            </Show>
          </Button>
        </form>
        <div class="ingestions-card-list">
          <For
            each={props.sources?.quarrySources ?? []}
            fallback={<div class="ingestions-empty-box">{i18n.tr('Quarry har ingen varige kilderegistreringer ennå.', 'Quarry has no durable source records yet.')}</div>}
          >
            {(source) => (
              <article class="ingestions-list-card">
                <div class="ingestions-title-row ingestions-title-row--spread">
                  <div class="ingestions-min-width">
                    <h3>{source.name}</h3>
                    <p class="ingestions-truncate">{source.url}</p>
                  </div>
                  <StatusBadge status={source.status} />
                </div>
                <div class="ingestions-meta-row ingestions-meta-row--spread">
                  <span class="ingestions-capitalize">{source.kind}</span>
                  <span class="ingestions-source-meta-actions">
                    <Show when={source.updatedAt}>{relativeTime(source.updatedAt, i18n)}</Show>
                    <Button
                      size="xs"
                      variant="ghost"
                      disabled={props.deletingId === source.id}
                      onClick={() => void props.onDelete(source.id)}
                    >
                      <Show
                        when={props.deletingId === source.id}
                        fallback={<Trash2 class="size-4" strokeWidth={1.9} />}
                      >
                        <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
                      </Show>
                      {i18n.tr('Fjern', 'Remove')}
                    </Button>
                  </span>
                </div>
              </article>
            )}
          </For>
        </div>
      </div>
    </section>
  )
}

export function ProfilesPanel(props: { profiles: ProfilePayload | null }) {
  const i18n = useI18n()
  return (
    <section class="verevon-panel ingestions-card">
      <h2>{i18n.tr('Profiler', 'Profiles')}</h2>
      <p>{i18n.tr('Nettleser-/øktprofiler for beskyttede kilder og tilstandsbaserte oppdateringsflyter.', 'Browser/session profiles for protected sources and stateful refresh flows.')}</p>
      <div class="ingestions-profile-grid">
        <For
          each={props.profiles?.profiles ?? []}
          fallback={<div class="ingestions-empty-box">{i18n.tr('Ingen lagrede profiler ennå.', 'No saved profiles yet.')}</div>}
        >
          {(profile) => (
            <article class="ingestions-list-card">
              <div class="ingestions-title-row ingestions-title-row--spread">
                <h3>{profile.id}</h3>
                <StatusBadge status={profile.restorable ? 'ready' : 'review'} />
              </div>
              <div class="ingestions-profile-details">
                <span>{i18n.tr('Informasjonskapsler', 'Cookies')}: {profile.cookies}</span>
                <span>{i18n.tr('Lagringsoppføringer', 'Storage entries')}: {profile.storage}</span>
                <span>
                  {profile.locale || i18n.tr('Ingen lokalitet', 'No locale')} - {profile.timezone || i18n.tr('Ingen tidssone', 'No timezone')}
                </span>
              </div>
            </article>
          )}
        </For>
      </div>
    </section>
  )
}
