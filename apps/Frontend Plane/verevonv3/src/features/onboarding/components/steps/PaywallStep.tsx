import { createMemo, For, Match, Show, Switch } from 'solid-js'
import type { BrandingSignals, CheckoutSession, PlanRecommendation } from '@/features/onboarding/lib/api'
import { type PlanId, onboardingPlanCards } from '@/features/onboarding/lib/model'
import { brandHost } from '@/features/onboarding/lib/view'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'
import { VerevonSwitch } from '@/shared/ui/verevon/VerevonSwitch'
import { HyperswitchCheckout } from '@/features/billing/components/HyperswitchCheckout'
import { NexiCheckout } from '@/features/billing/components/NexiCheckout'

/** Identity gathered earlier in onboarding (org step + website crawl) so the
 * plan step can address the customer by name and show what Verevon learned. */
type PaywallIdentity = {
  orgName?: string
  industry?: string
  orgNumber?: string
  websiteUrl?: string
  websitePages?: number
  connectorCount?: number
  connectedSourceCount?: number
  sourceCount?: number
  employeeCount?: number
  branding?: BrandingSignals
}

type PaywallStepProps = {
  activePlanId: PlanId
  checkoutReturnUrl: string
  checkoutSession?: CheckoutSession
  committing?: boolean
  confirmingCheckout?: boolean
  error?: string
  identity?: PaywallIdentity
  loadingRecommendation: boolean
  onConfirmCheckout: (payment: { paymentId?: string; clientSecret?: string; status: string }) => void | Promise<void>
  onCommitPlan: () => void | Promise<void>
  onRefreshRecommendation?: () => void | Promise<void>
  onSelectPlan: (planId: PlanId) => void
  recommendation?: PlanRecommendation
}

