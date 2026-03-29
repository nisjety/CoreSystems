// Core step components
export { ProfileStep, OrganizationStep, WebsiteStep, ConnectStep, TeamStep, CompleteStep } from './core'

// Page shell + transitions
export { OnboardingPage, DashboardTransition } from './page'

// Guards
export { OnboardingGuard } from './guards'

// UI sub-components
export { BrregSearch } from './ui'

// Types
export type {
  OnboardingState,
  OnboardingProfileData,
  OnboardingOrgData,
  OnboardingWebsiteData,
  OnboardingConnectData,
  OnboardingTeamData,
} from './types'
