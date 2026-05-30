import { NextRequest, NextResponse } from 'next/server'

// OAuth2 client management — proxied to the auth service
const AUTH_SERVICE_URL = process.env.AUTH_SERVICE_URL || 'http://auth-service:3011'
const INTERNAL_API_KEY = (process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET) as string

if (!INTERNAL_API_KEY) {
  throw new Error(
    'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET environment variable is required for inter-service authentication'
  )
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  return proxyRequest(request, await params, 'GET')
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  return proxyRequest(request, await params, 'POST')
}

export async function PUT(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  return proxyRequest(request, await params, 'PUT')
}

export async function DELETE(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  return proxyRequest(request, await params, 'DELETE')
}

async function proxyRequest(
  request: NextRequest,
  params: { path: string[] },
  method: string,
) {
  try {
    const apiPath = params.path.join('/')
    const searchParams = request.nextUrl.searchParams.toString()
    const targetUrl = `${AUTH_SERVICE_URL}/api/oauth2/${apiPath}${searchParams ? `?${searchParams}` : ''}`

    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      'X-Internal-Api-Key': INTERNAL_API_KEY,
    }

    const authorization = request.headers.get('authorization')
    if (authorization) headers['authorization'] = authorization

    const cookie = request.headers.get('cookie')
    if (cookie) headers['cookie'] = cookie

    const fetchOptions: RequestInit = { method, headers }
    if (method !== 'GET' && method !== 'DELETE') {
      try {
        const body = await request.json()
        fetchOptions.body = JSON.stringify(body)
      } catch {
        // No body
      }
    }

    const res = await fetch(targetUrl, fetchOptions)
    const text = await res.text()
    let data: unknown
    try {
      data = JSON.parse(text)
    } catch {
      data = { message: text }
    }
    return NextResponse.json(data, { status: res.status })
  } catch (error: any) {
    return NextResponse.json(
      { error: error.message || 'OAuth2 proxy error' },
      { status: 502 },
    )
  }
}
