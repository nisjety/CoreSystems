import { NextRequest, NextResponse } from 'next/server'

import { ChatStoreError, resolveChatActor } from '../../../chat/_lib/session-store'

const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || 'http://integration-api:3026'
const INTERNAL_API_KEY =
  process.env.AUTH_CORE_INTERNAL_API_KEY ||
  process.env.INTEGRATION_CORE_INTERNAL_API_KEY ||
  process.env.INTERNAL_API_KEY ||
  process.env.INTERNAL_SERVICE_SECRET

type IntegrationConnection = {
  provider?: string
  providerKey?: string
  nangoIntegrationId?: string
  userId?: string
  user_id?: string
  status?: string
  deletedAt?: string | null
  deleted_at?: string | null
}

export async function GET(
  _request: NextRequest,
  context: { params: Promise<{ sessionId: string }> | { sessionId: string } }
) {
  try {
    const actor = await resolveChatActor()
    const { sessionId } = await Promise.resolve(context.params)

    if (!sessionId) {
      return NextResponse.json({ error: 'Missing session id' }, { status: 400 })
    }

    if (!INTERNAL_API_KEY) {
      return NextResponse.json(
        { status: 'error', error_message: 'Service configuration error: missing integration internal key' },
        { status: 500 }
      )
    }

    const params = new URLSearchParams({
      organizationId: actor.orgId,
      userId: actor.userId,
    })

    const response = await fetch(`${INTEGRATION_CORE_URL}/api/v1/connections?${params.toString()}`, {
      method: 'GET',
      headers: {
        Accept: 'application/json',
        'X-Internal-Api-Key': INTERNAL_API_KEY,
      },
      cache: 'no-store',
    })

    const data = await response.json().catch(() => ({}))
    if (!response.ok) {
      return NextResponse.json(
        {
          status: 'error',
          error_message: data.detail || data.error || 'Failed to load integration status',
        },
        { status: response.status }
      )
    }

    const connections = ((data.data?.connections ?? data.connections ?? []) as IntegrationConnection[])
      .filter((connection) => !connection.deletedAt && !connection.deleted_at)

    const microsoftConnection = connections.find((connection) => {
      const provider = connection.providerKey || connection.provider || connection.nangoIntegrationId || ''
      const belongsToActor = (connection.userId || connection.user_id || actor.userId) === actor.userId
      return belongsToActor && ['microsoft', 'microsoft-graph'].includes(provider)
    })

    if (microsoftConnection) {
      return NextResponse.json({
        status: microsoftConnection.status || 'connected',
        provider: 'microsoft',
      })
    }

    return NextResponse.json({ status: 'pending' })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json(
        { status: 'error', error_message: error.message },
        { status: error.statusCode }
      )
    }

    return NextResponse.json(
      {
        status: 'error',
        error_message:
          error instanceof Error ? error.message : 'Failed to load integration status',
      },
      { status: 500 }
    )
  }
}
