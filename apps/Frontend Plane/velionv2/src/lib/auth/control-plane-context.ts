import "server-only";
import { cache } from "react";
import { buildNavbarSeedFromControlPlaneContext } from "@/features/shell-v2/lib/navbar-server";
import { getRequestAuthState } from "@/lib/auth/request-auth-state";
import { fetchBillingAccount } from "@/lib/integrations/billing-core";
import { fetchOrganization } from "@/lib/integrations/org-core";
import { entitlementsToFeatures } from "@/lib/integrations/session-core";
import { fetchUserCoreJson } from "@/lib/integrations/user-core";
import type { RequestActor } from "@/lib/integrations/request-actor";
import {
  ANONYMOUS_CONTROL_PLANE_CONTEXT,
  type ControlPlaneContextValue,
  type ControlPlaneEntitlements,
} from "@/lib/control-plane/context-types";

type AppearanceSettings = {
  theme?: "light" | "dark" | "system" | "auto";
  colorScheme?: string | null;
};

function normalizeAppearanceTheme(theme: AppearanceSettings["theme"]) {
  return theme === "auto" ? "system" : theme ?? "system";
}

async function fetchAppearanceSettings(actor: RequestActor) {
  const settings = await fetchUserCoreJson<AppearanceSettings>(
    actor,
    "/api/v1/settings/appearance",
  );

  return {
    theme: normalizeAppearanceTheme(settings.theme),
    colorScheme: settings.colorScheme ?? null,
  };
}

function withNavbarSeed(
  context: Omit<ControlPlaneContextValue, "navbar">,
): ControlPlaneContextValue {
  return {
    ...context,
    navbar: buildNavbarSeedFromControlPlaneContext(context),
  };
}

/**
 * Composes the full Control Plane context for the current request:
 *   auth-core/session-core (identity + org + onboarding) + billing-core
 *   + org-core + appearance settings.
 *
 * Wrapped in React `cache()` so all components in one render share a single
 * fetch. Never throws on a billing-core outage — entitlements degrade to null.
 */
export const getControlPlaneContext = cache(
  async (): Promise<ControlPlaneContextValue> => {
    const auth = await getRequestAuthState();
    const user = auth.user;
    if (!user) return ANONYMOUS_CONTROL_PLANE_CONTEXT;

    const actor: RequestActor = {
      userId: user.id,
      email: user.email,
      name: user.name,
      avatar: user.image ?? undefined,
      cookieHeader: user.cookieHeader,
    };

    if (user.testAuth) {
      const orgId = user.testOrgId ?? "org_playwright";
      return withNavbarSeed({
        user: {
          id: user.id,
          email: user.email,
          name: user.name,
          image: user.image ?? null,
        },
        orgId,
        organization: {
          id: orgId,
          name: "Playwright Workspace",
          plan: "standard",
          status: "active",
        },
        role: "owner",
        onboardingStatus: "COMPLETED",
        onboardingComplete: true,
        entitlements: null,
        appearance: null,
      });
    }

    if (auth.controlSession) {
      const snap = auth.controlSession;
      const hasBilling =
        snap.billing != null ||
        (Array.isArray(snap.entitlements) && snap.entitlements.length > 0);
      const orgId = snap.organization?.id ?? null;
      const [organization, appearance] = await Promise.all([
        orgId
          ? fetchOrganization(actor, orgId).catch(() => null)
          : Promise.resolve(null),
        fetchAppearanceSettings(actor).catch(() => null),
      ]);

      return withNavbarSeed({
        user: {
          id: user.id,
          email: user.email,
          name: user.name,
          image: user.image ?? null,
        },
        orgId,
        organization:
          organization ??
          (snap.organization?.id
            ? {
                id: snap.organization.id,
                name: snap.organization.name ?? "Workspace",
                plan: snap.organization.plan,
              }
            : null),
        role: snap.organization?.role ?? null,
        onboardingStatus: auth.onboardingStatus,
        onboardingComplete: auth.onboardingComplete,
        entitlements: hasBilling
          ? {
              plan: snap.billing?.plan ?? snap.organization?.plan ?? "free",
              subscriptionStatus: snap.billing?.status ?? "active",
              features: entitlementsToFeatures(snap.entitlements),
              quotas: {},
              credits: 0,
            }
          : null,
        appearance,
      });
    }

    const orgId = auth.orgId;

    let entitlements: ControlPlaneEntitlements | null = null;
    if (orgId) {
      try {
        const account = await fetchBillingAccount(actor, orgId);
        if (account) {
          entitlements = {
            plan: account.plan,
            subscriptionStatus: account.subscriptionStatus,
            features: account.entitlements,
            quotas: account.quotaLimits,
            credits: account.credits,
          };
        }
      } catch {
        // Billing is eventually-consistent (NATS-provisioned). Never block.
        entitlements = null;
      }
    }

    const [organization, appearance] = await Promise.all([
      orgId ? fetchOrganization(actor, orgId).catch(() => null) : Promise.resolve(null),
      fetchAppearanceSettings(actor).catch(() => null),
    ]);

    return withNavbarSeed({
      user: {
        id: user.id,
        email: user.email,
        name: user.name,
        image: user.image ?? null,
      },
      orgId,
      organization,
      role: auth.role,
      onboardingStatus: auth.onboardingStatus,
      onboardingComplete: auth.onboardingComplete,
      entitlements,
      appearance,
    });
  },
);
