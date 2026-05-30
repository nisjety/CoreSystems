import { NextRequest, NextResponse } from 'next/server'

import {
  ControlPlaneAuthError,
  requireSession,
} from '../../_lib/control-plane-auth'

const INTEGRATION_SERVICE_URL =
  process.env.INTEGRATION_CORE_URL ||
  process.env.INTEGRATION_ENGINE_URL ||
  'http://integration-api:3026'

const CONNECT_SESSION_TIMEOUT_MS = 35_000

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
    const authSession = await requireSession(request)
    const body = await request.json()
    const { org_id, provider, sources } = body

    const normalizedOrgId = typeof org_id === 'string' ? org_id.trim() : ''
    if (!normalizedOrgId) {
      return NextResponse.json(
        { error: 'Organization id is required before connecting sources.' },
        { status: 400 }
      )
    }

    if (!authSession.user.email) {
      return NextResponse.json(
        { error: 'A verified email is required before connecting sources.' },
        { status: 400 }
      )
    }

    const normalizedUserId = authSession.user.id
    const normalizedEmail = authSession.user.email
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

    const controller = new AbortController()
    const timeoutId = setTimeout(() => controller.abort(), CONNECT_SESSION_TIMEOUT_MS)

    let response: Response
    try {
      response = await fetch(`${INTEGRATION_SERVICE_URL}/api/v1/providers/${encodeURIComponent(normalizedProvider)}/connect-session`, {
        method: 'POST',
        headers,
        body: JSON.stringify({
          organizationId: normalizedOrgId,
          workspaceId: normalizedOrgId,
          userId: normalizedUserId,
          userEmail: normalizedEmail,
          selectedSources: requestedSources,
        }),
        cache: 'no-store',
        signal: controller.signal,
      })
    } catch (fetchError) {
      if (fetchError instanceof Error && fetchError.name === 'AbortError') {
        return NextResponse.json(
          { error: 'Integration service timeout' },
          { status: 504 }
        )
      }
      throw fetchError
    } finally {
      clearTimeout(timeoutId)
    }

    const data = await response.json().catch(() => ({}))

    if (!response.ok) {
      const payload = data as {
        error?: string | { message?: string; code?: string }
        detail?: string
      }
      const nestedError = payload?.error
      const message =
        typeof nestedError === 'string'
          ? nestedError
          : nestedError?.message ||
            payload?.detail ||
            'Failed to create Aqencia integration session'

      return NextResponse.json(
        {
          error: message,
          code: typeof nestedError === 'object' ? nestedError?.code : undefined,
          detail: payload?.detail,
        },
        { status: response.status }
      )
    }

    const sessionData = (data as { data?: unknown })?.data ?? data
    const connectSession = sessionData as {
      sessionToken?: string
      connectUrl?: string
      expiresAt?: string
      provider?: { key?: string }
    }

    return NextResponse.json({
      // Legacy-compatible shape expected by useIntegrationConnect
      connection: null,
      sync_jobs: [],
      // New fields used by OAuth-based connect flow
      session_id: connectSession.sessionToken,
      mode: 'connect_session',
      authorization_url: connectSession.connectUrl,
      connect_link: connectSession.connectUrl,
      requested_sources: requestedSources,
      provider: connectSession.provider?.key ?? normalizedProvider,
      expires_at: connectSession.expiresAt,
    })
  } catch (error) {
    if (error instanceof ControlPlaneAuthError) {
      return NextResponse.json(
        { error: error.message, code: error.code },
        { status: error.status }
      )
    }

    console.error('[api/connections/create] Failed:', error)
    return NextResponse.json(
      {
        error: error instanceof Error ? error.message : 'Internal server error',
      },
      { status: 500 }
    )
  }
}
