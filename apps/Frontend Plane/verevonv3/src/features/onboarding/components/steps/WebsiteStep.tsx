import { CheckCircle2, Circle, Globe, Loader2, RefreshCw } from '@/shared/icons'
import { For, Match, Show, Switch, createMemo } from 'solid-js'
import { Button } from '@/shared/ui/Button'
import type { CrawlSnippet } from '@/features/onboarding/lib/api'
import { dedupeCrawlSnippets } from '@/features/onboarding/lib/crawl-preview'
import { type OnboardingState, onboardingCrawlPhases } from '@/features/onboarding/lib/model'
import { activeCrawlPhase, stripUrlProtocol, websiteProgressPercent } from '@/features/onboarding/lib/view'
import { OnboardingField } from '@/features/onboarding/components/shared/OnboardingField'
import { OnboardingLinkButton } from '@/features/onboarding/components/shared/OnboardingLinkButton'
import './WebsiteStep.css'

const VISIBLE_PAGE_CARDS = 5

/** `/om-oss` for `https://www.aquatiq.com/om-oss/`; the bare host for the root. */
export function snippetPathLabel(snippet: Pick<CrawlSnippet, 'url'>): string {
  try {
    const url = new URL(snippet.url)
    const path = url.pathname.replace(/\/+$/, '')
    return path ? `${url.host.replace(/^www\./, '')}${path}` : url.host.replace(/^www\./, '')
  } catch {
    return snippet.url
  }
}

/** Newest first, one card per page, richest version of each. */
export function visibleCrawlPages(snippets: readonly CrawlSnippet[], limit = VISIBLE_PAGE_CARDS): CrawlSnippet[] {
  return dedupeCrawlSnippets(snippets).slice(-limit).reverse()
}

type WebsiteStepContentProps = {
  website: OnboardingState['website']
  onBriefInput: (value: string) => void
  onContinue: () => void
  onRunPreview: () => void | Promise<void>
  onSkip: () => void
  onUrlInput: (value: string) => void
}

