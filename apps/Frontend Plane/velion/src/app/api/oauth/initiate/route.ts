import { NextRequest, NextResponse } from 'next/server'

import { ChatStoreError, resolveChatActor } from '../../chat/_lib/session-store'

// In-cluster default matches `docker-compose` service name + container port.
// Localhost-only port `9026` was the wrong default and silently 401'd because
// the request never hit integration-core's auth middleware.
const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || 'http://integration-api:3026'

function normalizeProvider(value: unknown): string {
  const provider = String(value || 'microsoft').trim().toLowerCase()

  switch (provider) {
    case 'gdrive':
    case 'google_drive':
    case 'drive':
      return 'google-drive'
    case 'm365':
    case 'microsoft365':
    case 'microsoft-365':
    case 'teams':
    case 'sharepoint':
    case 'onedrive':
    case 'outlook':
      return 'microsoft'
    default:
      return provider || 'microsoft'
  }
}

export async function POST(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const body = await request.json()
    const { org_id, provider, sources } = body

    if (typeof org_id === 'string' && org_id.trim() && org_id.trim() !== actor.orgId) {
      return NextResponse.json(
        { error: 'Requested organization does not match the active session' },
        { status: 403 }
      )
    }

    const normalizedProvider = normalizeProvider(provider)
    const requestedSources = Array.isArray(sources)
      ? sources
          .filter((value: unknown): value is string => typeof value === 'string')
          .map((value) => value.trim().toLowerCase())
          .filter(Boolean)
      : []

    const headers = new Headers({ 'Content-Type': 'application/json' })
    const allowedForwardHeaders = ['cookie', 'authorization', 'user-agent', 'accept-language']
    for (const headerName of allowedForwardHeaders) {
      const headerValue = request.headers.get(headerName)
      if (headerValue) {
        headers.set(headerName, headerValue)
      }
    }

    const internalApiKey =
      process.env.AUTH_CORE_INTERNAL_API_KEY ||
      process.env.INTEGRATION_CORE_INTERNAL_API_KEY ||
      process.env.INTERNAL_API_KEY ||
      process.env.INTERNAL_SERVICE_SECRET

    if (!internalApiKey) {
      return NextResponse.json(
        { error: 'Service configuration error: missing integration internal key' },
        { status: 500 }
      )
    }
    headers.set('x-internal-api-key', internalApiKey)

    const response = await fetch(
      `${INTEGRATION_CORE_URL}/api/v1/providers/${encodeURIComponent(normalizedProvider)}/connect-session`,
      {
        method: 'POST',
        headers,
        body: JSON.stringify({
          organizationId: actor.orgId,
          workspaceId: actor.orgId,
          userId: actor.userId,
          userEmail: actor.userEmail,
          selectedSources: requestedSources,
        }),
      },
    )

    if (!response.ok) {
      const payload = await response.json().catch(() => ({}))
      return NextResponse.json(
        {
          error: payload.error || 'Failed to create Aqencia integration session',
          detail: payload.detail,
        },
        { status: response.status }
      )
    }

    const data = await response.json()
    const sessionData = data?.data ?? data

    return NextResponse.json({
      session_id: sessionData?.sessionToken,
      mode: 'connect_session',
      authorization_url: sessionData?.connectUrl,
      connect_link: sessionData?.connectUrl,
      requested_sources: requestedSources,
      provider: sessionData?.provider?.key ?? normalizedProvider,
      expires_at: sessionData?.expiresAt,
    })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }

    console.error('OAuth initiate error:', error)
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Internal server error' },
      { status: 500 }
    )
  }
}
