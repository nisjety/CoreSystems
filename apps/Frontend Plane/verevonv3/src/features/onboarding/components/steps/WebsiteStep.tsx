import { CheckCircle2, Circle, Globe, Loader2, RefreshCw } from '@/shared/icons'
import { For, Match, Show, Switch } from 'solid-js'
import { Button } from '@/shared/ui/Button'
import { type OnboardingState, onboardingCrawlPhases } from '@/features/onboarding/lib/model'
import { activeCrawlPhase, stripUrlProtocol, websiteProgressPercent } from '@/features/onboarding/lib/view'
import { OnboardingField } from '@/features/onboarding/components/shared/OnboardingField'
import { OnboardingLinkButton } from '@/features/onboarding/components/shared/OnboardingLinkButton'

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
  return (
    <div class="onboarding-website-visual">
      <div class="onboarding-folder-card">
        <div class="onboarding-folder-card__tab" />
        <p>Nettsidekunnskap</p>
        <h2>{props.website.snippets.length} utdrag samlet</h2>
        <span>
          <Switch fallback="Quarry henter strukturert tekst, bilder og filer fra nettstedet ditt.">
            <Match when={props.website.status === 'completed'}>
              Rask forhåndsvisning fra forsiden din. Hele nettstedet indekseres når arbeidsområdet er klart.
            </Match>
          </Switch>
        </span>
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
