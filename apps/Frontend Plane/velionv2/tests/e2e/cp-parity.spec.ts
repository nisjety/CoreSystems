import { expect, test } from "@playwright/test"

/**
 * Phase 5 — Control Plane parity E2E.
 *
 * Two robust gating checks (no backend needed — they exercise proxy.ts) plus a
 * mock-driven onboarding journey that simulates the Control Plane via
 * page.route (same approach as smoke.spec.ts), so it runs against `pnpm dev`
 * without a live stack. Run with: `pnpm test:e2e`.
 */

test.describe("workspace route gating (proxy.ts)", () => {
  test("unauthenticated /dashboard redirects to /login", async ({ page }) => {
    await page.goto("/dashboard")
    await expect(page).toHaveURL(/\/login/)
  })

  test("unauthenticated /onboarding redirects to /login", async ({ page }) => {
    await page.goto("/onboarding")
    await expect(page).toHaveURL(/\/login/)
  })

  test("unauthenticated /settings redirects to /login", async ({ page }) => {
    await page.goto("/settings")
    await expect(page).toHaveURL(/\/login/)
  })
})

/**
 * Full onboarding orchestration journey, Control Plane mocked at the network
 * boundary. Marked fixme until selectors are validated against the running UI
 * (the onboarding wizard copy/structure may shift). The mocks below assert the
 * key parity contracts: BREG search, org creation with brreg snapshot, plan
 * persistence, and completion.
 */
test.fixme(
  "onboarding orchestrates org (BREG) -> plan -> complete -> dashboard",
  async ({ page }) => {
    // Authenticated, not-yet-onboarded session.
    await page.route("**/api/auth/get-session**", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ user: { id: "u_1", email: "founder@acme.no", name: "Founder" } }),
      }),
    )
    await page.route("**/api/v1/me/session-context**", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ userId: "u_1", orgId: null, onboardingStatus: "NOT_STARTED" }),
      }),
    )

    // BREG search returns one company; selecting it autofills the org step.
    await page.route("**/api/org/api/v1/brreg/search**", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          results: [
            {
              organisasjonsnummer: "923609016",
              navn: "AQUATIQ AS",
              konkurs: false,
              underAvvikling: false,
              antallAnsatte: 42,
            },
          ],
          count: 1,
        }),
      }),
    )

    // Captured via a holder object so TS does not narrow these to their
    // initial values across the async route callbacks.
    const captured: {
      orgBody: Record<string, unknown> | null
      planSet: boolean
      completed: boolean
    } = { orgBody: null, planSet: false, completed: false }

    // Org creation MUST carry the brreg snapshot + org number.
    await page.route("**/api/org/orgs", async (route) => {
      captured.orgBody = route.request().postDataJSON() as Record<string, unknown>
      await route.fulfill({
        status: 201,
        contentType: "application/json",
        body: JSON.stringify({ id: "org_1", name: "AQUATIQ AS", slug: "aquatiq-as", plan: "free", org_number: "923609016" }),
      })
    })

    await page.route("**/api/org/orgs/org_1/plan", (route) => {
      captured.planSet = true
      route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ id: "org_1", plan: "standard" }) })
    })

    await page.route("**/api/v1/onboarding/status", (route) => {
      if (route.request().method() === "POST") captured.completed = true
      route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ success: true, data: { onboardingComplete: true } }) })
    })

    await page.goto("/onboarding")
    // TODO(validate-selectors): drive the wizard — org step BREG search, select
    // result, continue (asserts createOrgBody.brreg_data set), plan step,
    // continue, assembly/complete -> /dashboard.
    expect(captured.orgBody?.brreg_data ?? null).not.toBeNull()
    expect(captured.planSet).toBe(true)
    expect(captured.completed).toBe(true)
    await expect(page).toHaveURL(/\/dashboard/)
  },
)
