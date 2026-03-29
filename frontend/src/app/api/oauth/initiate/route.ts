import { NextRequest, NextResponse } from 'next/server'

const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || 'http://localhost:9026'

export async function POST(request: NextRequest) {
  try {
    const body = await request.json()
    const { org_id, user_id, redirect_uri } = body

    if (!org_id || !user_id) {
      return NextResponse.json(
        { error: 'Missing org_id or user_id' },
        { status: 400 }
      )
    }

    // Call integration-core to initiate OAuth
    const response = await fetch(`${INTEGRATION_CORE_URL}/api/v1/oauth/initiate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        org_id,
        user_id,
        redirect_uri,
      }),
    })

    if (!response.ok) {
      console.error(`Integration-core returned ${response.status}`)
      const errorText = await response.text()
      console.error('Error:', errorText)
      return NextResponse.json(
        { error: 'Failed to initiate OAuth flow' },
        { status: 500 }
      )
    }

    const data = await response.json()
    return NextResponse.json(data)
  } catch (error) {
    console.error('OAuth initiate error:', error)
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Internal server error' },
      { status: 500 }
    )
  }
}
