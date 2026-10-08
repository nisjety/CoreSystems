import { expect, test } from '@playwright/test'

/**
 * The org Zero Data Retention switch is gated on an attested provider (design task
 * Z-04). The gateway refuses to turn ZDR on while no provider carries a recorded
 * zero-retention attestation (`409 zdr_no_attested_provider`), and the security
 * page must not offer a switch that can only fail.
 *
 * READ-ONLY by default. Nothing here switches ZDR on or off for any organization.
 * The one request that could change state, a refused PATCH, runs only when
 * `ZDR_GATE_LIVE_PATCH=1` is set AND the live catalogue shows nothing attested AND
 * the org's ZDR is currently off AND the org is a dedicated test org (a `ZDR-TEST` org
 * or the e2e fixture org, so even a gateway build without the gate could only ever
 * switch a test org), so the
 * gateway is guaranteed to refuse it. If
 * a provider is attested (the gate is open) it never sends it. A dedicated ZDR test
 * org is seeded with `apps/gateway/scripts/zdr-seed-test-org.mjs`, never through here.
 *
 * What the live stack is expected to say today is derived from the live catalogue
 * itself, with the same rule as the gateway, the composer and the settings toggle
 * (`apps/gateway/tests/fixtures/zdr-model-catalogue-cases.json`), so the spec keeps
 * holding once a provider is attested.
 */

const SECURITY = '/settings/org-security'

type CatalogueModel = {
  modality?: string
  features?: string[]
  privacy_tier?: string
}

/** The gateway's rule: a CHAT model whose provider carries the zdr feature or a zdr tier. */
function attestedChatModel(models: CatalogueModel[]): boolean {
  return models.some(
    (model) =>
      model.modality === 'chat'
      && ((model.features ?? []).includes('zdr')
        || ['zdr_contractual', 'sovereign'].includes(model.privacy_tier ?? '')),
  )
}

async function liveCatalogueAttests(page: import('@playwright/test').Page): Promise<boolean> {
  const response = await page.request.get('/api/v1/models')
  expect(response.status(), 'the model catalogue must answer for the signed-in session').toBe(200)
  const body = (await response.json()) as { models?: CatalogueModel[] }
  expect(Array.isArray(body.models), 'the catalogue body must be {models: [...]}').toBeTruthy()
  return attestedChatModel(body.models ?? [])
}

