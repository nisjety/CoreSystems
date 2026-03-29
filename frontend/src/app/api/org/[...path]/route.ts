import { NextRequest, NextResponse } from 'next/server'

const ORG_SERVICE_URL = process.env.ORG_SERVICE_URL || 'http://localhost:8090'
const AUTH_SERVICE_URL = process.env.AUTH_SERVICE_URL || 'http://localhost:3011'
const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY || 'internal-dev-key-change-in-production'

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
    INTERNAL_API_KEY,
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

const buildTargetUrl = (request: NextRequest, pathSegments: string[] | undefined) => {
  if (!pathSegments || pathSegments.length === 0) {
    return ORG_SERVICE_URL
  }
  const authPath = pathSegments.join('/')
  const searchParams = request.nextUrl.searchParams.toString()
  const queryString = searchParams ? `?${searchParams}` : ''

  return `${ORG_SERVICE_URL}/${authPath}${queryString}`
}

const resolveUserIdFromSession = async (request: NextRequest) => {
  const session = await fetchSession(request)
  return session?.user?.id ?? null
}

const buildHeaders = async (request: NextRequest) => {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    'X-Internal-Api-Key': INTERNAL_API_KEY,
  }

  const authorization = request.headers.get('authorization')
  if (authorization) {
    headers.authorization = authorization
  }

  const forwardedUserId = request.headers.get('x-user-id')?.trim()
  const userId = forwardedUserId || (await resolveUserIdFromSession(request))
  if (userId) {
    headers['X-User-Id'] = userId
  }

  return headers
}

export async function GET(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const params = await context.params
    const path = params.path
    const targetUrl = buildTargetUrl(request, path)
    const headers = await buildHeaders(request)
    
    const response = await fetch(targetUrl, {
      method: 'GET',
      headers,
    })

    const responseText = await response.text()
    
    let data
    try {
      data = JSON.parse(responseText)
    } catch (parseErr) {
      return NextResponse.json(
        { error: 'Invalid JSON from org service', raw: responseText },
        { status: response.status },
      )
    }
    
    return NextResponse.json(data, { status: response.status })
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to fetch from org service' },
      { status: 500 },
    )
  }
}

export async function POST(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await context.params
    const targetUrl = buildTargetUrl(request, path)
    const body = await request.json()
    const headers = await buildHeaders(request)

    const response = await fetch(targetUrl, {
      method: 'POST',
      headers,
      body: JSON.stringify(body),
    })

    const data = await response.json()
    return NextResponse.json(data, { status: response.status })
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to post to org service' },
      { status: 500 },
    )
  }
}

export async function PUT(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await context.params
    const targetUrl = buildTargetUrl(request, path)
    const body = await request.json()
    const headers = await buildHeaders(request)

    const response = await fetch(targetUrl, {
      method: 'PUT',
      headers,
      body: JSON.stringify(body),
    })

    const data = await response.json()
    return NextResponse.json(data, { status: response.status })
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to update org service' },
      { status: 500 },
    )
  }
}

export async function PATCH(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await context.params
    const targetUrl = buildTargetUrl(request, path)
    const body = await request.json()
    const headers = await buildHeaders(request)

    const response = await fetch(targetUrl, {
      method: 'PATCH',
      headers,
      body: JSON.stringify(body),
    })

    const data = await response.json()
    return NextResponse.json(data, { status: response.status })
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to patch org service' },
      { status: 500 },
    )
  }
}

export async function DELETE(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await context.params
    const targetUrl = buildTargetUrl(request, path)
    const headers = await buildHeaders(request)
    const response = await fetch(targetUrl, {
      method: 'DELETE',
      headers,
    })

    const data = await response.json()
    return NextResponse.json(data, { status: response.status })
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'Failed to delete from org service' },
      { status: 500 },
    )
  }
}