export function WebsiteStepContent(props: WebsiteStepContentProps) {
  return (
    <section class="onboarding-copy onboarding-copy--website">
      <p class="onboarding-eyebrow">Nettside</p>
      <h1>Legg Til Nettside</h1>
      <p>Lim inn firmaets URL. Vi leser de offentlige sidene og gjør dem til agentens første kunnskapsbase. Du kan legge til flere kilder rett etterpå.</p>

      <OnboardingField label="Nettside-URL" class="onboarding-field--url">
        <div class="onboarding-url-control">
          <span class="onboarding-url-prefix">
            <Globe size={14} /> https://
          </span>
          <input
            value={stripUrlProtocol(props.website.url)}
            readonly={props.website.status === 'starting' || props.website.status === 'running'}
            onInput={(event) => {
              const raw = event.currentTarget.value.replace(/^https?:\/\//, '').replace(/\s+/g, '')
              props.onUrlInput(raw ? `https://${raw}` : '')
            }}
            placeholder="coresystem.com"
            inputmode="url"
          />
          <button
            type="button"
            class="onboarding-url-recrawl"
            disabled={!props.website.url || props.website.status === 'starting' || props.website.status === 'running'}
            aria-label={props.website.status === 'idle' ? 'Analyser nettside' : 'Analyser nettside på nytt'}
            title={props.website.status === 'idle' ? 'Analyser nettside' : 'Analyser nettside på nytt'}
            onClick={() => void props.onRunPreview()}
          >
            <Show
              when={props.website.status === 'starting' || props.website.status === 'running'}
              fallback={<RefreshCw size={15} />}
            >
              <Loader2 size={15} class="onboarding-phase-spinner" />
            </Show>
          </button>
        </div>
      </OnboardingField>

      <Show when={props.website.status !== 'starting' && props.website.status !== 'running'}>
        <OnboardingField label="Hva skal agenten hjelpe med?" optionalLabel="valgfritt">
          <textarea
            rows="2"
            value={props.website.brief}
            onInput={(event) => props.onBriefInput(event.currentTarget.value)}
            placeholder="vi trenger en chatbot som er integrert i vår webside og shopify"
          />
        </OnboardingField>
      </Show>

      <Show when={props.website.status !== 'idle'}>
        <div class="onboarding-phase-list">
          <For each={onboardingCrawlPhases}>
            {(phase, i) => {
              const phaseIdx = () => activeCrawlPhase(props.website)
              const done = () => i() < phaseIdx()
              const active = () => i() === phaseIdx()

              return (
                <div
                  class={[
                    'onboarding-phase-row',
                    {
                      'onboarding-phase-row--done': done(),
                      'onboarding-phase-row--active': active(),
                    },
                  ]}
                >
                  <Show
                    when={done()}
                    fallback={
                      <Show when={active()} fallback={<Circle size={15} />}>
                        <Loader2 size={15} class="onboarding-phase-spinner" />
                      </Show>
                    }
                  >
                    <CheckCircle2 size={15} />
                  </Show>
                  <span>{phase}</span>
                </div>
              )
            }}
          </For>
        </div>
      </Show>

      <Show when={props.website.warning}>
        <p class="onboarding-warning">{props.website.warning}</p>
      </Show>

      <div class="onboarding-actions">
        <Switch>
          <Match when={props.website.status === 'starting' || props.website.status === 'running'}>
            <Button variant="primary" size="sm" onClick={props.onContinue}>
              Fortsett mens vi jobber
            </Button>
          </Match>
          <Match when={props.website.status === 'completed'}>
            <Button variant="primary" size="sm" onClick={props.onContinue}>
              Fortsett
            </Button>
          </Match>
          <Match when={props.website.status === 'failed'}>
            <Button variant="primary" size="sm" onClick={() => void props.onRunPreview()}>
              Prøv igjen
            </Button>
            <OnboardingLinkButton onClick={props.onSkip}>Hopp over</OnboardingLinkButton>
          </Match>
          <Match when={true}>
            <>
              <Button
                variant="primary"
                size="sm"
                disabled={!props.website.url}
                onClick={() => void props.onRunPreview()}
              >
                Analyser nettside
              </Button>
              <OnboardingLinkButton onClick={props.onSkip}>Hopp over nå</OnboardingLinkButton>
            </>
          </Match>
        </Switch>
      </div>
    </section>
  )
}

export function WebsiteStepVisual(props: { website: OnboardingState['website'] }) {
  const pages = createMemo(() => dedupeCrawlSnippets(props.website.snippets))
  const visible = createMemo(() => visibleCrawlPages(props.website.snippets))
  const hidden = () => Math.max(0, pages().length - visible().length)
  const crawling = () => props.website.status === 'starting' || props.website.status === 'running'

  return (
    <div class="onboarding-website-visual">
      <div class={['onboarding-folder-card', { 'onboarding-folder-card--pages': pages().length > 0 }]}>
        <div class="onboarding-folder-card__tab" />
        <p>Nettsidekunnskap</p>
        <h2>
          <Switch fallback={`${pages().length} sider lest`}>
            <Match when={pages().length === 0 && crawling()}>Leser nettsiden …</Match>
            <Match when={pages().length === 0}>0 sider lest</Match>
            <Match when={pages().length === 1}>1 side lest</Match>
          </Switch>
        </h2>
        <span>
          <Switch fallback="Quarry henter strukturert tekst, bilder og filer fra nettstedet ditt.">
            <Match when={props.website.status === 'completed'}>
              Rask forhåndsvisning fra forsiden din. Hele nettstedet indekseres når arbeidsområdet er klart.
            </Match>
          </Switch>
        </span>
        <Show when={visible().length > 0}>
          <ul class="onboarding-folder-card__pages" aria-label="Sider funnet på nettstedet" aria-live="polite">
            <For each={visible()}>
              {(snippet) => {
                const text = () => snippet.excerpt?.trim() || snippet.summary?.trim() || ''
                const pending = () => !text()
                return (
                  <li
                    class={['onboarding-page-card', { 'onboarding-page-card--pending': pending() }]}
                    data-title-source={snippet.titleSource ?? 'unknown'}
                  >
                    <div class="onboarding-page-card__head">
                      <strong class="onboarding-page-card__title" title={snippet.title}>
                        {snippet.title}
                      </strong>
                      <Show when={snippet.titleSource === 'model'}>
                        <span class="onboarding-page-card__badge" title="Navnet er foreslått av AI fra sideteksten">
                          AI-navn
                        </span>
                      </Show>
                    </div>
                    <span class="onboarding-page-card__path" title={snippet.url}>
                      {snippetPathLabel(snippet)}
                    </span>
                    <Show
                      when={!pending()}
                      fallback={
                        <p class="onboarding-page-card__excerpt onboarding-page-card__excerpt--pending">
                          {crawling() ? 'Henter tekst …' : 'Ingen lesbar tekst funnet på denne siden.'}
                        </p>
                      }
                    >
                      <p class="onboarding-page-card__excerpt">{text()}</p>
                    </Show>
                    <Show when={snippet.wordCount || snippet.summary}>
                      <div class="onboarding-page-card__meta">
                        <Show when={snippet.wordCount}>
                          <span>{snippet.wordCount} ord</span>
                        </Show>
                        <Show when={snippet.summary && snippet.excerpt}>
                          <span title={snippet.summary}>Oppsummert</span>
                        </Show>
                      </div>
                    </Show>
                  </li>
                )
              }}
            </For>
          </ul>
          <Show when={hidden() > 0}>
            <p class="onboarding-folder-card__more">+ {hidden()} flere sider</p>
          </Show>
        </Show>
        <div class="onboarding-folder-card__progress">
          <div>
            <span>Fremdrift</span>
            <strong>{websiteProgressPercent(props.website)}%</strong>
          </div>
          <meter min="0" max="100" value={websiteProgressPercent(props.website)} />
        </div>
      </div>
    </div>
  )
}
