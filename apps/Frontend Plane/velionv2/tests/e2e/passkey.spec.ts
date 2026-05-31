import { expect, test } from "@playwright/test"

/**
 * Passkey (WebAuthn) LIVE verification using a CDP VIRTUAL AUTHENTICATOR.
 *
 * Chrome's virtual authenticator simulates a platform authenticator (Face ID /
 * Touch ID / Windows Hello-like) with user verification auto-approved, so the
 * full register + authenticate ceremonies run with no real hardware.
 *
 * Requires a working auth backend so the server can generate + verify the
 * ceremony: either Control Plane mode (auth-core reachable via the /api/auth
 * proxy) or standalone mode (velionv2's own Better Auth + a Postgres
 * DATABASE_URL). Marked `fixme` until run against such a stack and the login
 * selectors are validated. Run with `pnpm test:e2e`.
 */
test.fixme(
  "register a passkey, then sign in with it (virtual authenticator)",
  async ({ page }) => {
    // 1. Attach a virtual platform authenticator via the Chrome DevTools Protocol.
    const client = await page.context().newCDPSession(page)
    await client.send("WebAuthn.enable")
    const { authenticatorId } = await client.send(
      "WebAuthn.addVirtualAuthenticator",
      {
        options: {
          protocol: "ctap2",
          transport: "internal",
          hasResidentKey: true,
          hasUserVerification: true,
          isUserVerified: true,
          automaticPresenceSimulation: true,
        },
      },
    )
    expect(authenticatorId).toBeTruthy()

    // 2. Sign in with an existing account (email/password) to reach settings.
    //    TODO(stack): seed a verified test user and complete the login form.
    await page.goto("/login")
    // await page.getByLabel(/e-?post|email/i).fill(process.env.E2E_USER_EMAIL!)
    // await page.getByLabel(/passord|password/i).fill(process.env.E2E_USER_PASSWORD!)
    // await page.getByRole("button", { name: /logg inn|sign in/i }).click()

    // 3. Register a passkey from the account Security section (new UI).
    await page.goto("/account")
    await page.getByRole("button", { name: /register a passkey/i }).click()
    await expect(page.getByText(/passkey registered/i)).toBeVisible()

    // The virtual authenticator should now hold exactly one resident credential.
    const afterRegister = await client.send("WebAuthn.getCredentials", {
      authenticatorId,
    })
    expect(afterRegister.credentials.length).toBeGreaterThanOrEqual(1)

    // 4. Sign out, then authenticate with the passkey on /login.
    //    TODO(stack): trigger sign-out via the account menu.
    await page.goto("/login")
    await page.getByRole("button", { name: /passkey/i }).click()
    await expect(page).toHaveURL(/\/(dashboard|onboarding)/)
  },
)
