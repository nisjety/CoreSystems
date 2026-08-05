import { expect, test } from '@playwright/test'
import {
  closeBrowserSession,
  createBrowserProfile,
  createBrowserSession,
  deleteBrowserProfile,
} from './browser-workspace-helpers'

/**
 * Regression test for the cross-session cookie leak in Quarry-v2's
 * ChromiumoxideDriver (`quarry-browser/src/chromiumoxide.rs`): every browser
 * session used to share one single global Chromium browser context, so a
 * cookie set by one session — even an unrelated, unprofiled, ephemeral one —
 * was visible to every other session, including a brand-new profile's very
 * first-ever navigation and `zdr: true` sessions. Fixed by giving each
 * session its own isolated CDP browser context (`Browser::create_browser_context`
 * on first tab, threaded via `browser_context_id` to later tabs on that same
 * session, disposed on release). This spec proves the fix holds end-to-end
 * through the real gateway + Quarry-edge, not just at the driver's own
 * `CHROMIUMOXIDE_TEST=1` unit-test level.
 */
test('a leaked cookie from an unrelated session never reaches a fresh profile session', async ({ page, baseURL }) => {
  test.setTimeout(120_000)
  const origin = baseURL ?? 'http://localhost:5173'
  const api = page.request
  const marker = `zdrleak${Date.now()}${Math.floor(Math.random() * 1_000_000)}`

  const sessionIds: string[] = []
  const profileIds: string[] = []

  try {
    // 1. "Leaker" session: no profile (ephemeral), sets a uniquely-named
    // cookie via httpbin, then closes. Under the pre-fix shared-context bug
    // this cookie landed in the one global Chromium context every session —
    // including totally unrelated ones — used.
    const leaker = await createBrowserSession(api, origin, {
      url: `https://httpbin.org/cookies/set/${marker}/leaked`,
    })
    sessionIds.push(leaker.session.id)
    await closeBrowserSession(api, leaker.session.id)

    // 2. Brand-new profile + a session attached to it, navigating straight to
    // https://httpbin.org/cookies (no /set) as its first-ever action. If the
    // shared-context bug were still present, the leaked cookie from step 1
    // would show up here even though this profile has never been used
    // before and never set that cookie itself.
    const profile = await createBrowserProfile(api, origin, { name: `zdr-e2e-${marker}` })
    profileIds.push(profile.profile_id)

    const created = await createBrowserSession(api, origin, {
      url: 'https://httpbin.org/cookies',
      profileId: profile.profile_id,
    })
    sessionIds.push(created.session.id)

    const snippet = created.observation?.dom_summary?.text_snippet ?? ''
    expect(snippet, "a fresh profile session must not observe another session's cookie").not.toContain(marker)
  } finally {
    for (const id of sessionIds) {
      await closeBrowserSession(api, id)
    }
    for (const id of profileIds) {
      await deleteBrowserProfile(api, id)
    }
  }
})
