import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  isPaidPlan,
  createOrganization,
  setOrganizationPlan,
  startCheckout,
  completeOnboarding,
  updateProfile,
  OnboardingServiceError,
  type OnboardingPlanId,
} from "@/features/onboarding-v2/lib/onboarding-service";

function makeFetchResponse(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as Response;
}

// ---------------------------------------------------------------------------
// isPaidPlan
// ---------------------------------------------------------------------------
describe("isPaidPlan", () => {
  it("returns false for 'free'", () => {
    expect(isPaidPlan("free")).toBe(false);
  });

  it("returns false for 'trial'", () => {
    expect(isPaidPlan("trial")).toBe(false);
  });

  it("returns true for 'hobby'", () => {
    expect(isPaidPlan("hobby")).toBe(true);
  });

  it("returns true for 'standard'", () => {
    expect(isPaidPlan("standard")).toBe(true);
  });

  it("returns true for 'pro'", () => {
    expect(isPaidPlan("pro")).toBe(true);
  });

  it("returns true for 'enterprise'", () => {
    expect(isPaidPlan("enterprise")).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// createOrganization
// ---------------------------------------------------------------------------
describe("createOrganization", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("POSTs to /api/org/orgs with snake_case body", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ id: "org-1", name: "Acme AS", slug: "acme-as", plan: "free" }),
    );

    await createOrganization({ name: "Acme AS", plan: "free", orgNumber: "987654321" });

    expect(mockFetch).toHaveBeenCalledOnce();
    const [url, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/org/orgs");
    expect(init.method).toBe("POST");

    const body = JSON.parse(init.body as string);
    expect(body.name).toBe("Acme AS");
    expect(body.slug).toBe("acme-as");
    expect(body.plan).toBe("free");
    expect(body.org_number).toBe("987654321");
  });

  it("normalizes snake_case response to camelCase CreatedOrganization", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({
        id: "org-1",
        name: "Acme AS",
        slug: "acme-as",
        plan: "free",
        org_number: "987654321",
        verification_status: "pending",
      }),
    );

    const result = await createOrganization({ name: "Acme AS" });
    expect(result.id).toBe("org-1");
    expect(result.orgNumber).toBe("987654321");
    expect(result.verificationStatus).toBe("pending");
  });

  it("defaults plan to 'free' when not supplied", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ id: "org-1", name: "No Plan AS", slug: "no-plan-as" }),
    );

    await createOrganization({ name: "No Plan AS" });
    const body = JSON.parse((mockFetch.mock.calls[0][1] as RequestInit).body as string);
    expect(body.plan).toBe("free");
  });

  it("includes brreg_data when provided", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ id: "org-1", name: "Test AS", slug: "test-as" }),
    );

    const brregData = { organisasjonsnummer: "123456789", navn: "Test AS", konkurs: false, underAvvikling: false };
    await createOrganization({ name: "Test AS", brregData });

    const body = JSON.parse((mockFetch.mock.calls[0][1] as RequestInit).body as string);
    expect(body.brreg_data).toEqual(brregData);
  });

  it("throws OnboardingServiceError on non-ok response", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ error: "Conflict" }, 409),
    );

    await expect(createOrganization({ name: "Acme AS" })).rejects.toBeInstanceOf(
      OnboardingServiceError,
    );
  });

  it("throws OnboardingServiceError with correct status", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeFetchResponse({ error: "Unauthorized" }, 401));

    let caught: unknown;
    try {
      await createOrganization({ name: "Acme" });
    } catch (e) {
      caught = e;
    }
    expect(caught).toBeInstanceOf(OnboardingServiceError);
    expect((caught as OnboardingServiceError).status).toBe(401);
  });
});

// ---------------------------------------------------------------------------
// setOrganizationPlan
// ---------------------------------------------------------------------------
describe("setOrganizationPlan", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("POSTs to /api/org/orgs/{orgId}/plan", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ id: "org-1", name: "Acme AS", slug: "acme-as", plan: "trial" }),
    );

    await setOrganizationPlan("org-1", "trial");

    const [url, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/org/orgs/org-1/plan");
    expect(init.method).toBe("POST");
    const body = JSON.parse(init.body as string);
    expect(body.plan).toBe("trial");
    expect(body.reason).toBe("onboarding");
  });

  it("throws OnboardingServiceError on non-ok", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeFetchResponse({ error: "Not found" }, 404));

    await expect(setOrganizationPlan("bad-id", "free")).rejects.toBeInstanceOf(
      OnboardingServiceError,
    );
  });
});

// ---------------------------------------------------------------------------
// startCheckout
// ---------------------------------------------------------------------------
describe("startCheckout", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("POSTs to /api/org/orgs/{orgId}/checkout-session", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ id: "cs_123", url: "https://stripe.com/pay" }),
    );

    const result = await startCheckout("org-1", "pro", {
      successUrl: "https://app.com/success",
      cancelUrl: "https://app.com/cancel",
    });

    const [url, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/org/orgs/org-1/checkout-session");
    expect(init.method).toBe("POST");
    const body = JSON.parse(init.body as string);
    expect(body.plan).toBe("pro");
    expect(body.successUrl).toBe("https://app.com/success");
    expect(body.cancelUrl).toBe("https://app.com/cancel");
    expect(result.url).toBe("https://stripe.com/pay");
  });

  it("throws OnboardingServiceError on non-ok", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeFetchResponse({ error: "Bad request" }, 400));

    await expect(
      startCheckout("org-1", "pro", {
        successUrl: "https://app.com/success",
        cancelUrl: "https://app.com/cancel",
      }),
    ).rejects.toBeInstanceOf(OnboardingServiceError);
  });
});

// ---------------------------------------------------------------------------
// completeOnboarding
// ---------------------------------------------------------------------------
describe("completeOnboarding", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("POSTs to /api/v1/onboarding/status", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeFetchResponse({}));

    await completeOnboarding({ step: "done" });

    const [url, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/v1/onboarding/status");
    expect(init.method).toBe("POST");
    const body = JSON.parse(init.body as string);
    expect(body.step).toBe("done");
  });

  it("throws OnboardingServiceError on non-ok", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeFetchResponse({ error: "Server error" }, 500));

    await expect(completeOnboarding()).rejects.toBeInstanceOf(OnboardingServiceError);
  });
});

// ---------------------------------------------------------------------------
// updateProfile — never throws
// ---------------------------------------------------------------------------
describe("updateProfile", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("does not throw when fetch resolves with error status", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse({}, 500));
    await expect(updateProfile({ name: "Alice" })).resolves.toBeUndefined();
  });

  it("does not throw when fetch rejects (network error)", async () => {
    vi.mocked(fetch).mockRejectedValueOnce(new Error("Network failure"));
    await expect(updateProfile({ name: "Alice" })).resolves.toBeUndefined();
  });
});
