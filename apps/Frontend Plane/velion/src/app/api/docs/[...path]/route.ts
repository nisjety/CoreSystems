import { NextRequest, NextResponse } from 'next/server'

const DOCS_SERVICE_URL = process.env.DOCS_SERVICE_URL || 'http://localhost:8001'
const RETRIEVAL_SERVICE_URL = process.env.RETRIEVAL_SERVICE_URL || 'http://localhost:8004'
const INTERNAL_API_KEY = process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET

if (!INTERNAL_API_KEY) {
  throw new Error(
    'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET environment variable is required for inter-service authentication'
  )
}

const buildHeaders = (request: NextRequest) => {
    const headers: Record<string, string> = {
        'Content-Type': 'application/json',
        'X-Internal-Api-Key': INTERNAL_API_KEY,
    }

    const authorization = request.headers.get('authorization')
    if (authorization) {
        headers.authorization = authorization
    }

    const userId = request.headers.get('x-user-id')
    if (userId) {
        headers['X-User-Id'] = userId
    }

    return headers
}

export async function GET(
    request: NextRequest,
    { params }: { params: Promise<{ path: string[] }> },
) {
    try {
        const { path } = await params
        const apiPath = path.join('/')
        // Routing based on path - if it's retrieval go to 8004, else 8001
        const baseUrl = apiPath.startsWith('retrieve') ? RETRIEVAL_SERVICE_URL : DOCS_SERVICE_URL
        const search = request.nextUrl.search // forward all query params
        const targetUrl = `${baseUrl}/v1/${apiPath}${search}`
        const headers = buildHeaders(request)

        const response = await fetch(targetUrl, { method: 'GET', headers })
        const data = await response.json()
        return NextResponse.json(data, { status: response.status })
    } catch (error: any) {
        return NextResponse.json({ error: error.message }, { status: 500 })
    }
}

export async function PUT(
    request: NextRequest,
    { params }: { params: Promise<{ path: string[] }> },
) {
    try {
        const { path } = await params
        const apiPath = path.join('/')
        const baseUrl = apiPath.startsWith('retrieve') ? RETRIEVAL_SERVICE_URL : DOCS_SERVICE_URL
        const search = request.nextUrl.search
        const targetUrl = `${baseUrl}/v1/${apiPath}${search}`
        const body = await request.json()
        const headers = buildHeaders(request)
        const response = await fetch(targetUrl, { method: 'PUT', headers, body: JSON.stringify(body) })
        const data = await response.json()
        return NextResponse.json(data, { status: response.status })
    } catch (error: any) {
        return NextResponse.json({ error: error.message }, { status: 500 })
    }
}

export async function DELETE(
    request: NextRequest,
    { params }: { params: Promise<{ path: string[] }> },
) {
    try {
        const { path } = await params
        const apiPath = path.join('/')
        const baseUrl = apiPath.startsWith('retrieve') ? RETRIEVAL_SERVICE_URL : DOCS_SERVICE_URL
        const search = request.nextUrl.search
        const targetUrl = `${baseUrl}/v1/${apiPath}${search}`
        const headers = buildHeaders(request)
        const response = await fetch(targetUrl, { method: 'DELETE', headers })
        const data = await response.json().catch(() => ({}))
        return NextResponse.json(data, { status: response.status })
    } catch (error: any) {
        return NextResponse.json({ error: error.message }, { status: 500 })
    }
}
export async function POST(
    request: NextRequest,
    { params }: { params: Promise<{ path: string[] }> },
) {
    try {
        const { path } = await params
        const apiPath = path.join('/')
        const baseUrl = apiPath.startsWith('retrieve') ? RETRIEVAL_SERVICE_URL : DOCS_SERVICE_URL
        const search = request.nextUrl.search
        const targetUrl = `${baseUrl}/v1/${apiPath}${search}`
        const body = await request.json()
        const headers = buildHeaders(request)

        const response = await fetch(targetUrl, {
            method: 'POST',
            headers,
            body: JSON.stringify(body),
        })

        const data = await response.json()
        return NextResponse.json(data, { status: response.status })
    } catch (error: any) {
        return NextResponse.json({ error: error.message }, { status: 500 })
    }
}