test.describe('org ZDR switch gate', () => {
  test('the switch agrees with the live catalogue: disabled with the reason while nothing is attested', async ({ page }) => {
    const attested = await liveCatalogueAttests(page)

    await page.goto(SECURITY)
    const toggle = page.getByRole('switch', { name: /^zero data retention/i })
    await expect(toggle).toBeVisible()

    const noProvider = page.getByText(
      /ingen AI-leverandør i dette miljøet har en registrert nullbevaringsgodkjenning|no AI provider in this environment has a recorded zero-retention attestation/i,
    )
    if (attested) {
      // A provider is attested: the attestation reason must NOT be shown. (The switch may still
      // be locked for the plan, which is a separate reason, so it is not asserted enabled here.)
      await expect(noProvider).toHaveCount(0)
    } else {
      await expect(noProvider).toBeVisible()
      await expect(toggle).toBeDisabled()
      await expect(toggle).toHaveAttribute('aria-checked', 'false')
    }
    // Never clicked: this spec writes nothing.
  })

  test('the composer predicate and the page rule read the same catalogue shape', async ({ page }) => {
    // Pins the field names the gateway gate, the composer and the toggle all read, against the
    // live /v1/models body instead of a fixture: if model-gateway renames modality, features or
    // privacy_tier, this fails here before a gate silently refuses or admits everything.
    const response = await page.request.get('/api/v1/models')
    expect(response.status()).toBe(200)
    const body = (await response.json()) as { models?: CatalogueModel[] }
    const models = body.models ?? []
    expect(models.length).toBeGreaterThan(0)
    for (const model of models) {
      expect(typeof (model as { id?: unknown }).id, 'every model has an id').toBe('string')
      expect(typeof model.modality, `${JSON.stringify(model)} has a modality`).toBe('string')
      expect(Array.isArray(model.features), `${JSON.stringify(model)} has a features list`).toBeTruthy()
      expect(typeof model.privacy_tier, `${JSON.stringify(model)} has a privacy_tier`).toBe('string')
    }
    expect(models.some((model) => model.modality === 'chat'), 'a chat model exists').toBeTruthy()
  })

  test('a PATCH turning ZDR on is refused with 409 zdr_no_attested_provider and writes nothing (opt-in)', async ({ page }) => {
    test.skip(
      process.env.ZDR_GATE_LIVE_PATCH !== '1',
      'opt-in: set ZDR_GATE_LIVE_PATCH=1 to send the one refused PATCH. It is never sent while a provider is attested.',
    )
    expect(await liveCatalogueAttests(page), 'a provider is attested, so this PATCH would not be refused: refusing to send it').toBe(false)

    const current = await page.request.get('/api/v1/session/current')
    expect(current.status()).toBe(200)
    const orgId = ((await current.json()) as { data?: { org?: { id?: string | null } | null } }).data?.org?.id
    expect(orgId, 'an active org is needed').toBeTruthy()

    // This PATCH is only ever safe because the gateway refuses it. If the gateway that answers is a
    // build without the gate, it would switch ZDR on, and a false-to-true change also queues the
    // purge of that org's conversation drafts, which switching it back off cannot undo. So it is
    // only ever sent for a dedicated test org: a `ZDR-TEST` org (docs/ZDR_OPERATOR_SEED_RUNBOOK.md)
    // or the e2e fixture account's own org (tests/e2e/auth.setup.ts), never for a real one,
    // whatever the catalogue says (E2E_EMAIL can point this suite at any account).
    const named = await page.request.get(`/api/v1/orgs/${orgId}`)
    expect(named.status()).toBe(200)
    const namedBody = (await named.json()) as { name?: string; data?: { name?: string } }
    const orgName = namedBody.data?.name ?? namedBody.name ?? ''
    expect(
      orgName,
      `org ${orgId} is neither a ZDR-TEST org nor the e2e fixture org: this spec never sends the PATCH for any other org`,
    ).toMatch(/^\s*(ZDR-TEST|Verevon E2E Org)/i)

    const readZdr = async (): Promise<boolean> => {
      const response = await page.request.get(`/api/v1/orgs/${orgId}`)
      expect(response.status()).toBe(200)
      const org = (await response.json()) as { metadata?: { interactiveRetention?: { zdr?: boolean } } }
      return org.metadata?.interactiveRetention?.zdr === true
    }
    expect(await readZdr(), 'ZDR is already on for this org: nothing to prove here, and nothing to send').toBe(false)

    const origin = new URL(page.url() === 'about:blank' ? (process.env.E2E_BASE_URL || 'http://localhost:5199') : page.url()).origin
    const patchZdr = (zeroDataRetention: boolean) => page.request.patch(`/api/v1/orgs/${orgId}/settings`, {
      headers: { 'content-type': 'application/json', origin },
      data: { zeroDataRetention },
    })
    try {
      const response = await patchZdr(true)
      expect(response.status()).toBe(409)
      const body = (await response.json()) as { error?: { code?: string } }
      expect(body.error?.code).toBe('zdr_no_attested_provider')

      expect(await readZdr(), 'the refused PATCH must not have written anything').toBe(false)
    } finally {
      // Safety net. The PATCH is meant to be refused, but if it ever is NOT (the gateway that
      // answers is an older build without the gate, or the gate is broken), this spec would
      // have switched a real org's ZDR on. Turning it off is never gated, so put it back.
      try {
        if (await readZdr()) await patchZdr(false)
      } catch {
        // Nothing more can be done from here; the assertion above already failed loudly.
      }
    }
  })
})
