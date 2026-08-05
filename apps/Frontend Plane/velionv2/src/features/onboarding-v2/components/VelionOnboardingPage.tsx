"use client";

/**
 * Host for the onboarding wizard. Owns the state machine and renders the
 * two-pane frame. Resumes mid-flow from localStorage (via the machine) and
 * jumps to the assembly step when Stripe checkout returns successfully.
 */

import { useEffect, useRef } from "react";
import { useSearchParams } from "next/navigation";

import { OnboardingFrame } from "./OnboardingFrame";
import { useOnboardingMachine } from "../lib/onboarding-machine";
import { organizationExists } from "../lib/onboarding-service";

export function VerevonOnboardingPage() {
  const machine = useOnboardingMachine();
  const searchParams = useSearchParams();
  const checkoutHandled = useRef(false);
  const validatedOrgId = useRef<string | null>(null);

  // After a successful Stripe checkout the user returns to
  // /onboarding?checkout=success. Once the machine has hydrated (so the org id
  // is restored), jump straight to assembly.
  useEffect(() => {
    if (checkoutHandled.current) return;
    if (!machine.hydrated) return;
    if (searchParams.get("checkout") === "success") {
      checkoutHandled.current = true;
      machine.goTo("assembly");
    }
  }, [machine, searchParams]);

  useEffect(() => {
    if (!machine.hydrated) return;

    const orgId = machine.state.organization?.id?.trim();
    if (!orgId) return;
    if (validatedOrgId.current === orgId) return;

    validatedOrgId.current = orgId;

    let cancelled = false;

    void organizationExists(orgId)
      .then((exists) => {
        if (cancelled || exists) return;
        window.console?.warn(
          `[onboarding] saved organization ${orgId} no longer exists; restarting at organization step`,
        );
        machine.invalidateOrganization();
      })
      .catch(() => {
        validatedOrgId.current = null;
      });

    return () => {
      cancelled = true;
    };
  }, [
    machine,
    machine.hydrated,
    machine.invalidateOrganization,
    machine.state.organization?.id,
  ]);

  return <OnboardingFrame machine={machine} />;
}
