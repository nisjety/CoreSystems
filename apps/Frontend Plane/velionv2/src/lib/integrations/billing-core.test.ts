import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  fetchBillingAccount,
  checkEntitlement,
  BillingCoreError,
  type BillingAccount,
} from "@/lib/integrations/billing-core";
import type { RequestActor } from "@/lib/integrations/request-actor";

const TEST_ACTOR: RequestActor = {
  userId: "user-test",
  email: "test@example.com",
};

function makeFetchResponse(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as Response;
}

const VALID_RAW = {
  org_id: "org-42",
  plan: "pro",
  subscription_state: "active",
  entitlements: { "feature.sso": true },
  quota_limits: { documents: 5000 },
  credits: 250,
};

// ---------------------------------------------------------------------------
// fetchBillingAccount
// ---------------------------------------------------------------------------
describe("fetchBillingAccount", () => {
  beforeEach(() => {
    process.env.INTERNAL_API_KEY = "test-internal-key";
    process.env.BILLING_SERVICE_URL = "http://billing-test:3014";
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    delete process.env.INTERNAL_API_KEY;
    delete process.env.BILLING_SERVICE_URL;
    vi.unstubAllGlobals();
  });

  it("maps snake_case raw response to camelCase BillingAccount", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse(VALID_RAW));

    const result = await fetchBillingAccount(TEST_ACTOR, "org-42");

    expect(result).not.toBeNull();
    const account = result as BillingAccount;
    expect(account.orgId).toBe("org-42");
    expect(account.plan).toBe("pro");
    expect(account.subscriptionStatus).toBe("active");
    expect(account.entitlements).toEqual({ "feature.sso": true });
    expect(account.quotaLimits).toEqual({ documents: 5000 });
    expect(account.credits).toBe(250);
  });

  it("returns null for 404", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse({}, 404));

    const result = await fetchBillingAccount(TEST_ACTOR, "org-missing");
    expect(result).toBeNull();
  });

  it("throws BillingCoreError on non-ok, non-404 response", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse({}, 500));

    await expect(
      fetchBillingAccount(TEST_ACTOR, "org-42"),
    ).rejects.toBeInstanceOf(BillingCoreError);
  });

  it("throws BillingCoreError(503) when INTERNAL_API_KEY is missing", async () => {
    delete process.env.INTERNAL_API_KEY;
    delete process.env.INTERNAL_SERVICE_SECRET;

    let caught: unknown;
    try {
      await fetchBillingAccount(TEST_ACTOR, "org-42");
    } catch (e) {
      caught = e;
    }
    expect(caught).toBeInstanceOf(BillingCoreError);
    expect((caught as BillingCoreError).status).toBe(503);
  });

  it("defaults plan to 'free' when raw.plan is missing", async () => {
    const raw = { ...VALID_RAW, plan: undefined };
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse(raw));

    const result = await fetchBillingAccount(TEST_ACTOR, "org-42");
    expect(result?.plan).toBe("free");
  });

  it("defaults subscriptionStatus to 'active' when raw.subscription_state is missing", async () => {
    const raw = { ...VALID_RAW, subscription_state: undefined };
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse(raw));

    const result = await fetchBillingAccount(TEST_ACTOR, "org-42");
    expect(result?.subscriptionStatus).toBe("active");
  });

  it("defaults credits to 0 when raw.credits is not a number", async () => {
    const raw = { ...VALID_RAW, credits: "not-a-number" };
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse(raw));

    const result = await fetchBillingAccount(TEST_ACTOR, "org-42");
    expect(result?.credits).toBe(0);
  });

  it("throws BillingCoreError(502) when fetch rejects (network error)", async () => {
    vi.mocked(fetch).mockRejectedValueOnce(new Error("ECONNREFUSED"));

    let caught: unknown;
    try {
      await fetchBillingAccount(TEST_ACTOR, "org-42");
    } catch (e) {
      caught = e;
    }
    expect(caught).toBeInstanceOf(BillingCoreError);
    expect((caught as BillingCoreError).status).toBe(502);
  });
});

// ---------------------------------------------------------------------------
// checkEntitlement
// ---------------------------------------------------------------------------
describe("checkEntitlement", () => {
  beforeEach(() => {
    process.env.INTERNAL_API_KEY = "test-internal-key";
    process.env.BILLING_SERVICE_URL = "http://billing-test:3014";
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    delete process.env.INTERNAL_API_KEY;
    delete process.env.BILLING_SERVICE_URL;
    vi.unstubAllGlobals();
  });

  it("returns true when response is ok and allowed is true", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse({ allowed: true }));

    const result = await checkEntitlement(TEST_ACTOR, "org-42", "feature.sso");
    expect(result).toBe(true);
  });

  it("returns false when status is 402", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse({}, 402));

    const result = await checkEntitlement(TEST_ACTOR, "org-42", "feature.sso");
    expect(result).toBe(false);
  });

  it("returns false when allowed is false in response body", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      makeFetchResponse({ allowed: false }),
    );

    const result = await checkEntitlement(TEST_ACTOR, "org-42", "feature.sso");
    expect(result).toBe(false);
  });

  it("returns false on network error (fetch rejects)", async () => {
    vi.mocked(fetch).mockRejectedValueOnce(new Error("Network failure"));

    const result = await checkEntitlement(TEST_ACTOR, "org-42", "feature.sso");
    expect(result).toBe(false);
  });

  it("returns false on non-ok response", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse({}, 500));

    const result = await checkEntitlement(TEST_ACTOR, "org-42", "feature.sso");
    expect(result).toBe(false);
  });

  it("calls correct URL for the feature", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse({ allowed: true }));

    await checkEntitlement(TEST_ACTOR, "org-42", "feature.sso");

    const url = vi.mocked(fetch).mock.calls[0][0] as string;
    expect(url).toBe(
      "http://billing-test:3014/api/v1/billing/orgs/org-42/entitlements/feature.sso",
    );
  });
});
