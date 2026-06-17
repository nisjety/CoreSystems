import { For, Show } from 'solid-js'
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
  onSelectPlan: (planId: PlanId) => void
  recommendation?: PlanRecommendation
}

export function PaywallStep(props: PaywallStepProps) {
  const recommendedPlanId = () => props.recommendation?.planId ?? 'trial'

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
                'onboarding-paywall-card--recommended': recommendedPlanId() === plan.id,
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
                  <Show when={recommendedPlanId() === plan.id}>
                    <span class="onboarding-paywall-card__badge">★ anbefalt</span>
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

      <div class="onboarding-paywall__reason">
        <strong>{recommendedPlanId() === 'trial' ? 'Gratis anbefales' : 'Velion anbefaler denne planen'}</strong>
        <p>
          {props.loadingRecommendation
            ? 'Analyserer onboarding-signalene.'
            : props.recommendation?.summary ||
              'Lite signal ennå. Gratis lar deg teste i 14 dager før Velion anbefaler en betalt plan.'}
        </p>
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
