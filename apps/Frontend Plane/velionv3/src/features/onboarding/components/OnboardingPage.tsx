import { useNavigate } from '@solidjs/router'
import { useQueryClient } from '@tanstack/solid-query'
import { Show, batch, createEffect, createMemo, createSignal, onCleanup, onMount, untrack } from 'solid-js'
import { createOnboardingGatewayActions } from '@/features/onboarding/lib/actions'
import {
  type BrregEnhet,
  type CheckoutSession,
  createEmptyPreviewResponse,
  getBrowserActor,
  saveOnboardingState,
} from '@/features/onboarding/lib/api'
import { signOut } from '@/shared/api/auth-client'
import { clearSession, getSession, loadSession, markSessionOnboardingComplete } from '@/shared/session/session-store'
import { createCrawlPreviewStream } from '@/features/onboarding/lib/crawl-preview'
import {
  type GraphDisplayNode,
  type OnboardingState,
  type PlanId,
  type Step,
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
  graphNodePosition,
  inferOrganizationQuery,
  rankBrregSuggestions,
  sizeFromEmployees,
  stepNumberFor,
} from '@/features/onboarding/lib/view'
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
import { createElementHeight } from '@/shared/ui/velion/createElementHeight'

const onboardingCardBaseHeight = 1140
const storageKey = 'velionv3.onboarding.state.v1'

