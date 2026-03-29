import { NextRequest, NextResponse } from 'next/server'

const USER_SERVICE_URL = process.env.USER_SERVICE_URL || 'http://localhost:3012'
const AUTH_SERVICE_URL = process.env.AUTH_SERVICE_URL || 'http://localhost:3011'
const INTERNAL_API_KEYS = Array.from(
  new Set(
    [
      process.env.INTERNAL_API_KEY,
      process.env.INTERNAL_SERVICE_SECRET,
      'change-me-internal-service-secret',
      'internal-dev-key-change-in-production',
      'dev-super-secret-internal-api-key',
    ].filter((value): value is string => Boolean(value && value.trim()))
  )
)

const getAuthBaseUrls = () => {
  const urls = [AUTH_SERVICE_URL]
  if (!urls.includes('http://localhost:3011')) {
    urls.push('http://localhost:3011')
  }
  if (!urls.includes('http://auth-service:3011')) {
    urls.push('http://auth-service:3011')
  }
  return urls
}

const fetchSession = async (request: NextRequest) => {
  const authHeaders = new Headers()

  request.headers.forEach((value, key) => {
    if (key.toLowerCase() !== 'host') {
      authHeaders.set(key, value)
    }
  })

  authHeaders.set(
    'X-Internal-Api-Key',
    INTERNAL_API_KEYS[0],
  )
  authHeaders.set('Content-Type', 'application/json')

  for (const baseUrl of getAuthBaseUrls()) {
    try {
      const authResponse = await fetch(`${baseUrl}/api/v2/auth/getSession`, {
        method: 'POST',
        headers: authHeaders,
        body: JSON.stringify({}),
        credentials: 'include',
      })

      if (authResponse.ok) {
        return await authResponse.json()
      }
    } catch {
      // Try next auth URL fallback
    }
  }

  return null
}

const buildTargetUrl = (request: NextRequest, pathSegments: string[]) => {
  const userPath = pathSegments.join('/')
  const searchParams = request.nextUrl.searchParams.toString()
  const queryString = searchParams ? `?${searchParams}` : ''

  // Prepend /api/v1 to match user service routes
  return `${USER_SERVICE_URL}/api/v1/${userPath}${queryString}`
}

const resolveUserIdFromSession = async (request: NextRequest) => {
  const session = await fetchSession(request)
  return session?.user?.id ?? null
}

const buildHeaders = async (request: NextRequest, internalApiKey: string) => {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    'X-Internal-Api-Key': internalApiKey,
  }

  const authorization = request.headers.get('authorization')
  if (authorization) {
    headers.authorization = authorization
  }

  // Prefer forwarded user id from client, fallback to server-side session lookup
  const forwardedUserId = request.headers.get('x-user-id')?.trim()
  const userId = forwardedUserId || (await resolveUserIdFromSession(request))
  if (userId) {
    headers['X-User-Id'] = userId
    
    // Also try to get email and name from session for auto-provisioning
    try {
      const forwardedUserEmail = request.headers.get('x-user-email')?.trim()
      const forwardedUserName = request.headers.get('x-user-name')?.trim()
      const forwardedUserAvatar = request.headers.get('x-user-avatar')?.trim()

      if (forwardedUserEmail) {
        headers['X-User-Email'] = forwardedUserEmail
      }
      if (forwardedUserName) {
        headers['X-User-Name'] = forwardedUserName
      }
      if (forwardedUserAvatar) {
        headers['X-User-Avatar'] = forwardedUserAvatar
      }

      const session = await fetchSession(request)
      if (session) {
        if (!headers['X-User-Email'] && session?.user?.email) {
          headers['X-User-Email'] = session.user.email
        }
        if (!headers['X-User-Name'] && session?.user?.name) {
          headers['X-User-Name'] = session.user.name
        }
        if (!headers['X-User-Avatar'] && session?.user?.image) {
          headers['X-User-Avatar'] = session.user.image
        }
      }

      // Fallback to email query param when session is not yet available during callback timing
      if (!headers['X-User-Email']) {
        const emailFromQuery = request.nextUrl.searchParams.get('email')
        if (emailFromQuery) {
          headers['X-User-Email'] = emailFromQuery
        }
      }
    } catch (e) {
      // Ignore errors, user-core will work with just ID
    }
  }

  return headers
}

const parseResponsePayload = async (response: Response) => {
  const raw = await response.text()
  if (!raw) {
    return null
  }
  try {
    return JSON.parse(raw)
  } catch {
    return { raw }
  }
}

const forwardRequest = async (
  request: NextRequest,
  targetUrl: string,
  method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE',
  body?: unknown,
) => {
  let finalResponse: Response | null = null

  for (const key of INTERNAL_API_KEYS) {
    const headers = await buildHeaders(request, key)
    const response = await fetch(targetUrl, {
      method,
      headers,
      ...(body !== undefined ? { body: JSON.stringify(body) } : {}),
    })

    finalResponse = response
    if (response.status !== 401) {
      break
    }
  }

  if (!finalResponse) {
    throw new Error('No response from user service')
  }

  const data = await parseResponsePayload(finalResponse)
  return NextResponse.json(data, { status: finalResponse.status })
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    const targetUrl = buildTargetUrl(request, path)
    return await forwardRequest(request, targetUrl, 'GET')
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to fetch from user service' },
      { status: 500 },
    )
  }
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    const targetUrl = buildTargetUrl(request, path)
    const body = await request.json()
    return await forwardRequest(request, targetUrl, 'POST', body)
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to post to user service' },
      { status: 500 },
    )
  }
}

export async function PUT(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    const targetUrl = buildTargetUrl(request, path)
    const body = await request.json()
    return await forwardRequest(request, targetUrl, 'PUT', body)
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to update user service' },
      { status: 500 },
    )
  }
}

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    const targetUrl = buildTargetUrl(request, path)
    const body = await request.json()
    return await forwardRequest(request, targetUrl, 'PATCH', body)
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to patch user service' },
      { status: 500 },
    )
  }
}

export async function DELETE(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    const targetUrl = buildTargetUrl(request, path)
    return await forwardRequest(request, targetUrl, 'DELETE')
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to delete from user service' },
      { status: 500 },
    )
  }
}
