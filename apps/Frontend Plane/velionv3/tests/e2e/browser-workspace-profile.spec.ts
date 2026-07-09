import { expect, test } from '@playwright/test'

import { closeSessionViaApi, closeSessionViaUi, deleteProfileViaApi, resetHome, submitBrowserUrl } from './browser-workspace-helpers'

/**
 * Phase 6 capability 2: profile/cookie persistence, driven through the real
 * "Innhent" composer UI — create a named persistent profile, browse against
 * a real cookie-setting target with it attached, close the session, then use
 * the composer's own "Sjekk profil" (check profile) button to confirm the
 * profile's persisted signal count went from empty to non-empty.
 *
 * Deliberately uses the `restore-probe` UI affordance (which reads the
 * Postgres-backed profile snapshot — see `apps/Ingestion Plane/Quarry-v2`'s
 * `profile_routes.rs::restore_probe`) rather than re-opening a second live
 * session and eyeballing rendered cookie JSON: a KNOWN, separately-tracked
 * bug (cross-session cookie bleed through the shared Chromium browser
 * context — see `browser-workspace-zdr.spec.ts`) would make a live-session
 * re-check pass even if PROFILE-scoped persistence itself were broken. The
 * restore-probe path is immune to that confound because it reads the
 * durable snapshot directly, which is what this spec is actually meant to
 * prove (the Phase 3/6 Postgres profile store).
 *
 * Uses `postman-echo.com` rather than `httpbin.org` for the cookie-setting
 * target — both are free community test services, but `httpbin.org` was
 * observed returning sustained 503s during this suite's own development,
 * while Postman's company-run echo service has proven materially more
 * reliable for CI-style automated use.
 */
const COOKIE_TARGET = 'https://postman-echo.com/cookies/set?velion_e2e_ui_probe=round_trip_ok'

test.describe('browser workspace: profile and cookie persistence', () => {
  let sessionId: string | null = null
  let profileId: string | null = null

  test.afterEach(async ({ request }) => {
    await closeSessionViaApi(request, sessionId)
    await deleteProfileViaApi(request, profileId)
    sessionId = null
    profileId = null
  })

  test('a named profile accumulates real, durable signals after a browsing session and the composer surfaces them', async ({ page }) => {
    test.setTimeout(90_000)
    await resetHome(page)

    // 1. Create a new named profile through the composer's picker.
    // CSS-class targeting: `getByLabel('Nettleserprofil')` is a substring,
    // case-insensitive match against EVERY element's accessible name, so it
    // also matches the "Oppdater nettleserprofiler" / "Sjekk nettleserprofil"
    // buttons — a `select`-specific locator avoids that strict-mode
    // violation.
    const profileSelect = page.locator('.dashboard-knowledge-composer__browser-profile select')
    await expect(profileSelect).toBeEnabled({ timeout: 15_000 })
    // "new" is the literal <option value> (see `newProfileChoice` in
    // KnowledgeComposer.tsx) — selecting by value, not by the i18n-translated
    // visible label, keeps this correct regardless of locale.
    await profileSelect.selectOption('new')
    const profileName = `e2e-ui-profile-${Date.now()}`
    await page.getByLabel('Navn på ny profil').fill(profileName)
    const [createProfileResponse] = await Promise.all([
      page.waitForResponse((res) => res.url().includes('/api/v1/browser/profiles') && res.request().method() === 'POST'),
      page.locator('.dashboard-knowledge-composer__browser-profile-create-submit').click(),
    ])
    expect(createProfileResponse.status(), await createProfileResponse.text()).toBe(200)
    const createdProfile = (await createProfileResponse.json()) as { data: { profile_id: string } }
    profileId = createdProfile.data.profile_id
    expect(profileId).toBeTruthy()
    await expect(profileSelect).toHaveValue(profileId!)

    // 2. Fill the URL (a real cookie-setting endpoint) and confirm the probe
    // reports an EMPTY profile before any browsing has happened yet.
    const urlInput = page.locator('.dashboard-knowledge-composer__input')
    await urlInput.fill(COOKIE_TARGET)
    await page.getByLabel('Sjekk nettleserprofil').click()
    await expect(page.locator('.dashboard-knowledge-composer__browser-profile-status')).toHaveText(/Tom profil/, { timeout: 15_000 })

    // 3. Submit the URL with the profile attached — a real session browses
    // the cookie-setting target — then close it through the UI (which
    // triggers the real profile-snapshot flush on session close).
    sessionId = await submitBrowserUrl(page, COOKIE_TARGET)
    // The rendered page is an `<img alt="Gjengitt nettleserside for ...">`,
    // not a form control — `getByAltText` matches its accessible name;
    // `getByLabel` never considers `alt` and would find nothing here.
    await expect(page.getByAltText(/Gjengitt nettleserside for .+/)).toBeVisible({ timeout: 20_000 })
    await closeSessionViaUi(page)
    sessionId = null // closed via UI; afterEach's API cleanup becomes a no-op

    // 4. Re-open the composer's ingest tab (closing the session returns to
    // the plain form) and probe the SAME profile again — it must now report
    // at least one durable signal, proving persistence survived the close.
    await page.getByLabel('Sjekk nettleserprofil').click()
    await expect(page.locator('.dashboard-knowledge-composer__browser-profile-status')).toHaveText(/\d+ lagrede signaler/, { timeout: 15_000 })
    const summaryText = await page.locator('.dashboard-knowledge-composer__browser-profile-status').innerText()
    const match = /(\d+) lagrede signaler/.exec(summaryText)
    expect(match, `expected a "N lagrede signaler" summary, got: ${summaryText}`).not.toBeNull()
    expect(Number(match![1])).toBeGreaterThan(0)
  })
})
