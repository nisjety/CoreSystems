import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  STORAGE_KEY,
  useOnboardingMachine,
} from "@/features/onboarding-v2/lib/onboarding-machine";

describe("useOnboardingMachine", () => {
  beforeEach(() => {
    window.localStorage.clear();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("invalidates a stale organization while preserving the draft fields", async () => {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        step: "paywall",
        introPlayed: true,
        startedAt: 1,
        organization: {
          id: "org_1780",
          name: "Aquatiq AS",
          plan: "trial",
          size: "medium",
          brregOrgNumber: "123456789",
          employeeCount: 94,
        },
        website: {
          url: "https://aquatiq.com",
          agentBrief: "Support shoppers on the storefront",
        },
        connectors: [{ id: "shopify", label: "Shopify" }],
      }),
    );

    const { result } = renderHook(() => useOnboardingMachine());

    await waitFor(() => expect(result.current.hydrated).toBe(true));

    act(() => {
      result.current.invalidateOrganization();
    });

    expect(result.current.state.step).toBe("organization");
    expect(result.current.state.organization).toMatchObject({
      name: "Aquatiq AS",
      size: "medium",
      brregOrgNumber: "123456789",
      employeeCount: 94,
    });
    expect(result.current.state.organization?.id).toBeUndefined();
    expect(result.current.state.organization?.plan).toBeUndefined();
    expect(result.current.state.connectors).toEqual([]);

    const stored = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "null") as {
      step: string;
      organization?: { id?: string; plan?: string; name?: string };
      connectors?: unknown[];
    } | null;

    expect(stored?.step).toBe("organization");
    expect(stored?.organization?.id).toBeUndefined();
    expect(stored?.organization?.plan).toBeUndefined();
    expect(stored?.organization?.name).toBe("Aquatiq AS");
    expect(stored?.connectors).toEqual([]);
  });
});
