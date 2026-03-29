import { NextRequest, NextResponse } from 'next/server'

const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || 'http://localhost:9026'

export async function POST(request: NextRequest) {
  try {
    const body = await request.json()
    const { code, org_id, user_id } = body

    if (!code || !org_id || !user_id) {
      return NextResponse.json(
        { error: 'Missing code, org_id, or user_id' },
        { status: 400 }
      )
    }

    // Get the redirect URI that was stored during initiate
    const redirectUri = `${process.env.NEXT_PUBLIC_BASE_URL || 'http://localhost:3000'}/onboarding/connect/callback`

    // Call integration-core to exchange code for token
    const response = await fetch(`${INTEGRATION_CORE_URL}/api/v1/oauth/callback`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        code,
        org_id,
        user_id,
        redirect_uri: redirectUri,
      }),
    })

    if (!response.ok) {
      console.error(`Integration-core returned ${response.status}`)
      const errorText = await response.text()
      console.error('Error:', errorText)
      return NextResponse.json(
        { error: 'Failed to exchange OAuth code for token' },
        { status: 500 }
      )
    }

    const data = await response.json()
    return NextResponse.json({
      success: true,
      token_id: data.id,
      microsoft_user_id: data.microsoft_user_id,
    })
  } catch (error) {
    console.error('OAuth callback error:', error)
    return NextResponse.json(
      { 
        error: error instanceof Error ? error.message : 'Internal server error',
        detail: error instanceof Error ? error.message : 'Unknown error'
      },
      { status: 500 }
    )
  }
}
