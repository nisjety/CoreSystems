import { NextRequest } from 'next/server'

const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || 'http://integration-api:3026'

interface ConnectorTokenResponse {
  token?: string
  access_token?: string
  shop_domain?: string
}

async function getConnectorToken(
  organizationId: string,
  connectorType: string,
): Promise<ConnectorTokenResponse | null> {
  try {
    const res = await fetch(`${INTEGRATION_CORE_URL}/internal/connectors/token`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ organizationId, connectorType }),
      signal: AbortSignal.timeout(5_000),
    })

    if (res.status === 404) return null
    if (!res.ok) return null

    return res.json()
  } catch {
    return null
  }
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ customerId: string }> },
) {
  const { customerId } = await params
  const { searchParams } = new URL(request.url)
  const orgId = searchParams.get('orgId') || ''

  const [shopifyConn, stripeConn] = await Promise.all([
    getConnectorToken(orgId, 'shopify'),
    getConnectorToken(orgId, 'stripe'),
  ])

  const [shopifyResult, stripeResult] = await Promise.all([
    (async () => {
      if (!shopifyConn?.token || !shopifyConn?.shop_domain) return null
      try {
        const res = await fetch(
          `https://${shopifyConn.shop_domain}/admin/api/2024-01/customers/${customerId}/orders.json?limit=5`,
          {
            headers: {
              'X-Shopify-Access-Token': shopifyConn.token,
              'Content-Type': 'application/json',
            },
            signal: AbortSignal.timeout(8_000),
          },
        )
        if (!res.ok) return null
        const data = await res.json()
        return { orders: data.orders ?? [] }
      } catch {
        return null
      }
    })(),
    (async () => {
      const token = stripeConn?.token || stripeConn?.access_token
      if (!token) return null

      // customerId here is an email or Stripe customer ID — pass as email search
      try {
        const qs = new URLSearchParams({ limit: '1' })
        // If it looks like a Stripe customer id, search by that; otherwise treat as email
        if (customerId.startsWith('cus_')) {
          const res = await fetch(`https://api.stripe.com/v1/customers/${customerId}`, {
            headers: {
              Authorization: `Bearer ${token}`,
            },
            signal: AbortSignal.timeout(8_000),
          })
          if (!res.ok) return null
          const customer = await res.json()
          return { customer }
        }

        qs.set('email', customerId)
        const res = await fetch(`https://api.stripe.com/v1/customers?${qs}`, {
          headers: {
            Authorization: `Bearer ${token}`,
          },
          signal: AbortSignal.timeout(8_000),
        })
        if (!res.ok) return null
        const data = await res.json()
        return { customer: data.data?.[0] ?? null }
      } catch {
        return null
      }
    })(),
  ])

  return Response.json({
    shopify: shopifyResult,
    stripe: stripeResult,
  })
}
