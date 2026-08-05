import { NextRequest, NextResponse } from 'next/server'

import { ChatStoreError, resolveChatActor } from '../../../chat/_lib/session-store'

const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || 'http://integration-api:3026'
const INTERNAL_API_KEY =
  process.env.AUTH_CORE_INTERNAL_API_KEY ||
  process.env.INTEGRATION_CORE_INTERNAL_API_KEY ||
  process.env.INTERNAL_API_KEY ||
  process.env.INTERNAL_SERVICE_SECRET

type RouteContext = {
  params: Promise<{ path: string[] }> | { path: string[] }
}

function normalizePath(parts: string[]): string {
  return parts
    .map((part) => part.trim())
    .filter(Boolean)
    .join('/')
}

function integrationCoreTarget(path: string): string | null {
  switch (path) {
    case 'health':
    case 'api/v1/health':
      return '/health'
    case 'integrations':
    case 'api/v1/integrations':
      return '/api/v1/integrations/nango/list'
    default:
      break
  }

  const integrationMatch = path.match(/^api\/v1\/integrations\/([^/]+)$/)
  if (integrationMatch?.[1]) {
    return `/api/v1/integrations/nango/${encodeURIComponent(integrationMatch[1])}`
  }

  const statusMatch = path.match(/^api\/v1\/connections\/([^/]+)\/status$/)
  if (statusMatch?.[1]) {
    return `/api/v1/connections/nango/${encodeURIComponent(statusMatch[1])}/status`
  }

  return null
}

export async function GET(request: NextRequest, context: RouteContext) {
  try {
    await resolveChatActor()

    if (!INTERNAL_API_KEY) {
      return NextResponse.json(
        { error: 'Service configuration error: missing integration internal key' },
        { status: 500 },
      )
    }

    const { path } = await Promise.resolve(context.params)
    const target = integrationCoreTarget(normalizePath(path))

    if (!target) {
      return NextResponse.json(
        { error: 'Unsupported Nango proxy path. Use the Verevon integration APIs.' },
        { status: 404 },
      )
    }

    const upstream = await fetch(`${INTEGRATION_CORE_URL}${target}${request.nextUrl.search}`, {
      method: 'GET',
      headers: {
        Accept: 'application/json',
        'X-Internal-Api-Key': INTERNAL_API_KEY,
      },
      cache: 'no-store',
    })

    const payload = await upstream.json().catch(() => ({}))
    return NextResponse.json(payload, { status: upstream.status })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }

    return NextResponse.json({ error: 'Failed to load Nango integration state' }, { status: 500 })
  }
}

export async function POST() {
  return NextResponse.json(
    { error: 'Direct Nango mutations are disabled. Use /api/connections/create.' },
    { status: 410 },
  )
}

export async function PATCH() {
  return NextResponse.json(
    { error: 'Direct Nango mutations are disabled. Use /api/connections/create.' },
    { status: 410 },
  )
}

export async function OPTIONS() {
  return new NextResponse(null, {
    status: 204,
    headers: {
      Allow: 'GET, POST, PATCH, OPTIONS',
    },
  })
}