export function PaywallStep(props: PaywallStepProps) {
  const i18n = useI18n()
  const defaultPlan = onboardingPlanCards[0]!
  const hasRecommendation = () => Boolean(props.recommendation)
  const recommendedPlanId = () => props.recommendation?.planId ?? 'trial'
  const recommendedPlan = createMemo(() => (
    onboardingPlanCards.find((plan) => plan.id === recommendedPlanId()) ?? defaultPlan
  ))
  const activePlan = createMemo(() => (
    onboardingPlanCards.find((plan) => plan.id === props.activePlanId) ?? defaultPlan
  ))
  // Keep the paywall concise (it must not overflow the non-scrolling page):
  // only the 2 strongest proof points stay here; scope signals + opportunities
  // move to the assembly "workspace getting ready" step.
  const proofPoints = createMemo(() => props.recommendation?.proofPoints?.filter(Boolean).slice(0, 2) ?? [])
  const suggestionSummary = () => {
    if (props.loadingRecommendation) {
      return i18n.tr(
        'Verevon AI analyserer nettsted, organisasjon og kildene du har valgt.',
        'Verevon AI is analyzing your website, organization, and selected sources.',
      )
    }

    const summary = props.recommendation?.summary
    if (!summary) {
      return i18n.tr(
        'Lite signal ennå. Gratis lar deg teste i 14 dager før Verevon anbefaler en betalt plan.',
        'There is still limited signal. Free lets you test for 14 days before Verevon recommends a paid plan.',
      )
    }

    return buildStructuredRecommendationSummary(props.recommendation?.planId, props.identity, i18n)
      ?? alignRecommendationCopy(summary, props.recommendation?.planId, i18n)
  }
  const suggestionTitle = () => {
    if (props.loadingRecommendation) return i18n.tr('Verevon AI finner beste plan', 'Verevon AI is finding the best plan')
    return i18n.tr(`${planName(recommendedPlan().id, i18n)} anbefales`, `${planName(recommendedPlan().id, i18n)} recommended`)
  }
  const suggestionKicker = () => {
    if (props.loadingRecommendation) return i18n.tr('Analyserer', 'Analyzing')
    return props.recommendation?.source === 'model'
      ? i18n.tr('Verevon AI-forslag', 'Verevon AI suggestion')
      : i18n.tr('Verevon forslag', 'Verevon suggestion')
  }

  // Personalization: name + logo + what Verevon learned from the crawl. Derived
  // from the identity gathered in the website + organization steps.
  const brandLogo = () => props.identity?.branding?.favicon || props.identity?.branding?.logoCandidate
  const brandName = () =>
    props.identity?.orgName?.trim() ||
    props.identity?.branding?.siteName?.trim() ||
    brandHost(props.identity?.websiteUrl) ||
    ''
  const brandInitial = () => (brandName() ? brandName()!.charAt(0).toUpperCase() : 'V')
  const hasIdentity = () => Boolean(brandName())
  const learnedChips = createMemo(() => {
    const id = props.identity
    if (!id) return [] as string[]
    const chips: string[] = []
    const host = brandHost(id.websiteUrl)
    if ((id.websitePages ?? 0) > 0 && host) chips.push(`${id.websitePages} ${i18n.tr('sider fra', 'pages from')} ${host}`)
    else if (host) chips.push(host)
    if (id.industry) chips.push(id.industry)
    if (id.orgNumber) chips.push(`${i18n.tr('Org.nr', 'Org no.')} ${id.orgNumber}`)
    const connectedSourceCount = id.connectedSourceCount ?? id.connectorCount ?? 0
    if (connectedSourceCount > 0) chips.push(`${connectedSourceCount} ${i18n.tr('tilkoblede kilder', 'connected sources')}`)
    return chips.slice(0, 4)
  })

  return (
    <section class="onboarding-paywall">
      <Show when={hasIdentity()}>
        <div class="onboarding-paywall__identity" aria-label={i18n.tr('Tilpasset for din organisasjon', 'Personalized for your organization')}>
          <div class="onboarding-paywall__identity-badge" style={props.identity?.branding?.themeColor ? { background: props.identity.branding.themeColor } : undefined}>
            <Show when={brandLogo()} fallback={<span>{brandInitial()}</span>}>
              <img src={brandLogo()} alt="" />
            </Show>
          </div>
          <div class="onboarding-paywall__identity-body">
            <span class="onboarding-paywall__identity-kicker">{i18n.tr('Tilpasset for', 'Personalized for')}</span>
            <strong>{brandName()}</strong>
            <Show when={learnedChips().length}>
              <div class="onboarding-paywall__identity-chips">
                <For each={learnedChips()}>{(chip) => <span>{chip}</span>}</For>
              </div>
            </Show>
          </div>
        </div>
      </Show>

      <div class="onboarding-paywall__header">
        <div>
          <h1>{hasIdentity() ? i18n.tr(`Beste match for ${brandName()}`, `Best match for ${brandName()}`) : i18n.tr('Din beste match', 'Your best match')}</h1>
          <p>{i18n.tr('Velg planen som passer behovet ditt', 'Choose the plan that fits your needs')}</p>
        </div>
        <div class="onboarding-paywall__billing">
          <span>{i18n.tr('Månedlig', 'Monthly')}</span>
          <VerevonSwitch label={i18n.tr('Faktureringsperiode', 'Billing period')} />
          <span>{i18n.tr('Årlig', 'Annual')}</span>
          <span class="onboarding-paywall__trial-pill">{i18n.tr('14 dager gratis', '14 days free')}</span>
        </div>
      </div>

      <div class="onboarding-paywall__grid">
        <For each={onboardingPlanCards}>
          {(plan) => (
            (() => {
              const copy = () => planCopy(plan.id, i18n)
              return (
            <article
              class="onboarding-paywall-card"
              classList={{
                'onboarding-paywall-card--active': props.activePlanId === plan.id,
                'onboarding-paywall-card--recommended': hasRecommendation() && recommendedPlanId() === plan.id,
              }}
            >
              <div class="onboarding-paywall-card__head">
                <strong>{copy().name}</strong>
                <span class="onboarding-paywall-card__badges">
                  <Show when={plan.id === 'trial'}>
                    <span class="onboarding-paywall-card__badge onboarding-paywall-card__badge--trial">
                      {i18n.tr('Aktiv prøve', 'Active trial')}
                    </span>
                  </Show>
                  <Show when={hasRecommendation() && recommendedPlanId() === plan.id}>
                    <span class="onboarding-paywall-card__badge">{i18n.tr('Verevon foreslår', 'Verevon suggests')}</span>
                  </Show>
                  <Show when={props.activePlanId === plan.id && recommendedPlanId() !== plan.id}>
                    <span class="onboarding-paywall-card__badge onboarding-paywall-card__badge--choice">
                      {i18n.tr('Ditt valg', 'Your choice')}
                    </span>
                  </Show>
                </span>
              </div>
              <div class="onboarding-paywall-card__price">
                <span>{plan.price === 'Custom' ? i18n.tr('Tilpasset', 'Custom') : `${plan.price} kr`}</span>
                <Show when={plan.price !== 'Custom'}>
                  <small>{i18n.tr('/mnd', '/mo')}</small>
                </Show>
              </div>
              <p>{copy().description}</p>
              <ul>
                <For each={copy().features}>{(item) => <li>{item}</li>}</For>
              </ul>
              <div class="onboarding-paywall-card__actions">
                <Button
                  variant={props.activePlanId === plan.id ? 'primary' : 'secondary'}
                  size="lg"
                  fullWidth
                  onClick={() => props.onSelectPlan(plan.id)}
                >
                  {props.activePlanId === plan.id ? i18n.tr('Valgt', 'Selected') : i18n.tr('Velg plan', 'Choose plan')}
                </Button>
              </div>
            </article>
              )
            })()
          )}
        </For>
      </div>

      <div class="onboarding-paywall__suggestion" aria-live="polite">
        <div class="onboarding-paywall__suggestion-main">
          <span class="onboarding-paywall__suggestion-kicker">{suggestionKicker()}</span>
          <strong>{suggestionTitle()}</strong>
          <p>{suggestionSummary()}</p>
          <Show when={props.recommendation?.reason}>
            {(reason) => <small>{recommendationReason(reason(), props.recommendation?.planId, i18n)}</small>}
          </Show>
        </div>

        <div class="onboarding-paywall__suggestion-side">
          <div>
            <span>{i18n.tr('Valgt nå', 'Selected now')}</span>
            <strong>{planName(activePlan().id, i18n)}</strong>
          </div>
          <Show when={hasRecommendation()}>
            <Show
              when={props.activePlanId !== recommendedPlanId()}
              fallback={<span class="onboarding-paywall__suggestion-confirmed">{i18n.tr('Forslaget er valgt', 'Suggestion selected')}</span>}
            >
              <Button
                variant="secondary"
                size="sm"
                disabled={props.loadingRecommendation}
                onClick={() => props.onSelectPlan(recommendedPlanId())}
              >
                {i18n.tr('Bruk anbefalingen', 'Use recommendation')}
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
              {i18n.tr('Oppdater forslag', 'Refresh suggestion')}
            </Button>
          </Show>
        </div>

        <Show when={proofPoints().length}>
          <div class="onboarding-paywall__suggestion-evidence">
            <ul>
              <For each={proofPoints()}>{(item) => <li>{item}</li>}</For>
            </ul>
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

      <Switch
        fallback={
          <div class="onboarding-paywall__actions">
            <Button variant="primary" size="sm" disabled={props.committing} onClick={() => void props.onCommitPlan()}>
              {props.committing
                ? i18n.tr('Lagrer...', 'Saving...')
                : props.activePlanId === 'trial'
                  ? i18n.tr('Fortsett til oppsett', 'Continue to setup')
                  : i18n.tr('Fortsett til betaling', 'Continue to payment')}
            </Button>
          </div>
        }
      >
        <Match
          when={
            props.checkoutSession?.provider === 'nexi' &&
            (props.checkoutSession.payment_id || props.checkoutSession.id) &&
            props.checkoutSession.publishable_key &&
            props.checkoutSession.client_url
              ? props.checkoutSession
              : undefined
          }
        >
          {(session) => (
            <NexiCheckout
              session={session()}
              confirming={props.confirmingCheckout}
              returnUrl={props.checkoutReturnUrl}
              onConfirmed={props.onConfirmCheckout}
            />
          )}
        </Match>
        <Match
          when={
            props.checkoutSession?.provider === 'hyperswitch' && props.checkoutSession.client_secret
              ? props.checkoutSession
              : undefined
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
        </Match>
      </Switch>
    </section>
  )
}

type PaywallI18n = ReturnType<typeof useI18n>

function planName(planId: PlanId, i18n: PaywallI18n): string {
  return planCopy(planId, i18n).name
}

function buildStructuredRecommendationSummary(
  planId: PlanId | undefined,
  identity: PaywallIdentity | undefined,
  i18n: PaywallI18n,
): string | undefined {
  const totalSourceCount = identity?.sourceCount
  const connectedSourceCount = identity?.connectedSourceCount
  const employeeCount = identity?.employeeCount
  if (
    !planId ||
    typeof totalSourceCount !== 'number' ||
    typeof connectedSourceCount !== 'number'
  ) {
    return undefined
  }

  const sourceText = totalSourceCount > connectedSourceCount
    ? totalSourceCount === 1
      ? i18n.tr(
          `${totalSourceCount} kilde totalt (${connectedSourceCount} ${connectedSourceCount === 1 ? 'tilkoblet' : 'tilkoblede'})`,
          `${totalSourceCount} source total (${connectedSourceCount} connected)`,
        )
      : i18n.tr(
          `${totalSourceCount} kilder totalt (${connectedSourceCount} ${connectedSourceCount === 1 ? 'tilkoblet' : 'tilkoblede'})`,
          `${totalSourceCount} total sources (${connectedSourceCount} connected)`,
        )
    : i18n.tr(
        `${totalSourceCount} ${totalSourceCount === 1 ? 'kilde' : 'kilder'}`,
        `${totalSourceCount} ${totalSourceCount === 1 ? 'source' : 'sources'}`,
      )

  const employeeText = typeof employeeCount === 'number' && employeeCount > 0
    ? {
        nb: ` og ${employeeCount} ${employeeCount === 1 ? 'ansatt' : 'ansatte'}`,
        en: ` and ${employeeCount} ${employeeCount === 1 ? 'employee' : 'employees'}`,
      }
    : { nb: '', en: '' }
  const hasEmployeeSubject = employeeText.en.length > 0

  return i18n.tr(
    `${sourceText}${employeeText.nb} gir best start med ${planName(planId, i18n)}.`,
    `${sourceText}${employeeText.en} ${totalSourceCount === 1 && !hasEmployeeSubject ? 'makes' : 'make'} ${planName(planId, i18n)} the best starting point.`,
  )
}

function recommendationReason(value: string, planId: PlanId | undefined, i18n: PaywallI18n): string {
  if (planId === 'enterprise') {
    return i18n.tr(
      'Kompleksitet, volum eller governance-signaler peker mot Custom.',
      'Complexity, volume, or governance signals point to Custom.',
    )
  }
  return value
}

function alignRecommendationCopy(
  value: string,
  planId: PlanId | undefined,
  i18n: PaywallI18n,
): string {
  if (planId !== 'enterprise') return value
  return value.replace(/\bEnterprise\b/gi, planName(planId, i18n))
}

function planCopy(planId: PlanId, i18n: PaywallI18n): { name: string; description: string; features: string[] } {
  switch (planId) {
    case 'trial':
      return {
        name: i18n.tr('Gratis', 'Free'),
        description: i18n.tr(
          'Prøv Verevon og agenten i 14 dager før du velger betalt plan.',
          'Try Verevon and the agent for 14 days before choosing a paid plan.',
        ),
        features: [
          i18n.tr('Ingen kort kreves', 'No card required'),
          i18n.tr('14 dagers prøveperiode', '14-day trial'),
          i18n.tr('Oppgrader når du er klar', 'Upgrade when ready'),
        ],
      }
    case 'hobby':
      return {
        name: 'Essential',
        description: i18n.tr(
          'For små team som vil validere en enkel chatbot.',
          'For small teams validating a simple chatbot.',
        ),
        features: [
          i18n.tr('Bruksbasert prising for AI-løste henvendelser', 'Usage-based AI resolution pricing'),
          i18n.tr('Chatbot + delt innboks', 'Chatbot + shared inbox'),
          i18n.tr('Nettside og kunnskapskilder', 'Website and knowledge sources'),
        ],
      }
    case 'standard':
      return {
        name: 'Advanced',
        description: i18n.tr(
          'For team som trenger automasjon, ruting og flere kilder.',
          'For teams that need automation, routing, and multiple sources.',
        ),
        features: [
          i18n.tr('Bruksbasert prising for AI-løste henvendelser', 'Usage-based AI resolution pricing'),
          i18n.tr('Automasjon og ruting', 'Automation and routing'),
          i18n.tr('Flere team-innbokser', 'Multiple team inboxes'),
          i18n.tr('20 Lite-seter inkludert', '20 Lite seats included'),
        ],
      }
    case 'pro':
      return {
        name: 'Expert',
        description: i18n.tr(
          'For større supportteam med rapportering og styring.',
          'For larger support teams with reporting and controls.',
        ),
        features: [
          i18n.tr('Bruksbasert prising for AI-løste henvendelser', 'Usage-based AI resolution pricing'),
          i18n.tr('SSO og identitetsstyring', 'SSO and identity controls'),
          i18n.tr('SLA, rapportering og multibrand', 'SLA, reporting, and multibrand'),
          i18n.tr('50 Lite-seter inkludert', '50 Lite seats included'),
        ],
      }
    case 'enterprise':
      return {
        name: 'Custom',
        description: i18n.tr(
          'Kontakt salg for volum, onboarding og governance.',
          'Contact sales for volume, onboarding, and governance.',
        ),
        features: [
          i18n.tr('Volumpris per AI-svar', 'Volume pricing per AI answer'),
          i18n.tr('Tilpassede vilkår', 'Custom terms'),
          i18n.tr('Utvidet onboarding', 'Extended onboarding'),
          i18n.tr('Dedikert success-team', 'Dedicated success team'),
        ],
      }
  }
}