export default function OnboardingPage() {
  // Identity comes from the validated session (RequireOnboarding guarantees an
  // authenticated user before this mounts); fall back to the dev actor only when
  // no session user is present (local development without auth-core).
  const sessionUser = getSession().user
  const actor = sessionUser
    ? { userId: sessionUser.id, userEmail: sessionUser.email, userName: sessionUser.name }
    : getBrowserActor()
  const navigate = useNavigate()
  const actions = createOnboardingGatewayActions(actor)
  const queryClient = useQueryClient()
  const [state, setState] = createOnboardingState(storageKey)
  const [searchResults, setSearchResults] = createSignal<BrregEnhet[]>([])
  const [searching, setSearching] = createSignal(false)
  const [orgAutoInferred, setOrgAutoInferred] = createSignal(false)
  const [submittingOrg, setSubmittingOrg] = createSignal(false)
  const [connectingId, setConnectingId] = createSignal<string>()
  const [committingPlan, setCommittingPlan] = createSignal(false)
  const [confirmingCheckout, setConfirmingCheckout] = createSignal(false)
  const [checkoutSession, setCheckoutSession] = createSignal<CheckoutSession>()
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
  const emptyGraph = createEmptyPreviewResponse()

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

  onMount(() => {
    const updateViewportHeight = () => setViewportHeight(window.innerHeight)
    updateViewportHeight()
    window.addEventListener('resize', updateViewportHeight)
    onCleanup(() => window.removeEventListener('resize', updateViewportHeight))

    const checkoutParams = new URLSearchParams(window.location.search)
    const checkoutState = checkoutParams.get('checkout')
    void actions.loadOnboardingState<OnboardingState>()
      .then((snapshot) => {
        if (snapshot?.state) {
          setState(reconcileOnboardingState(snapshot))
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
            setState('step', 'paywall')
            setError('Betalingen mangler referanse. Start betalingen på nytt.')
          }
        }

        if (checkoutState === 'cancel') {
          setState('step', 'paywall')
          setError('Betaling avbrutt.')
        }

        setHydratedFromServer(true)
      })
      .catch(() => setHydratedFromServer(true))

  })

  createEffect(() => {
    if (typeof window === 'undefined') return
    window.clearTimeout(introTimer)
    introTimer = undefined

    if (state.step !== 'post-signin') return
    introTimer = window.setTimeout(advanceFromIntro, state.introPlayed ? 600 : 3000)
  })

  // On reaching the organization step, infer the org from the website crawl and
  // pre-search Enhetsregisteret once, so verified matches appear without the
  // user re-typing what the crawl already discovered.
  createEffect(() => {
    if (state.step !== 'organization') return
    if (untrack(orgAutoInferred)) return
    if (untrack(() => Boolean(state.organization.orgNumber || state.organization.id))) return
    if (untrack(() => searchResults().length > 0)) return

    const inferred = inferOrganizationQuery(state.website)
    if (!inferred) return

    setOrgAutoInferred(true)
    if (!untrack(() => state.organization.name.trim())) {
      setState('organization', 'name', inferred)
    }
    void autoInferOrganizationFromWebsite(inferred)
  })

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
  const graphDisplayNodes = createMemo<GraphDisplayNode[]>(() => {
    const graph = graphQuery.data ?? emptyGraph
    const nodes =
      graph.nodes.length > 0
        ? graph.nodes.slice(0, 9)
        : [{ id: 'org', label: state.organization.name || 'Org', group: 'org' }]

    return nodes.map((node, index) => ({
      ...node,
      position: graphNodePosition(index, nodes.length),
    }))
  })
  const recommendedPlan = createMemo(() => state.recommendation?.planId ?? 'trial')
  const activePlan = createMemo(() => state.plan ?? recommendedPlan())
  const recommendationContext = createMemo<Record<string, unknown>>(() => {
    const graph = graphQuery.data
    return {
      organization: {
        name: state.organization.name,
        size: state.organization.size,
        employeeCount: state.organization.employeeCount,
      },
      website: {
        url: state.website.url,
        agentBrief: state.website.brief,
      },
      websites: state.website.url ? [{ url: state.website.url, agentBrief: state.website.brief }] : [],
      connectors: state.connectors.map((item) => ({ id: item.id, label: item.label })),
      locale: 'nb',
      sourceCount: state.connectors.length + (state.website.url ? 1 : 0),
      // --- Rich personalization signals (top-level so the gateway's
      // flatten-passthrough carries them through to the Model Plane prompt,
      // which explicitly reads goal/industry/connected-systems and
      // `context.dataPlane` graph evidence). ---
      orgNumber: state.organization.orgNumber,
      industry: state.organization.industry,
      orgForm: state.organization.orgForm,
      goal: state.website.brief,
      branding: state.website.branding,
      websitePages: state.website.pages,
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
  const recommendationQuery = createPlanRecommendationQuery(
    actions,
    () => recommendationContext(),
    () => state.step === 'paywall' && !state.recommendation,
  )
  const checkoutReturnUrl = createMemo(() =>
    typeof window === 'undefined'
      ? '/onboarding?checkout=success'
      : `${window.location.origin}/onboarding?checkout=success`,
  )

  createEffect(() => {
    const recommendation = recommendationQuery.data
    if (!recommendation) return
    setState('recommendation', recommendation)
    if (!untrack(() => state.plan)) setState('plan', recommendation.planId)
  })

  createEffect(() => {
    const reason = recommendationQuery.error
    if (state.step !== 'paywall' || !reason) return
    setError(reason instanceof Error ? reason.message : 'Could not compute a recommendation.')
  })

  function advanceFromIntro() {
    setState('introPlayed', true)
    setState('step', 'website')
  }

  function clearSearchResults() {
    setSearchResults([])
  }

  function skipWebsite() {
    setState('websiteSkipped', true)
    setState('step', 'organization')
  }

  function selectOrganizationResult(item: BrregEnhet) {
    setState('organization', 'name', item.navn)
    setState('organization', 'orgNumber', item.organisasjonsnummer)
    setState('organization', 'employeeCount', item.antallAnsatte)
    setState('organization', 'size', sizeFromEmployees(item.antallAnsatte))
    // Capture industry + org form from the Brreg entry — strong, specific
    // signals the AI plan recommender uses to personalize its reasoning.
    setState('organization', 'industry', item.naeringskode1?.beskrivelse)
    setState('organization', 'orgForm', item.organisasjonsform?.beskrivelse)

    // Pre-fill the website from the Brreg registry entry so a downstream ingest
    // has a URL to crawl — but never clobber a site the user already entered or
    // crawled in the website step. The website step stores the URL in
    // `https://`-prefixed form, so normalize to match.
    const site = (item.hjemmeside ?? '').trim().replace(/^https?:\/\//i, '').replace(/\s+/g, '')
    if (site && !state.website.url.trim()) setState('website', 'url', `https://${site}`)
  }

  async function runWebsitePreview() {
    const url = state.website.url.trim()
    if (!url || crawlPreview.previewing()) return
    const brief = state.website.brief
    const orgId = state.organization.id

    setError(undefined)
    setState('website', 'snippets', [])
    setState('website', 'pages', 0)
    setState('website', 'elements', 0)
    setState('website', 'status', 'starting')
    setState('website', 'warning', undefined)

    await crawlPreview.start(
      {
        url,
        brief,
        maxPages: 3,
        orgId,
      },
      {
        onStarted: (payload) => {
          setState('website', 'crawlJobId', payload.jobId)
        },
        onSnippet: (payload) => {
          setState('website', 'snippets', (current) => [...current, payload].slice(-12))
        },
        onProgress: (payload) => {
          batch(() => {
            setState('website', 'status', payload.status)
            setState('website', 'pages', payload.pages)
            setState('website', 'elements', payload.elements)
          })
        },
        onBranding: (payload) => {
          setState('website', 'branding', payload)
        },
        onWarning: (payload) => {
          setState('website', 'warning', payload.message || payload.code)
        },
        onDone: (payload) => {
          batch(() => {
            setState('website', 'status', payload.status === 'failed' ? 'failed' : 'completed')
            setState('website', 'pages', payload.pages || state.website.pages)
            setState('website', 'elements', payload.elements || state.website.elements)
          })
        },
      },
    ).catch((reason: unknown) => {
      setState('website', 'status', 'failed')
      setError(reason instanceof Error ? reason.message : 'Could not preview the website.')
    })
  }

  async function runBrregSearch() {
    if (!state.organization.name.trim()) return

    setSearching(true)
    setError(undefined)
    try {
      setSearchResults(await actions.searchBrreg(state.organization.name))
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not search Brreg.')
    } finally {
      setSearching(false)
    }
  }

  // Auto-fill the organization step from the website crawl findings: derive a
  // likely org name from the detected brand/site (or the URL host), seed the
  // name field, and pre-search Enhetsregisteret so verified matches surface
  // without the user re-typing. Best-effort — manual search still works on
  // failure. Mirrors velion v2's inferred-from-website suggestion flow.
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
        metadata: state.website.branding
          ? {
              onboarding_branding: {
                site_name: state.website.branding.siteName,
                theme_color: state.website.branding.themeColor,
                favicon: state.website.branding.favicon,
                logo_candidate: state.website.branding.logoCandidate,
                palette: state.website.branding.palette?.slice(0, 8),
              },
            }
          : undefined,
      })

      setState('organization', 'id', created.id)
      setState('organization', 'name', created.name || state.organization.name)

      if (websiteUrl) {
        await actions.startWebsiteIngest({
          orgId: created.id,
          url: websiteUrl,
          brief: state.website.brief,
        }).catch(() => undefined)
      }

      setState('step', 'connect')
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not create the organization.')
    } finally {
      setSubmittingOrg(false)
    }
  }

  async function connectSource(option: {
    id: string
    label: string
    provider: string
    sources: string[]
  }) {
    if (!state.organization.id) {
      setError('Create the organization before connecting sources.')
      return
    }

    const orgId = state.organization.id
    setConnectingId(option.id)
    setError(undefined)

    try {
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
      setState('connectors', (current) => [
        ...current.filter((item) => item.id !== option.id),
        { id: option.id, label: option.label, status: 'pending', connectUrl: session.connectUrl },
      ])

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

      setState('connectors', (current) => [
        ...current.filter((item) => item.id !== option.id),
        {
          id: option.id,
          label: option.label,
          status: coresFailed ? 'partial' : 'connected',
          connectUrl: session.connectUrl,
        },
      ])

      if (coresFailed) {
        setError(`${option.label} tilkoblet, men synkronisering kan ha feilet. Sjekk innstillinger.`)
      }
      void queryClient.invalidateQueries({ queryKey: onboardingQueryKeys.graphPreview(orgId) })
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : `Could not connect ${option.label}.`)
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
            recommendation: state.recommendation,
            sourceCount: state.connectors.length + 1,
          },
        })
        setState('step', 'assembly')
        return
      }

      const checkout = await actions.startCheckout({
        orgId,
        plan: selectedPlan,
        successUrl: checkoutReturnUrl(),
        cancelUrl: `${window.location.origin}/onboarding?checkout=cancel`,
      })

      if (checkout.url) {
        window.location.assign(checkout.url)
        return
      }

      if (!checkout.client_secret || !checkout.publishable_key) {
        throw new Error('Payment checkout is not configured.')
      }

      setCheckoutSession(checkout)
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not save the selected plan.')
    } finally {
      setCommittingPlan(false)
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
      setState('step', 'paywall')
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
      const normalizedStatus = status.status.toLowerCase()
      if (normalizedStatus !== 'succeeded' && normalizedStatus !== 'processing') {
        setState('step', 'paywall')
        setError('Betalingen er ikke fullført ennå.')
        return
      }

      await actions.setPlan({
        orgId,
        plan: selectedPlan,
        onboarding: {
          recommendation: state.recommendation,
          sourceCount: state.connectors.length + 1,
        },
      })
      setCheckoutSession(undefined)
      setState('step', 'assembly')
      if (typeof window !== 'undefined' && window.location.search.includes('checkout=')) {
        window.history.replaceState(null, '', '/onboarding')
      }
    } catch (reason) {
      setState('step', 'paywall')
      setError(reason instanceof Error ? reason.message : 'Could not confirm the payment.')
    } finally {
      setConfirmingCheckout(false)
    }
  }

  function finishOnboarding() {
    if (finalizingOnboarding()) return

    setFinalizingOnboarding(true)
    const completionSnapshot = cloneOnboardingState(untrack(() => state))
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
          source: 'velion-v3-onboarding',
          metadata: {
            selected_theme: completionSnapshot.themeMode,
            websites: [{ url: completionSnapshot.website.url, brief: completionSnapshot.website.brief }],
            connectors: completionSnapshot.connectors,
            recommendation: completionSnapshot.recommendation,
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
          reason instanceof Error ? reason.message : 'Kunne ikke fullføre oppsett. Prøv igjen.',
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
      setState('step', previous)
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
    setState(createInitialOnboardingState())
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
            onUrlInput={(value) => setState('website', 'url', value)}
            onBriefInput={(value) => setState('website', 'brief', value)}
            onRunPreview={runWebsitePreview}
            onContinue={() => setState('step', 'organization')}
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
            onNameInput={(value) => setState('organization', 'name', value)}
            onSearch={runBrregSearch}
            onSelectResult={selectOrganizationResult}
            onSelectSize={(size) => setState('organization', 'size', size)}
            onContinue={submitOrganization}
            onSkipStep={() => setState('step', 'connect')}
          />
        )
      case 'connect':
        return (
          <ConnectStepContent
            connectedSources={state.connectors}
            connectingId={connectingId()}
            onConnect={connectSource}
            onContinue={() => setState('step', 'social-proof')}
            onSkip={() => setState('step', 'social-proof')}
          />
        )
      case 'social-proof':
        return <SocialProofStepContent onContinue={() => setState('step', 'paywall')} />
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
        return <ConnectStepVisual graphNodes={graphDisplayNodes()} />
      case 'social-proof':
        return <SocialProofStepVisual />
      case 'assembly':
        return (
          <AssemblyStepVisual
            activePlan={activePlan()}
            connectorCount={state.connectors.length}
            organizationName={state.organization.name}
            websitePages={state.website.pages}
          />
        )
      case 'paywall':
        return <div aria-hidden class="onboarding-right-blank" />
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
          onSelectStep={(step) => setState('step', step)}
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
                    Står du fast? <a href="mailto:support@velion.com">support@velion.com</a>
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
        onSelectStep={(step) => setState('step', step)}
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
          loadingRecommendation={recommendationQuery.isFetching}
          recommendation={state.recommendation}
          onRefreshRecommendation={() => {
            void recommendationQuery.refetch()
          }}
          onSelectPlan={(planId) => {
            setCheckoutSession(undefined)
            setState('plan', planId)
          }}
          onConfirmCheckout={finalizePaidCheckout}
          onCommitPlan={commitPlan}
        />
      </OnboardingScreen>
    </Show>
  )
}
