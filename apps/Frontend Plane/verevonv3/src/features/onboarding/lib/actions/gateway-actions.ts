import {
  cleanupSource,
  confirmCheckout,
  completeOnboarding,
  createOrganization,
  discoverSource,
  fetchGraphPreview,
  fetchOnboardingLifecycle,
  fetchOnboardingStatus,
  fetchSessionBootstrap,
  getBrowserActor,
  loadOnboardingState,
  recommendPlan,
  saveOnboardingState,
  searchBrreg,
  setBrandTheme,
  setPlan,
  startCheckout,
  startConnectSession,
  startIntegrationSync,
  startWebsiteIngest,
  translatePlanRecommendation,
  warmSharePointDiscovery,
  type ActionActor,
} from '@/features/onboarding/lib/api'

export function createOnboardingGatewayActions(actor: ActionActor = getBrowserActor()) {
  return {
    actor,
    cleanupSource: (input: Omit<Parameters<typeof cleanupSource>[0], 'actor'>) =>
      cleanupSource({ actor, ...input }),
    confirmCheckout: (input: Omit<Parameters<typeof confirmCheckout>[0], 'actor'>) =>
      confirmCheckout({ actor, ...input }),
    completeOnboarding: (input: Omit<Parameters<typeof completeOnboarding>[0], 'actor'>) =>
      completeOnboarding({ actor, ...input }),
    createOrganization: (input: Omit<Parameters<typeof createOrganization>[0], 'actor'>) =>
      createOrganization({ actor, ...input }),
    discoverSource: (input: Omit<Parameters<typeof discoverSource>[0], 'actor'>) =>
      discoverSource({ actor, ...input }),
    fetchGraphPreview,
    fetchOnboardingLifecycle,
    fetchOnboardingStatus,
    fetchSessionBootstrap,
    loadOnboardingState,
    recommendPlan,
    saveOnboardingState: (input: Omit<Parameters<typeof saveOnboardingState>[0], 'actor'>) =>
      saveOnboardingState({ actor, ...input }),
    searchBrreg,
    setBrandTheme: (input: Omit<Parameters<typeof setBrandTheme>[0], 'actor'>) =>
      setBrandTheme({ actor, ...input }),
    setPlan: (input: Omit<Parameters<typeof setPlan>[0], 'actor'>) => setPlan({ actor, ...input }),
    startCheckout: (input: Omit<Parameters<typeof startCheckout>[0], 'actor'>) =>
      startCheckout({ actor, ...input }),
    startConnectSession: (input: Omit<Parameters<typeof startConnectSession>[0], 'actor'>) =>
      startConnectSession({ actor, ...input }),
    startIntegrationSync: (input: Omit<Parameters<typeof startIntegrationSync>[0], 'actor'>) =>
      startIntegrationSync({ actor, ...input }),
    startWebsiteIngest: (input: Omit<Parameters<typeof startWebsiteIngest>[0], 'actor'>) =>
      startWebsiteIngest({ actor, ...input }),
    translatePlanRecommendation,
    warmSharePointDiscovery: (input: Omit<Parameters<typeof warmSharePointDiscovery>[0], 'actor'>) =>
      warmSharePointDiscovery({ actor, ...input }),
  }
}

export type OnboardingGatewayActions = ReturnType<typeof createOnboardingGatewayActions>
