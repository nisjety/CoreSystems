"use client"

import { createContext, useContext, type ReactNode } from "react"
import {
  ANONYMOUS_CONTROL_PLANE_CONTEXT,
  hasFeature as hasFeatureFn,
  type ControlPlaneContextValue,
  type ControlPlaneEntitlements,
} from "@/lib/control-plane/context-types"

const ControlPlaneCtx = createContext<ControlPlaneContextValue | null>(null)

export function ControlPlaneProvider({
  value,
  children,
}: {
  value: ControlPlaneContextValue
  children: ReactNode
}) {
  return (
    <ControlPlaneCtx.Provider value={value}>{children}</ControlPlaneCtx.Provider>
  )
}

export function useControlPlaneContext(): ControlPlaneContextValue {
  // Degrade gracefully to the anonymous context when no provider is present
  // (unit tests, static prerender, Storybook). At runtime the workspace layout
  // always supplies the real value.
  return useContext(ControlPlaneCtx) ?? ANONYMOUS_CONTROL_PLANE_CONTEXT
}

export function useEntitlements(): ControlPlaneEntitlements | null {
  return useControlPlaneContext().entitlements
}

/** True when the current org's plan grants the named feature flag. */
export function useHasFeature(feature: string): boolean {
  const ctx = useControlPlaneContext()
  return hasFeatureFn(ctx, feature)
}

/** Current org id, or null when unauthenticated / pre-onboarding. */
export function useOrgId(): string | null {
  return useControlPlaneContext().orgId
}
