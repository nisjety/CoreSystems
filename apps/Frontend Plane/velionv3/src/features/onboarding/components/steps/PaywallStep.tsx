import { createMemo, For, Show } from 'solid-js'
import type { CheckoutSession, PlanRecommendation } from '@/features/onboarding/lib/api'
import { type PlanId, onboardingPlanCards } from '@/features/onboarding/lib/model'
import { Button } from '@/shared/ui/Button'
import { VelionSwitch } from '@/shared/ui/velion/VelionSwitch'
import { HyperswitchCheckout } from '@/features/billing/components/HyperswitchCheckout'

type PaywallStepProps = {
  activePlanId: PlanId
  checkoutReturnUrl: string
  checkoutSession?: CheckoutSession
  committing?: boolean
  confirmingCheckout?: boolean
  error?: string
  loadingRecommendation: boolean
  onConfirmCheckout: (payment: { paymentId?: string; clientSecret?: string; status: string }) => void | Promise<void>
  onCommitPlan: () => void | Promise<void>
  onRefreshRecommendation?: () => void | Promise<void>
  onSelectPlan: (planId: PlanId) => void
  recommendation?: PlanRecommendation
}

export function PaywallStep(props: PaywallStepProps) {
  const defaultPlan = onboardingPlanCards[0]!
  const hasRecommendation = () => Boolean(props.recommendation)
  const recommendedPlanId = () => props.recommendation?.planId ?? 'trial'
  const recommendedPlan = createMemo(() => (
    onboardingPlanCards.find((plan) => plan.id === recommendedPlanId()) ?? defaultPlan
  ))
  const activePlan = createMemo(() => (
    onboardingPlanCards.find((plan) => plan.id === props.activePlanId) ?? defaultPlan
  ))
  const proofPoints = createMemo(() => props.recommendation?.proofPoints?.filter(Boolean).slice(0, 3) ?? [])
  const scopeSignals = createMemo(() => props.recommendation?.scopeSignals?.filter(Boolean).slice(0, 4) ?? [])
  const opportunities = createMemo(() => props.recommendation?.opportunities?.filter(Boolean).slice(0, 2) ?? [])
  const suggestionSummary = () => (
    props.loadingRecommendation
      ? 'Velion AI analyserer nettsted, organisasjon og kildene du har valgt.'
      : props.recommendation?.summary ||
        'Lite signal ennå. Gratis lar deg teste i 14 dager før Velion anbefaler en betalt plan.'
  )
  const suggestionTitle = () => {
    if (props.loadingRecommendation) return 'Velion AI finner beste plan'
    return `${recommendedPlan().name} anbefales`
  }
  const suggestionKicker = () => {
    if (props.loadingRecommendation) return 'Analyserer'
    return props.recommendation?.source === 'model' ? 'Velion AI-forslag' : 'Velion forslag'
  }

  return (
    <section class="onboarding-paywall">
      <div class="onboarding-paywall__header">
        <div>
          <h1>Din beste match</h1>
          <p>Velg planen som passer behovet ditt</p>
        </div>
        <div class="onboarding-paywall__billing">
          <span>Månedlig</span>
          <VelionSwitch label="Billing period" />
          <span>Årlig</span>
          <span class="onboarding-paywall__trial-pill">14 dager gratis</span>
        </div>
      </div>

      <div class="onboarding-paywall__grid">
        <For each={onboardingPlanCards}>
          {(plan) => (
            <article
              class="onboarding-paywall-card"
              classList={{
                'onboarding-paywall-card--active': props.activePlanId === plan.id,
                'onboarding-paywall-card--recommended': hasRecommendation() && recommendedPlanId() === plan.id,
              }}
            >
              <div class="onboarding-paywall-card__head">
                <strong>{plan.name}</strong>
                <span class="onboarding-paywall-card__badges">
                  <Show when={plan.id === 'trial'}>
                    <span class="onboarding-paywall-card__badge onboarding-paywall-card__badge--trial">
                      Aktiv prøve
                    </span>
                  </Show>
                  <Show when={hasRecommendation() && recommendedPlanId() === plan.id}>
                    <span class="onboarding-paywall-card__badge">Velion foreslår</span>
                  </Show>
                  <Show when={props.activePlanId === plan.id && recommendedPlanId() !== plan.id}>
                    <span class="onboarding-paywall-card__badge onboarding-paywall-card__badge--choice">
                      Ditt valg
                    </span>
                  </Show>
                </span>
              </div>
              <div class="onboarding-paywall-card__price">
                <span>{plan.price === 'Custom' ? 'Tilpasset' : `${plan.price} kr`}</span>
                <Show when={plan.price !== 'Custom'}>
                  <small>/mnd</small>
                </Show>
              </div>
              <p>{plan.description}</p>
              <ul>
                <For each={plan.features}>{(item) => <li>{item}</li>}</For>
              </ul>
              <div class="onboarding-paywall-card__actions">
                <Button
                  variant={props.activePlanId === plan.id ? 'primary' : 'secondary'}
                  size="lg"
                  fullWidth
                  onClick={() => props.onSelectPlan(plan.id)}
                >
                  {props.activePlanId === plan.id ? 'Valgt' : 'Velg plan'}
                </Button>
              </div>
            </article>
          )}
        </For>
      </div>

      <div class="onboarding-paywall__suggestion" aria-live="polite">
        <div class="onboarding-paywall__suggestion-main">
          <span class="onboarding-paywall__suggestion-kicker">{suggestionKicker()}</span>
          <strong>{suggestionTitle()}</strong>
          <p>{suggestionSummary()}</p>
          <Show when={props.recommendation?.reason}>
            {(reason) => <small>{reason()}</small>}
          </Show>
        </div>

        <div class="onboarding-paywall__suggestion-side">
          <div>
            <span>Valgt nå</span>
            <strong>{activePlan().name}</strong>
          </div>
          <Show when={hasRecommendation()}>
            <Show
              when={props.activePlanId !== recommendedPlanId()}
              fallback={<span class="onboarding-paywall__suggestion-confirmed">Forslaget er valgt</span>}
            >
              <Button
                variant="secondary"
                size="sm"
                disabled={props.loadingRecommendation}
                onClick={() => props.onSelectPlan(recommendedPlanId())}
              >
                Bruk anbefalingen
              </Button>
            </Show>
          </Show>
          <Show when={props.onRefreshRecommendation}>
            <Button
              variant="secondary"
              size="sm"
              disabled={props.loadingRecommendation}
              onClick={() => void props.onRefreshRecommendation?.()}
            >
              Oppdater forslag
            </Button>
          </Show>
        </div>

        <Show when={proofPoints().length || scopeSignals().length || opportunities().length}>
          <div class="onboarding-paywall__suggestion-evidence">
            <Show when={proofPoints().length}>
              <ul>
                <For each={proofPoints()}>{(item) => <li>{item}</li>}</For>
              </ul>
            </Show>
            <Show when={scopeSignals().length}>
              <div class="onboarding-paywall__suggestion-signals">
                <For each={scopeSignals()}>{(item) => <span>{item}</span>}</For>
              </div>
            </Show>
            <Show when={opportunities().length}>
              <div class="onboarding-paywall__suggestion-opportunities">
                <For each={opportunities()}>{(item) => <p>{item}</p>}</For>
              </div>
            </Show>
          </div>
        </Show>
      </div>

      <Show when={props.error}>
        {(message) => (
          <p class="onboarding-error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show
        when={
          props.checkoutSession?.provider === 'hyperswitch' && props.checkoutSession.client_secret
            ? props.checkoutSession
            : undefined
        }
        fallback={
          <div class="onboarding-paywall__actions">
            <Button variant="primary" size="sm" disabled={props.committing} onClick={() => void props.onCommitPlan()}>
              {props.committing
                ? 'Lagrer...'
                : props.activePlanId === 'trial'
                  ? 'Fortsett til oppsett'
                  : 'Fortsett til betaling'}
            </Button>
          </div>
        }
      >
        {(session) => (
          <HyperswitchCheckout
            session={session()}
            confirming={props.confirmingCheckout}
            returnUrl={props.checkoutReturnUrl}
            onConfirmed={props.onConfirmCheckout}
          />
        )}
      </Show>
    </section>
  )
}
