import { useNavigate } from '@solidjs/router'
import { useQueryClient } from '@tanstack/solid-query'
import { Show, createEffect, createMemo, createSignal, flush, onCleanup, untrack } from 'solid-js'
import { createOnboardingGatewayActions } from '@/features/onboarding/lib/actions'
import {
  type BrregEnhet,
  type CheckoutSession,
  getBrowserActor,
  getShippingCarriers,
  saveOnboardingState,
} from '@/features/onboarding/lib/api'
import { resolveCheckoutSurface } from '@/features/billing/lib/checkout'
import { isCheckoutActivatingStatus } from '@/features/billing/lib/plans'
import { signOut } from '@/shared/api/auth-client'
import { clearSession, getSession, loadSession, markSessionOnboardingComplete } from '@/shared/session/session-store'
import { createCrawlPreviewStream } from '@/features/onboarding/lib/crawl-preview'
import {
  type ConnectorOption,
  type OnboardingState,
  type PlanId,
  type Step,
  onboardingPlanCards,
  onboardingSteps,
} from '@/features/onboarding/lib/model'
import { createOnboardingState } from '@/features/onboarding/lib/onboarding-state'
import { createOnboardingPersistence } from '@/features/onboarding/lib/persistence'
import { runDirectOauthWindow } from '@/features/onboarding/lib/provider-auth-window'
import {
  createGraphPreviewQuery,
  createPlanRecommendationQuery,
  onboardingQueryKeys,
} from '@/features/onboarding/lib/queries'
import {
  cloneOnboardingState,
  createInitialOnboardingState,
  reconcileOnboardingState,
} from '@/features/onboarding/lib/state'
import { createOnboardingStepTransition } from '@/features/onboarding/lib/step-transition'
import {
  approxEmployeesFromSize,
  inferOrganizationQuery,
  rankBrregSuggestions,
  sizeFromEmployees,
  sizeLabel,
  stepNumberFor,
} from '@/features/onboarding/lib/view'
import { summarizeOnboardingSources } from '@/features/onboarding/lib/source-summary'
import {
  hasRecommendationLocale,
  localizeRecommendation,
  planRecommendationContextHash,
  recommendationLocale,
  recommendationText,
  withRecommendationTranslation,
} from '@/features/onboarding/lib/plan-recommendation'
import { translateApiError, useI18n } from '@/shared/i18n'
import { ContactSalesModal } from '@/features/onboarding/components/ContactSalesModal'
import { AssemblyStepContent, AssemblyStepVisual } from '@/features/onboarding/components/steps/AssemblyStep'
import { ConnectStepContent, ConnectStepVisual } from '@/features/onboarding/components/steps/ConnectStep'
import { IntroStepContent, IntroStepVisual } from '@/features/onboarding/components/steps/IntroStep'
import { OrganizationStepContent, OrganizationStepVisual } from '@/features/onboarding/components/steps/OrganizationStep'
import { PaywallStep } from '@/features/onboarding/components/steps/PaywallStep'
import { SocialProofStepContent, SocialProofStepVisual } from '@/features/onboarding/components/steps/SocialProofStep'
import { WebsiteStepContent, WebsiteStepVisual } from '@/features/onboarding/components/steps/WebsiteStep'
import { OnboardingBrandStrip } from '@/features/onboarding/components/shared/OnboardingBrandStrip'
import { OnboardingFrame } from '@/features/onboarding/components/shared/OnboardingFrame'
import { OnboardingScreen } from '@/features/onboarding/components/shared/OnboardingScreen'
import { createElementHeight } from '@/shared/ui/verevon/createElementHeight'

const onboardingCardBaseHeight = 1140
const storageKey = 'verevonv3.onboarding.state.v1'

