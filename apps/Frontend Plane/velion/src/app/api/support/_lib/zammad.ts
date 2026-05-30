// U7-2 (ui-ux-velion-gap.md): shared Zammad client helpers. When the
// integration is not configured (no token) all consumer routes return a
// uniform 503 instead of crashing on `getaddrinfo ENOTFOUND
// zammad-railsserver`. /helpdesk is scaffolded but the Zammad service is
// not part of the local stack today — set ZAMMAD_API_URL + ZAMMAD_API_TOKEN
// to enable the surface.

export const ZAMMAD_URL =
  process.env.ZAMMAD_API_URL || 'http://zammad-railsserver:3000'

export const ZAMMAD_TOKEN = process.env.ZAMMAD_API_TOKEN || ''

export function zammadConfigured(): boolean {
  return ZAMMAD_TOKEN.length > 0
}

export function zammadHeaders(): Record<string, string> {
  return {
    Authorization: `Token token=${ZAMMAD_TOKEN}`,
    'Content-Type': 'application/json',
  }
}

export function notConfiguredResponse(): Response {
  return Response.json(
    {
      error: 'support_not_configured',
      message:
        'The support / helpdesk integration (Zammad) is not configured in this environment. Set ZAMMAD_API_URL + ZAMMAD_API_TOKEN to enable.',
    },
    { status: 503 },
  )
}
