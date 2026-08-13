import { expect, test } from '@playwright/test'

// A release fixture must point at an already active, Control-registered Space
// owned by the authenticated E2E principal. We deliberately do not create one
// through a browser test because Application's lifecycle/Control-registration
// saga is the authority being proven here.
const spaceRef = process.env.E2E_SPACE_REF?.trim()

test('Space shell uses server-composed context and never exposes an authority bearer', async ({ page }) => {
  test.skip(!spaceRef, 'E2E_SPACE_REF must name an active fixture Space')

  const canonical = await page.request.get(`/api/v1/spaces/${encodeURIComponent(spaceRef!)}/context`)
  expect(canonical.status()).toBe(200)
  const canonicalPayload = unwrap(await canonical.json())
  expect(canonicalPayload.space?.space_ref).toBe(spaceRef)
  expect(canonicalPayload.membership?.space_ref).toBe(spaceRef)
  expect(JSON.stringify(canonicalPayload)).not.toMatch(/space_decision_token|retrieval_decision_token|payload_digest/i)

  const forged = await page.request.get(`/api/v1/spaces/${encodeURIComponent(spaceRef!)}/context`, {
    headers: {
      'x-verevon-org-id': 'forged-org',
      'x-space-decision-token': 'forged-decision',
      'x-space-ref': 'forged-space',
    },
  })
  expect(forged.status()).toBe(200)
  const forgedPayload = unwrap(await forged.json())
  expect(forgedPayload.space?.space_ref).toBe(spaceRef)
  expect(forgedPayload.membership?.space_ref).toBe(spaceRef)
  expect(JSON.stringify(forgedPayload)).not.toContain('forged-')

  await page.goto(`/spaces/${encodeURIComponent(spaceRef!)}`)
  await expect(page.getByRole('heading', { name: canonicalPayload.space?.name ?? /Space/ })).toBeVisible()
  await expect(page.getByRole('link', { name: 'Chat' })).toHaveAttribute(
    'href',
    `/chat?space_ref=${encodeURIComponent(spaceRef!)}`,
  )
})

function unwrap(payload: any) {
  return payload?.data ?? payload
}
