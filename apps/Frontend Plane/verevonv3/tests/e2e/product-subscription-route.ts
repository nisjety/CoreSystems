import { expect, type Page } from '@playwright/test'

// Standing test constraint: never substitute another model or bill an API
// provider when this user-owned subscription is unavailable.
export const PRODUCT_MODEL = 'gpt-5.6-terra'
export const PRODUCT_PROVIDER = 'openai-codex-subscription'

export async function requireProductSubscription(page: Page, mockedInference = false) {
  const response = await page.request.get('http://localhost:3011/api/inference-core/token')
  expect(response.ok(), 'A local authenticated test account is required').toBe(true)
  const { token } = await response.json() as { token: string }
  expect(typeof token, 'Inference authentication was not available').toBe('string')
  // Scope selection only. The backend verifies the token; this grants nothing.
  const claims = JSON.parse(Buffer.from(token.split('.')[1], 'base64url').toString()) as { org_id: string; zdr?: boolean }
  expect(claims.zdr, 'Subscription tests must use an account whose policy permits this provider').not.toBe(true)
  let connectionId: string
  if (mockedInference) {
    connectionId = 'transport-fixture-only-no-live-inference'
    await page.route('**/api/v1/integrations/connections', route => route.fulfill({
      status: 200, contentType: 'application/json', body: JSON.stringify({ data: { connections: [
        { id: connectionId, providerKey: PRODUCT_PROVIDER, status: 'active' },
      ] } }),
    }))
  } else {
    const connectionsResponse = await page.request.get('/api/v1/integrations/connections', { headers: { 'x-verevon-org-id': claims.org_id } })
    expect(connectionsResponse.ok(), 'Could not read the test account’s subscription connections').toBe(true)
    const envelope = await connectionsResponse.json()
    const data = envelope.data ?? envelope
    const rows = (Array.isArray(data) ? data : data.connections ?? []) as Record<string, unknown>[]
    const connection = rows.find(row =>
      (row.providerKey ?? row.provider_key ?? row.providerId) === PRODUCT_PROVIDER
      && String(row.status).toLowerCase() === 'active' && !row.deletedAt && !row.deleted_at)
    expect(Boolean(connection), 'Connect ChatGPT in Verevon Integrations for the local test account. Tests will not fall back to Balance or Claude.').toBe(true)
    connectionId = String(connection!.id)
  }
  await page.addInitScript(({ orgId, connectionId, model, provider }) => {
    localStorage.setItem(`verevon.ai-model-selection.v1:${orgId}`, JSON.stringify({ model, provider, label: 'ChatGPT Terra', subscriptionConnectionId: connectionId }))
  }, { orgId: claims.org_id, connectionId, model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
  await page.route('**/api/v1/chat/stream', async route => {
    const request = route.request().postDataJSON() as Record<string, unknown>
    if (request.model !== PRODUCT_MODEL || request.provider !== PRODUCT_PROVIDER || request.subscription_connection_id !== connectionId) {
      await route.abort('blockedbyclient')
      throw new Error('Blocked product inference: the request did not preserve the selected ChatGPT Terra subscription')
    }
    // Mock fixtures register fulfill handlers after this guard. If none handles
    // the request, stop here; a fixture must never spill into live inference.
    if (mockedInference) await route.abort('blockedbyclient')
    else await route.fallback()
  })
}