export default function OnboardingPage() {
  // Identity comes from the validated session (RequireOnboarding guarantees an
  // authenticated user before this mounts). The dev actor fallback only keeps
  // local gateway actions usable; the graph itself remains session-gated.
  const sessionUser = getSession().user
  const actor = sessionUser
    ? { userId: sessionUser.id, userEmail: sessionUser.email, userName: sessionUser.name }
    : getBrowserActor()
  const navigate = useNavigate()
  const actions = createOnboardingGatewayActions(actor)
  const i18n = useI18n()
  const queryClient = useQueryClient()
  const [state, setState] = createOnboardingState(storageKey)
  const [searchResults, setSearchResults] = createSignal<BrregEnhet[]>([])
  const [searching, setSearching] = createSignal(false)
  const [orgAutoInferred, setOrgAutoInferred] = createSignal(false)
  const [submittingOrg, setSubmittingOrg] = createSignal(false)
  const [connectingId, setConnectingId] = createSignal<string>()
  const [committingPlan, setCommittingPlan] = createSignal(false)
  const [contactSalesOpen, setContactSalesOpen] = createSignal(false)
  // Armed on connect-step hover (only when a source is connected) to start the
  // plan recommendation early, so it's ready by the time the user reaches the
  // paywall. No-integration users still trigger it on entering the paywall.
  const [recommendationPrefetch, setRecommendationPrefetch] = createSignal(false)
  const [confirmingCheckout, setConfirmingCheckout] = createSignal(false)
  const [checkoutSession, setCheckoutSession] = createSignal<CheckoutSession>()
  const [translatingRecommendationKey, setTranslatingRecommendationKey] = createSignal<string>()
  const [assemblyTicks, setAssemblyTicks] = createSignal(0)
  const [assemblyError, setAssemblyError] = createSignal<string>()
  const [error, setError] = createSignal<string>()
  const [hydratedFromServer, setHydratedFromServer] = createSignal(false)
  const [finalizingOnboarding, setFinalizingOnboarding] = createSignal(false)
  const [viewportHeight, setViewportHeight] = createSignal<number | null>(
    typeof window === 'undefined' ? null : window.innerHeight,
  )
  const crawlPreview = createCrawlPreviewStream()
  const leftPaneSize = createElementHeight<HTMLDivElement>()
  const currentStep = () => state.step
  const { displayedStep, stepTransitionPhase } = createOnboardingStepTransition(currentStep)
  let introTimer: number | undefined
  let assemblyTimer: number | undefined
  let completionTimer: number | undefined

  createOnboardingPersistence({
    actor: actions.actor,
    hydratedFromServer,
    paused: finalizingOnboarding,
    state,
    storageKey,
  })

  const graphQuery = createGraphPreviewQuery(
    actions,
    () => state.organization.id,
    () => state.step === 'connect' && Boolean(state.organization.id),
  )

  // Solid v2 has no onMount; a two-phase createEffect with a constant
  // compute runs its effect function exactly once after mount.
  createEffect(
    () => undefined,
    () => {
      const updateViewportHeight = () => setViewportHeight(window.innerHeight)
      updateViewportHeight()
      window.addEventListener('resize', updateViewportHeight)

      const checkoutParams = new URLSearchParams(window.location.search)
      const checkoutState = checkoutParams.get('checkout')
      void actions.loadOnboardingState<OnboardingState>()
        .then((snapshot) => {
        if (snapshot?.state) {
          setState(() => reconcileOnboardingState(snapshot))
        }

        if (checkoutState === 'success') {
          const paymentId = checkoutParams.get('payment_id') || undefined
          const clientSecret = checkoutParams.get('payment_intent_client_secret') || undefined
          const providerStatus = checkoutParams.get('status') || 'processing'
          const planFromSnapshot = snapshot?.state?.plan || state.plan || recommendedPlan()

          if (paymentId || clientSecret) {
            void finalizePaidCheckout({
              paymentId,
              clientSecret,
              status: providerStatus,
              plan: planFromSnapshot,
            })
          } else {
            setState((s) => { s.step = 'paywall' })
            setError('Betalingen mangler referanse. Start betalingen på nytt.')
          }
        }

        if (checkoutState === 'cancel') {
          setState((s) => { s.step = 'paywall' })
          setError('Betaling avbrutt.')
        }

        setHydratedFromServer(true)
      })
        .catch(() => setHydratedFromServer(true))

      // An effect function runs with no owner, so onCleanup() inside it would
      // silently never fire; the returned function is the real teardown.
      return () => window.removeEventListener('resize', updateViewportHeight)
    },
  )

  createEffect(
    () => ({ step: state.step, introPlayed: state.introPlayed }),
    ({ step, introPlayed }) => {
      if (typeof window === 'undefined') return
      window.clearTimeout(introTimer)
      introTimer = undefined

      if (step !== 'post-signin') return
      introTimer = window.setTimeout(advanceFromIntro, introPlayed ? 600 : 3000)
    },
  )

  // On reaching the organization step, infer the org from the website crawl and
  // pre-search Enhetsregisteret once, so verified matches appear without the
  // user re-typing what the crawl already discovered.
  createEffect(
    () => ({ step: state.step, website: state.website }),
    // The effect function is untracked, so the peeks below stay one-shot reads
    // exactly as the untrack() wrappers made them under the single-arg form.
    ({ step, website }) => untrack(() => {
      if (step !== 'organization') return
      if (orgAutoInferred()) return
      if (Boolean(state.organization.orgNumber || state.organization.id)) return
      if (searchResults().length > 0) return

      const inferred = inferOrganizationQuery(website)
      if (!inferred) return

      setOrgAutoInferred(true)
      if (!state.organization.name.trim()) {
        setState((s) => { s.organization.name = inferred })
      }
      void autoInferOrganizationFromWebsite(inferred)
    }),
  )

  onCleanup(() => {
    if (typeof window === 'undefined') return
    window.clearTimeout(introTimer)
    window.clearInterval(assemblyTimer)
    window.clearTimeout(completionTimer)
  })

  const currentStepIndex = createMemo(() => Math.max(0, onboardingSteps.indexOf(state.step)))
  const visibleStepNumber = createMemo(() => stepNumberFor(currentStep()))
  const cardScale = createMemo(() => {
    const height = viewportHeight()
    const fittedScale = height ? Math.min(1, Math.max(0.52, (height - 18) / onboardingCardBaseHeight)) : 1
    return Math.min(1.04, fittedScale * 1.04)
  })
  const sourceSummary = createMemo(() => summarizeOnboardingSources({
    connectors: state.connectors,
    websiteUrl: state.website.url,
  }))
  const currentRecommendationLocale = createMemo(() => recommendationLocale(i18n.locale()))
  const recommendationContext = createMemo<Record<string, unknown>>(() => {
    const graph = graphQuery.data
    const sources = sourceSummary()
    return {
      organization: {
        name: state.organization.name,
        size: state.organization.size,
        // Fall back to a size-band midpoint when the org wasn't Brreg-verified,
        // so the AI (and the local heuristic's numeric tiering) still gets a
        // head-count signal instead of nothing.
        employeeCount: state.organization.employeeCount ?? approxEmployeesFromSize(state.organization.size),
      },
      website: {
        url: state.website.url,
        agentBrief: state.website.brief,
      },
      websites: state.website.url ? [{ url: state.website.url, agentBrief: state.website.brief }] : [],
      connectors: sources.sourceDetails.map((item) => ({
        id: item.connectorId,
        label: item.connectorLabel,
        sourceCount: item.sourceCount,
        sources: item.sources,
      })),
      locale: currentRecommendationLocale(),
      connectorCount: sources.connectorCount,
      connectedSourceCount: sources.connectedSourceCount,
      connectedSources: sources.sourceDetails,
      sourceCount: sources.totalSourceCount,
      sourceSummary: sources,
      websiteSourceCount: sources.websiteSourceCount,
      // --- Rich personalization signals (top-level so the gateway's
      // flatten-passthrough carries them through to the Model Plane prompt,
      // which explicitly reads goal/industry/connected-systems and
      // `context.dataPlane` graph evidence). ---
      orgNumber: state.organization.orgNumber,
      industry: state.organization.industry,
      orgForm: state.organization.orgForm,
      // Honest head-count band (e.g. "51-250") from the size chip, so the model
      // can frame team size without quoting the synthetic exact count above.
      employeeBand: state.organization.size ? sizeLabel(state.organization.size) : undefined,
      goal: state.website.brief,
      branding: state.website.branding,
      websitePages: state.website.pages,
      // Actual text the crawl pulled off the site (title + excerpt per page),
      // capped and trimmed. This is the strongest personalization signal: it
      // lets the AI describe what the company does in its own words instead of
      // guessing from the domain. Flattened through the gateway to the Model
      // Plane prompt (which is instructed to ground on it, not invent content).
      websiteContent: state.website.snippets
        .filter((s) => s.title || s.excerpt)
        .slice(0, 8)
        .map((s) => ({
          title: s.title?.slice(0, 120),
          excerpt: s.excerpt?.slice(0, 280),
          url: s.url,
        })),
      ...(graph
        ? {
            dataPlane: {
              nodeCount: graph.counts?.nodes ?? graph.nodes.length,
              edgeCount: graph.counts?.edges ?? graph.edges.length,
              groupCount: graph.counts?.groups,
              sampleNodes: graph.nodes.slice(0, 12).map((node) => node.label),
            },
          }
        : {}),
    }
  })
  const recommendationContextHash = createMemo(() => planRecommendationContextHash(recommendationContext()))
  const activeRecommendation = createMemo(() => {
    const recommendation = state.recommendation
    if (!recommendation) return undefined
    return recommendation.contextHash === recommendationContextHash() ? recommendation : undefined
  })
  const localizedRecommendation = createMemo(() =>
    localizeRecommendation(activeRecommendation(), currentRecommendationLocale()))
  const recommendedPlan = createMemo(() => activeRecommendation()?.planId ?? 'trial')
  const activePlan = createMemo(() => state.plan ?? recommendedPlan())
  const recommendationQuery = createPlanRecommendationQuery(
    actions,
    () => recommendationContext(),
    () => (state.step === 'paywall' || recommendationPrefetch()) && !activeRecommendation(),
    () => recommendationContextHash(),
  )
  const checkoutReturnUrl = createMemo(() =>
    typeof window === 'undefined'
      ? '/onboarding?checkout=success'
      : `${window.location.origin}/onboarding?checkout=success`,
  )

  createEffect(
    () => {
      const recommendation = recommendationQuery.data
      if (!recommendation) return undefined
      return {
        recommendation,
        sources: sourceSummary(),
        contextHash: recommendationContextHash(),
      }
    },
    (computed) => untrack(() => {
      if (!computed) return
      const { recommendation, sources, contextHash } = computed
      const locale = currentRecommendationLocale()
      setState((s) => { s.recommendation = {
        ...recommendation,
        connectedSourceCount: sources.connectedSourceCount,
        contextHash,
        locale,
        sourceCount: sources.totalSourceCount,
      } })
      if (!state.plan) setState((s) => { s.plan = recommendation.planId })
    }),
  )

  createEffect(
    () => ({
      recommendation: activeRecommendation(),
      targetLanguage: currentRecommendationLocale(),
    }),
    ({ recommendation, targetLanguage }) => {
      if (!recommendation || hasRecommendationLocale(recommendation, targetLanguage)) return

      const key = `${recommendation.contextHash ?? 'current'}:${recommendation.generatedAt}:${targetLanguage}`
      if (untrack(() => translatingRecommendationKey()) === key) return
      setTranslatingRecommendationKey(key)

      void actions
        .translatePlanRecommendation({
          recommendation: recommendationText(recommendation),
          sourceLanguage: recommendation.locale,
          targetLanguage,
        })
        .then((translation) => {
          setState((s) => { s.recommendation = ((current) => {
            if (!current || current.contextHash !== recommendation.contextHash) return current
            return withRecommendationTranslation(current, targetLanguage, translation)
          })(s.recommendation) })
        })
        .catch(() => undefined)
    },
  )

  createEffect(
    () => ({
      recommendation: state.recommendation,
      contextHash: recommendationContextHash(),
    }),
    ({ recommendation, contextHash }) => {
      if (!recommendation || recommendation.contextHash === contextHash) return

      flush(() => {
        if (untrack(() => state.plan) === recommendation.planId) setState((s) => { s.plan = undefined })
        setState((s) => { s.recommendation = undefined })
      })
    },
  )

  createEffect(
    () => ({ reason: recommendationQuery.error, step: state.step }),
    ({ reason, step }) => {
      if (step !== 'paywall' || !reason) return
      setError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke beregne en anbefaling.', en: 'Could not compute a recommendation.' }))
    },
  )

  function advanceFromIntro() {
    setState((s) => { s.introPlayed = true })
    setState((s) => { s.step = 'website' })
  }

  function clearSearchResults() {
    setSearchResults([])
  }

  function skipWebsite() {
    setState((s) => { s.websiteSkipped = true })
    setState((s) => { s.step = 'organization' })
  }

  function selectOrganizationResult(item: BrregEnhet) {
    setState((s) => { s.organization.name = item.navn })
    setState((s) => { s.organization.orgNumber = item.organisasjonsnummer })
    setState((s) => { s.organization.employeeCount = item.antallAnsatte })
    setState((s) => { s.organization.size = sizeFromEmployees(item.antallAnsatte) })
    // Capture industry + org form from the Brreg entry — strong, specific
    // signals the AI plan recommender uses to personalize its reasoning.
    setState((s) => { s.organization.industry = item.naeringskode1?.beskrivelse })
    setState((s) => { s.organization.orgForm = item.organisasjonsform?.beskrivelse })

    // Pre-fill the website from the Brreg registry entry so a downstream ingest
    // has a URL to crawl — but never clobber a site the user already entered or
    // crawled in the website step. The website step stores the URL in
    // `https://`-prefixed form, so normalize to match.
    const site = (item.hjemmeside ?? '').trim().replace(/^https?:\/\//i, '').replace(/\s+/g, '')
    if (site && !state.website.url.trim()) setState((s) => { s.website.url = `https://${site}` })
  }

  async function runWebsitePreview() {
    const url = state.website.url.trim()
    if (!url || crawlPreview.previewing()) return
    const brief = state.website.brief
    const orgId = state.organization.id

    setError(undefined)
    setState((s) => { s.website.snippets = [] })
    setState((s) => { s.website.pages = 0 })
    setState((s) => { s.website.elements = 0 })
    setState((s) => { s.website.status = 'starting' })
    setState((s) => { s.website.warning = undefined })

    await crawlPreview.start(
      {
        url,
        brief,
        maxPages: 6,
        orgId,
      },
      {
        onStarted: (payload) => {
          setState((s) => { s.website.crawlJobId = payload.jobId })
        },
        onSnippet: (payload) => {
          setState((s) => { s.website.snippets = ((current) => [...current, payload].slice(-12))(s.website.snippets) })
        },
        onProgress: (payload) => {
          flush(() => {
            setState((s) => { s.website.status = payload.status })
            setState((s) => { s.website.pages = payload.pages })
            setState((s) => { s.website.elements = payload.elements })
          })
        },
        onBranding: (payload) => {
          setState((s) => { s.website.branding = payload })
        },
        onWarning: (payload) => {
          setState((s) => { s.website.warning = payload.message || payload.code })
        },
        onDone: (payload) => {
          flush(() => {
            setState((s) => { s.website.status = payload.status === 'failed' ? 'failed' : 'completed' })
            setState((s) => { s.website.pages = payload.pages || state.website.pages })
            setState((s) => { s.website.elements = payload.elements || state.website.elements })
          })
        },
      },
    ).catch((reason: unknown) => {
      setState((s) => { s.website.status = 'failed' })
      setError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke forhåndsvise nettsiden.', en: 'Could not preview the website.' }))
    })
  }

  async function runBrregSearch() {
    if (!state.organization.name.trim()) return

    setSearching(true)
    setError(undefined)
    try {
      setSearchResults(await actions.searchBrreg(state.organization.name))
    } catch (reason) {
      setError(translateApiError(reason, i18n.tr, { no: 'Søket i Brreg feilet.', en: 'Could not search Brreg.' }))
    } finally {
      setSearching(false)
    }
  }

  // Auto-fill the organization step from the website crawl findings: derive a
  // likely org name from the detected brand/site (or the URL host), seed the
  // name field, and pre-search Enhetsregisteret so verified matches surface
  // without the user re-typing. Best-effort — manual search still works on
  // failure. Mirrors verevon v2's inferred-from-website suggestion flow.
  async function autoInferOrganizationFromWebsite(query: string) {
    setSearching(true)
    try {
      const results = await actions.searchBrreg(query)
      const ranked = rankBrregSuggestions(results, query)
      setSearchResults(ranked.length > 0 ? ranked : results)
    } catch {
      // Silent: the user did not explicitly trigger this, and the manual search
      // box remains available.
    } finally {
      setSearching(false)
    }
  }

  async function submitOrganization() {
    if (!state.organization.name.trim()) return

    setSubmittingOrg(true)
    setError(undefined)

    try {
      const websiteUrl = state.website.url.trim()
      const created = await actions.createOrganization({
        name: state.organization.name,
        plan: 'trial',
        orgNumber: state.organization.orgNumber,
        metadata: {
          // Persist the org's interactive Zero-Data-Retention intent at
          // creation. zdr=true is the privacy-preserving default; live
          // enforcement is applied through auth-core's managed retention policy.
          interactiveRetention: { zdr: state.organization.zeroDataRetention },
          ...(state.website.branding
            ? {
                onboarding_branding: {
                  site_name: state.website.branding.siteName,
                  theme_color: state.website.branding.themeColor,
                  favicon: state.website.branding.favicon,
                  logo_candidate: state.website.branding.logoCandidate,
                  palette: state.website.branding.palette?.slice(0, 8),
                },
              }
            : {}),
        },
      })

      setState((s) => { s.organization.id = created.id })
      setState((s) => { s.organization.name = created.name || state.organization.name })

      if (websiteUrl) {
        await actions.startWebsiteIngest({
          orgId: created.id,
          url: websiteUrl,
          brief: state.website.brief,
        }).catch(() => undefined)
      }

      setState((s) => { s.step = 'connect' })
    } catch (reason) {
      setError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke opprette organisasjonen.', en: 'Could not create the organization.' }))
    } finally {
      setSubmittingOrg(false)
    }
  }

  async function connectSource(option: Pick<ConnectorOption, 'id' | 'label' | 'provider' | 'sources'>) {
    if (!state.organization.id) {
      setError(i18n.tr('Opprett organisasjonen før du kobler til kilder.', 'Create the organization before connecting sources.'))
      return
    }

    const orgId = state.organization.id
    setConnectingId(option.id)
    setError(undefined)

    try {
      // The shipping aggregator is Verevon's own carrier fleet (shipping-core)
      // — no per-user OAuth. "Connecting" it verifies the aggregator is live
      // and shows which carriers it can compare.
      if (option.provider === 'shipping') {
        const carriers = await getShippingCarriers()
        if (carriers.length === 0) {
          throw new Error('Fraktaggregatoren svarte uten transportører. Sjekk shipping-core.')
        }
        setState((s) => { s.connectors = ((current) => [
          ...current.filter((item) => item.id !== option.id),
          {
            id: option.id,
            label: `${option.label} (${carriers.length} transportører)`,
            status: 'connected',
            sources: option.sources,
          },
        ])(s.connectors) })
        return
      }

      const session = await actions.startConnectSession({
        orgId,
        provider: option.provider,
        selectedSources: option.sources,
      })

      await runDirectOauthWindow({
        connectUrl: session.connectUrl,
        sessionToken: session.sessionToken,
      })

      // Show connector in-flight while discover/sync settle
      setState((s) => { s.connectors = ((current) => [
        ...current.filter((item) => item.id !== option.id),
        {
          id: option.id,
          label: option.label,
          status: 'pending',
          connectUrl: session.connectUrl,
          sources: option.sources,
        },
      ])(s.connectors) })

      const source = {
        connectorId: option.id,
        label: option.label,
        orgId,
        provider: option.provider,
        sources: option.sources,
      }
      const [discoverResult, syncResult] = await Promise.allSettled([
        actions.discoverSource(source),
        actions.startIntegrationSync(source),
        option.provider === 'microsoft'
          ? actions.warmSharePointDiscovery({ orgId })
          : Promise.resolve({ warmed: false }),
      ])

      const coresFailed =
        discoverResult.status === 'rejected' || syncResult.status === 'rejected'

      setState((s) => { s.connectors = ((current) => [
        ...current.filter((item) => item.id !== option.id),
        {
          id: option.id,
          label: option.label,
          status: coresFailed ? 'partial' : 'connected',
          connectUrl: session.connectUrl,
          sources: option.sources,
        },
      ])(s.connectors) })

      if (coresFailed) {
        setError(`${option.label} tilkoblet, men synkronisering kan ha feilet. Sjekk innstillinger.`)
      }
      void queryClient.invalidateQueries({ queryKey: onboardingQueryKeys.graphPreview(orgId) })
    } catch (reason) {
      setError(translateApiError(reason, i18n.tr, { no: `Kunne ikke koble til ${option.label}.`, en: `Could not connect ${option.label}.` }))
    } finally {
      setConnectingId(undefined)
    }
  }

  async function commitPlan() {
    const orgId = state.organization.id
    const selectedPlan = activePlan()
    if (!orgId || !selectedPlan || committingPlan()) return

    setError(undefined)
    setCommittingPlan(true)

    try {
      await actions.setBrandTheme({
        mode: state.themeMode,
        primaryColor: state.website.branding?.themeColor || '#111111',
      })

      if (selectedPlan === 'trial') {
        setCheckoutSession(undefined)
        await actions.setPlan({
          orgId,
          plan: 'trial',
          onboarding: {
            recommendation: activeRecommendation(),
            sourceCount: sourceSummary().totalSourceCount,
          },
        })
        setState((s) => { s.step = 'assembly' })
        return
      }

      // Mirrors the settings/billing guard (WorkspaceSettingsPage.startPlanCheckout):
      // the enterprise/"Custom" tier's real price is never shown in this UI, so
      // it must never reach a real checkout session — open the contact-sales
      // modal instead, since custom pricing depends on what's needed beyond
      // what Verevon supports natively.
      const planOption = onboardingPlanCards.find((item) => item.id === selectedPlan)
      if (!planOption?.checkoutEnabled) {
        setContactSalesOpen(true)
        return
      }

      const checkout = await actions.startCheckout({
        orgId,
        plan: selectedPlan,
        successUrl: checkoutReturnUrl(),
        cancelUrl: `${window.location.origin}/onboarding?checkout=cancel`,
      })

      const checkoutSurface = resolveCheckoutSurface(checkout)
      switch (checkoutSurface) {
        case 'nexi-embedded':
        case 'hyperswitch-embedded':
          setCheckoutSession(checkout)
          return
        case 'redirect':
          window.location.assign(checkout.url!)
          return
        default:
          throw new Error('Payment checkout is not configured.')
      }
    } catch (reason) {
      setError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke lagre den valgte planen.', en: 'Could not save the selected plan.' }))
    } finally {
      setCommittingPlan(false)
    }
  }

  async function continueToPaywall() {
    setError(undefined)
    try {
      const lifecycle = await actions.fetchOnboardingLifecycle()
      // CREATED is now an honest 200 (no organization yet), not a 409 — the
      // paywall still needs an active org, so keep the user on this step.
      if (lifecycle.state === 'CREATED') {
        setError(i18n.tr(
          'Organisasjonen er ikke klar ennå. Prøv igjen før du velger plan.',
          'The organization is not ready yet. Try again before choosing a plan.',
        ))
        return
      }
      setState((s) => { s.step = 'paywall' })
    } catch (reason) {
      setError(
        translateApiError(reason, i18n.tr, {
          no: 'Organisasjonen er ikke klar ennå. Prøv igjen før du velger plan.',
          en: 'The organization is not ready yet. Try again before choosing a plan.',
        }),
      )
    }
  }

  async function finalizePaidCheckout(payment: {
    paymentId?: string
    clientSecret?: string
    status: string
    plan?: PlanId
  }) {
    const orgId = state.organization.id
    const selectedPlan = payment.plan ?? activePlan()
    if (!orgId || selectedPlan === 'trial') {
      setError('Betalingen kunne ikke knyttes til organisasjonen.')
      setState((s) => { s.step = 'paywall' })
      return
    }

    setError(undefined)
    setConfirmingCheckout(true)

    try {
      const status = await actions.confirmCheckout({
        orgId,
        plan: selectedPlan,
        paymentId: payment.paymentId,
        clientSecret: payment.clientSecret,
      })
      if (!isCheckoutActivatingStatus(status.status)) {
        setState((s) => { s.step = 'paywall' })
        setError('Betalingen er ikke fullført ennå.')
        return
      }

      await actions.setPlan({
        orgId,
        plan: selectedPlan,
        onboarding: {
          recommendation: activeRecommendation(),
          sourceCount: sourceSummary().totalSourceCount,
        },
      })
      setCheckoutSession(undefined)
      setState((s) => { s.step = 'assembly' })
      if (typeof window !== 'undefined' && window.location.search.includes('checkout=')) {
        window.history.replaceState(null, '', '/onboarding')
      }
    } catch (reason) {
      setState((s) => { s.step = 'paywall' })
      setError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke bekrefte betalingen.', en: 'Could not confirm the payment.' }))
    } finally {
      setConfirmingCheckout(false)
    }
  }

  function finishOnboarding() {
    if (finalizingOnboarding()) return

    setFinalizingOnboarding(true)
    const completionSnapshot = cloneOnboardingState(untrack(() => state))
    const completionRecommendation = activeRecommendation()
    const selectedPlan = activePlan()

    setAssemblyTicks(0)
    setAssemblyError(undefined)
    window.clearInterval(assemblyTimer)
    window.clearTimeout(completionTimer)

    assemblyTimer = window.setInterval(() => {
      setAssemblyTicks((current) => {
        if (current >= 4) {
          window.clearInterval(assemblyTimer)
          return current
        }

        return current + 1
      })
    }, 280)

    completionTimer = window.setTimeout(async () => {
      try {
        // The user can skip the organization step, but the backend only marks
        // onboarding COMPLETED once an org exists (PROFILE_READY). Auto-provision
        // a minimal default org so "skip everything" still completes.
        let orgId = completionSnapshot.organization.id
        if (!orgId) {
          const sessionUser = getSession().user
          const fallbackName =
            completionSnapshot.organization.name?.trim() ||
            sessionUser?.name?.trim() ||
            sessionUser?.email?.split('@')[0] ||
            'Min organisasjon'
          const createdOrg = await actions.createOrganization({
            name: fallbackName,
            plan: selectedPlan || 'trial',
          })
          orgId = createdOrg.id
        }

        const result = await actions.completeOnboarding({
          orgId,
          plan: selectedPlan,
          source: 'verevon-v3-onboarding',
          metadata: {
            selected_theme: completionSnapshot.themeMode,
            websites: [{ url: completionSnapshot.website.url, brief: completionSnapshot.website.brief }],
            connectors: completionSnapshot.connectors,
            recommendation: completionRecommendation,
            source_summary: summarizeOnboardingSources({
              connectors: completionSnapshot.connectors,
              websiteUrl: completionSnapshot.website.url,
            }),
          },
        })
        if (!result.completed) {
          setFinalizingOnboarding(false)
          setAssemblyError('Oppsett fullført, men noe gikk galt. Prøv igjen.')
          return
        }
        window.localStorage.removeItem(storageKey)
        const currentSession = getSession()
        const completedOrg =
          currentSession.activeOrg ??
          (orgId
            ? {
                id: orgId,
                name: completionSnapshot.organization.name?.trim() || 'Min organisasjon',
                role: 'owner',
              }
            : null)
        markSessionOnboardingComplete(completedOrg)
        // Refresh the session so onboardingStatus flips to COMPLETED, then
        // SPA-navigate — no full reload, and the guards now allow /dashboard.
        await loadSession()
        if (getSession().onboardingStatus !== 'COMPLETED') {
          markSessionOnboardingComplete(completedOrg)
        }
        navigate('/dashboard', { replace: true })
      } catch (reason) {
        setFinalizingOnboarding(false)
        setAssemblyError(
          translateApiError(reason, i18n.tr, {
            no: 'Kunne ikke fullføre oppsett. Prøv igjen.',
            en: 'Could not finish setup. Try again.',
          }),
        )
      }
    }, 1800)
  }

  function back() {
    const previous = onboardingSteps[currentStepIndex() - 1]
    // `post-signin` is a transient intro splash that auto-advances forward, so
    // stepping back into it just bounces. Treat it as "before the start": from
    // the first interactive step (website) — or post-signin itself — "back"
    // exits onboarding rather than landing on a screen that immediately skips.
    if (previous && previous !== 'post-signin') {
      setState((s) => { s.step = previous })
      return
    }
    if (!window.confirm(i18n.tr(
      'Vil du avslutte oppsettet og logge ut? Påbegynt oppsett blir slettet.',
      'Leave setup and sign out? Your unfinished setup will be deleted.',
    ))) {
      return
    }
    // Start of onboarding — "back" exits entirely: log out AND restore the
    // session to zero, so the next sign-in starts from a clean slate rather
    // than resuming half-finished progress. Order matters:
    //   1. Pause persistence so no debounced write resurrects the old state.
    //   2. Reset the in-memory store + wipe the local snapshot.
    //   3. Reset the server-side onboarding snapshot (while the session is
    //      still valid), so onMount's loadOnboardingState resumes from zero.
    //   4. Sign out, clear the in-memory session, and return to login.
    setFinalizingOnboarding(true)
    setState(() => createInitialOnboardingState())
    if (typeof window !== 'undefined') {
      window.localStorage.removeItem(storageKey)
    }
    void (async () => {
      await saveOnboardingState({
        actor: actions.actor,
        step: 'post-signin',
        state: createInitialOnboardingState(),
      }).catch(() => undefined)
      await signOut().catch(() => undefined)
      clearSession()
      navigate('/login', { replace: true })
    })()
  }

  function renderLeftStep(step: Step) {
    switch (step) {
      case 'post-signin':
        return <IntroStepContent />
      case 'website':
        return (
          <WebsiteStepContent
            website={state.website}
            onUrlInput={(value) => setState((s) => { s.website.url = value })}
            onBriefInput={(value) => setState((s) => { s.website.brief = value })}
            onRunPreview={runWebsitePreview}
            onContinue={() => setState((s) => { s.step = 'organization' })}
            onSkip={skipWebsite}
          />
        )
      case 'organization':
        return (
          <OrganizationStepContent
            organization={state.organization}
            searchResults={searchResults()}
            searching={searching()}
            submitting={submittingOrg()}
            websiteSkipped={state.websiteSkipped}
            onClearResults={clearSearchResults}
            onNameInput={(value) => setState((s) => { s.organization.name = value })}
            onSearch={runBrregSearch}
            onSelectResult={selectOrganizationResult}
            onSelectSize={(size) => setState((s) => { s.organization.size = size })}
            onToggleZdr={(value) => setState((s) => { s.organization.zeroDataRetention = value })}
            onContinue={submitOrganization}
            onSkipStep={() => setState((s) => { s.step = 'connect' })}
          />
        )
      case 'connect':
        return (
          <ConnectStepContent
            connectedSources={state.connectors}
            connectingId={connectingId()}
            onConnect={connectSource}
            onContinue={() => setState((s) => { s.step = 'social-proof' })}
            onSkip={() => setState((s) => { s.step = 'social-proof' })}
            onPrefetch={() => {
              if (state.connectors.length > 0) setRecommendationPrefetch(true)
            }}
          />
        )
      case 'social-proof':
        return <SocialProofStepContent onContinue={() => void continueToPaywall()} />
      case 'assembly':
        return (
          <AssemblyStepContent
            assemblyTicks={assemblyTicks()}
            error={assemblyError()}
            organizationName={state.organization.name || ''}
            onFinish={finishOnboarding}
          />
        )
      case 'paywall':
        return <section class="onboarding-copy"><p class="onboarding-eyebrow">Plan</p></section>
    }
  }

  function renderRightStep(step: Step) {
    switch (step) {
      case 'post-signin':
        return <IntroStepVisual onEnded={advanceFromIntro} />
      case 'website':
        return <WebsiteStepVisual website={state.website} />
      case 'organization':
        return <OrganizationStepVisual organization={state.organization} />
      case 'connect':
        return (
          <ConnectStepVisual
            connectedSources={state.connectors}
            organizationName={state.organization.name}
            currentUserId={sessionUser?.id ?? ''}
            currentUserName={sessionUser?.name?.trim() ?? ''}
          />
        )
      case 'social-proof':
        return <SocialProofStepVisual />
      case 'assembly':
        return (
          <AssemblyStepVisual
            activePlan={activePlan()}
            connectedSourceCount={sourceSummary().connectedSourceCount}
            organizationName={state.organization.name}
            websitePages={state.website.pages}
            recommendation={localizedRecommendation()}
          />
        )
      case 'paywall':
        return <div aria-hidden="true" class="onboarding-right-blank" />
    }
  }

  return (
    <Show
      when={currentStep() === 'paywall'}
      fallback={
        <OnboardingScreen
          steps={onboardingSteps}
          currentStep={currentStep()}
          currentStepIndex={currentStepIndex()}
          visibleStepNumber={visibleStepNumber()}
          onBack={back}
          onSelectStep={(step) => setState((s) => { s.step = step })}
          backHref="/"
          screenStyle={{
            '--onboarding-accent': '#111111',
            '--onboarding-rail': '#FF2E63',
          }}
          chromeStyle={{
            transform: `scale(${cardScale()})`,
            'transform-origin': 'center center',
          }}
        >
          <>
            <OnboardingBrandStrip branding={state.website.branding} websiteUrl={state.website.url} />
            <OnboardingFrame
              leftPaneHeight={leftPaneSize.height()}
              onLeftPaneRef={leftPaneSize.setElement}
              showScanner={state.step !== 'connect'}
              stepTransitionPhase={stepTransitionPhase()}
              left={
                <>
                  {renderLeftStep(displayedStep())}
                  <Show when={error()}>
                    {(message) => <p class="onboarding-error">{message()}</p>}
                  </Show>
                  <p class="onboarding-support-copy">
                    Står du fast? <a href="mailto:support@verevon.com">support@verevon.com</a>
                  </p>
                </>
              }
              right={renderRightStep(displayedStep())}
            />
          </>
        </OnboardingScreen>
      }
    >
      <OnboardingScreen
        paywall
        steps={onboardingSteps}
        currentStep={currentStep()}
        currentStepIndex={currentStepIndex()}
        visibleStepNumber={visibleStepNumber()}
        onBack={back}
        onSelectStep={(step) => setState((s) => { s.step = step })}
        screenStyle={{
          '--onboarding-accent': '#111111',
          '--onboarding-rail': '#FF2E63',
        }}
        chromeStyle={{
          transform: `scale(${cardScale()})`,
          'transform-origin': 'center center',
        }}
      >
        <PaywallStep
          activePlanId={activePlan()}
          checkoutReturnUrl={checkoutReturnUrl()}
          checkoutSession={checkoutSession()}
          committing={committingPlan()}
          confirmingCheckout={confirmingCheckout()}
          error={error()}
          identity={{
            orgName: state.organization.name,
            industry: state.organization.industry,
            orgNumber: state.organization.orgNumber,
            websiteUrl: state.website.url,
            websitePages: state.website.pages,
            connectedSourceCount: sourceSummary().connectedSourceCount,
            connectorCount: sourceSummary().connectorCount,
            sourceCount: sourceSummary().totalSourceCount,
            employeeCount: state.organization.employeeCount ?? approxEmployeesFromSize(state.organization.size),
            branding: state.website.branding,
          }}
          loadingRecommendation={recommendationQuery.isFetching}
          recommendation={localizedRecommendation()}
          onRefreshRecommendation={() => {
            void recommendationQuery.refetch()
          }}
          onSelectPlan={(planId) => {
            setCheckoutSession(undefined)
            setState((s) => { s.plan = planId })
          }}
          onConfirmCheckout={finalizePaidCheckout}
          onCommitPlan={commitPlan}
        />
      </OnboardingScreen>
      <ContactSalesModal
        open={contactSalesOpen()}
        onClose={() => setContactSalesOpen(false)}
        orgName={state.organization.name}
        employeeCount={state.organization.employeeCount}
        websiteUrl={state.website.url}
      />
    </Show>
  )
}
