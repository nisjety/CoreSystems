import { NextRequest, NextResponse } from 'next/server'

const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || 'http://localhost:9026'

/**
 * Create M365 connections using existing auth-core session token
 * 
 * For users who already authenticated with Microsoft during login,
 * this creates connections and discovers documents without re-auth
 */
export async function POST(request: NextRequest) {
  try {
    const body = await request.json()
    const { org_id, user_id, sources } = body

    if (!org_id || !user_id) {
      return NextResponse.json(
        { error: 'org_id and user_id required' },
        { status: 400 }
      )
    }

    const authHeader = request.headers.get('authorization')
    const cookieHeader = request.headers.get('cookie')

    // Call integration-core to create connections using auth-core token
    const response = await fetch(
      `${INTEGRATION_CORE_URL}/api/v1/connections/from-auth-core`,
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': authHeader || '',
          'Cookie': cookieHeader || '',
        },
        body: JSON.stringify({
          org_id,
          user_id,
          sources: sources || [
            'sharepoint',
            'onedrive',
            'teams',
            'outlook',
          ],
        }),
      }
    )

    const data = await response.json()

    if (!response.ok) {
      return NextResponse.json(data, { status: response.status })
    }

    return NextResponse.json({
      success: true,
      connections_created: data.connections_created,
      documents_discovered: data.documents_discovered,
      message: data.message,
      token_source: data.token_source || 'auth-core',
    })
  } catch (error) {
    console.error('Connection creation error:', error)
    return NextResponse.json(
      { error: 'Failed to create connections' },
      { status: 500 }
    )
  }
}
