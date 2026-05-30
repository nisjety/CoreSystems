import { NextRequest } from "next/server";

const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || "http://integration-api:3026";

type ConnectorTokenResponse = {
  token?: string;
  access_token?: string;
  shop_domain?: string;
};

async function getConnectorToken(organizationId: string, connectorType: string): Promise<ConnectorTokenResponse | null> {
  try {
    const response = await fetch(`${INTEGRATION_CORE_URL}/internal/connectors/token`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ organizationId, connectorType }),
      cache: "no-store",
      signal: AbortSignal.timeout(5_000),
    });

    if (response.status === 404 || !response.ok) return null;
    return response.json() as Promise<ConnectorTokenResponse>;
  } catch {
    return null;
  }
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ customerId: string }> },
) {
  const { customerId } = await params;
  const { searchParams } = new URL(request.url);
  const orgId = searchParams.get("orgId") || "";

  const [shopifyConn, stripeConn] = await Promise.all([
    getConnectorToken(orgId, "shopify"),
    getConnectorToken(orgId, "stripe"),
  ]);

  const [shopifyResult, stripeResult] = await Promise.all([
    fetchShopifyContext(customerId, shopifyConn),
    fetchStripeContext(customerId, stripeConn),
  ]);

  return Response.json({
    shopify: shopifyResult,
    stripe: stripeResult,
  });
}

async function fetchShopifyContext(customerId: string, connection: ConnectorTokenResponse | null) {
  if (!connection?.token || !connection.shop_domain) return null;

  try {
    const response = await fetch(
      `https://${connection.shop_domain}/admin/api/2024-01/customers/${encodeURIComponent(customerId)}/orders.json?limit=5`,
      {
        headers: {
          "Content-Type": "application/json",
          "X-Shopify-Access-Token": connection.token,
        },
        cache: "no-store",
        signal: AbortSignal.timeout(8_000),
      },
    );

    if (!response.ok) return null;
    const payload = (await response.json()) as { orders?: unknown[] };
    return { orders: payload.orders ?? [] };
  } catch {
    return null;
  }
}

async function fetchStripeContext(customerId: string, connection: ConnectorTokenResponse | null) {
  const token = connection?.token || connection?.access_token;
  if (!token) return null;

  try {
    if (customerId.startsWith("cus_")) {
      const response = await fetch(`https://api.stripe.com/v1/customers/${encodeURIComponent(customerId)}`, {
        headers: { Authorization: `Bearer ${token}` },
        cache: "no-store",
        signal: AbortSignal.timeout(8_000),
      });

      if (!response.ok) return null;
      const customer = await response.json();
      return { customer };
    }

    const query = new URLSearchParams({ email: customerId, limit: "1" });
    const response = await fetch(`https://api.stripe.com/v1/customers?${query}`, {
      headers: { Authorization: `Bearer ${token}` },
      cache: "no-store",
      signal: AbortSignal.timeout(8_000),
    });

    if (!response.ok) return null;
    const payload = (await response.json()) as { data?: unknown[] };
    return { customer: payload.data?.[0] ?? null };
  } catch {
    return null;
  }
}
