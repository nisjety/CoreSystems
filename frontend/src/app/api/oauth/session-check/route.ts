import { NextRequest, NextResponse } from 'next/server'

const INTEGRATION_CORE_URL = process.env.INTEGRATION_CORE_URL || 'http://localhost:9026'

/**
 * Check if user has existing Microsoft OAuth token from auth-core session
 * 
 * This endpoint enables seamless UX:
 * 1. User signs in with Microsoft
 * 2. Onboarding step 4 checks /api/oauth/session-check
 * 3. If user already authenticated with Microsoft during login, skip re-auth
 * 4. Just show source selection (SharePoint, OneDrive, Teams, Outlook)
 * 5. User clicks "Activate" and documents are auto-discovered
 */
export async function GET(request: NextRequest) {
  try {
    const authHeader = request.headers.get('authorization')
    const cookieHeader = request.headers.get('cookie')

    if (!authHeader && !cookieHeader) {
      return NextResponse.json(
        {
          has_microsoft: false,
          has_google: false,
          authenticated: false,
          error: 'No auth headers provided',
        },
        { status: 400 }
      )
    }

    // Call integration-core to validate session with auth-core
    const response = await fetch(`${INTEGRATION_CORE_URL}/api/v1/session/validate`, {
      method: 'POST',
      headers: {
        'Authorization': authHeader || '',
        'Content-Type': 'application/json',
        'Cookie': cookieHeader || '',
      },
    })

    const data = await response.json()

    if (!response.ok || !data.valid) {
      return NextResponse.json(
        {
          has_microsoft: false,
          has_google: false,
          authenticated: false,
          error: data.error || 'Session validation failed',
        },
        { status: 401 }
      )
    }

    // Return OAuth provider availability
    return NextResponse.json({
      authenticated: true,
      has_microsoft: data.has_microsoft === true,
      has_google: data.has_google === true,
      providers: data.providers || [],
      user_email: data.user_email,
      user_id: data.user_id,
    })
  } catch (error) {
    console.error('Session check error:', error)
    return NextResponse.json(
      {
        has_microsoft: false,
        has_google: false,
        authenticated: false,
        error: 'Session check failed',
      },
      { status: 500 }
    )
  }
}
