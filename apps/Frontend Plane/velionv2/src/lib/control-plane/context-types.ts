import type { NavbarPayload } from "@/features/shell-v2/lib/navbar-types";

/**
 * Shared, framework-agnostic Control Plane context types.
 * Imported by BOTH the server composer (control-plane-context.ts) and the
 * client provider (shell-v2/lib/control-plane-provider.tsx), so it must not
 * import server-only or React modules.
 */

export type ControlPlaneEntitlements = {
  plan: string
  subscriptionStatus: string
  /** Feature flag map, e.g. { "feature.sso": true } from billing-core. */
  features: Record<string, boolean>
  /** Quota limit map, e.g. { "documents": 1000 }. */
  quotas: Record<string, number>
  credits: number
}

export type ControlPlaneUser = {
  id: string
  email?: string
  name?: string
  image?: string | null
}

export type ControlPlaneOrganization = {
  id: string
  name: string
  slug?: string
  plan?: string
  status?: string
  primaryDomain?: string
  logoUrl?: string | null
  accentColor?: string | null
}

export type ControlPlaneAppearance = {
  theme: "light" | "dark" | "system"
  colorScheme?: string | null
}

export type ControlPlaneContextValue = {
  user: ControlPlaneUser | null
  orgId: string | null
  organization: ControlPlaneOrganization | null
  role: string | null
  onboardingStatus: string | null
  onboardingComplete: boolean
  entitlements: ControlPlaneEntitlements | null
  appearance: ControlPlaneAppearance | null
  navbar: NavbarPayload | null
}

export const ANONYMOUS_CONTROL_PLANE_CONTEXT: ControlPlaneContextValue = {
  user: null,
  orgId: null,
  organization: null,
  role: null,
  onboardingStatus: null,
  onboardingComplete: false,
  entitlements: null,
  appearance: null,
  navbar: null,
}

/** True when the org's plan grants the named feature flag. */
export function hasFeature(
  ctx: ControlPlaneContextValue,
  feature: string,
): boolean {
  return Boolean(ctx.entitlements?.features?.[feature])
}
