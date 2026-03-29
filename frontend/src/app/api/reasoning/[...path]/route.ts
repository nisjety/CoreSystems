import { NextRequest, NextResponse } from 'next/server'

const REASONING_CORE_URL = process.env.REASONING_CORE_URL || 'http://localhost:8101'
const INTERNAL_API_KEY = process.env.INTERNAL_API_KEY || 'internal-dev-key-change-in-production'

export async function POST(
    request: NextRequest,
    { params }: { params: Promise<{ path: string[] }> },
) {
    try {
        const { path } = await params
        const apiPath = path.join('/')
        const targetUrl = `${REASONING_CORE_URL}/api/v1/${apiPath}`
        const body = await request.json()

        const headers: Record<string, string> = {
            'Content-Type': 'application/json',
            'X-Internal-Api-Key': INTERNAL_API_KEY,
        }

        const authorization = request.headers.get('authorization')
        if (authorization) {
            headers.authorization = authorization
        }

        // Pass user ID if provided by client or auth session
        const userId = request.headers.get('x-user-id')
        if (userId) {
            headers['X-User-Id'] = userId
        }

        const response = await fetch(targetUrl, {
            method: 'POST',
            headers,
            body: JSON.stringify(body),
        })

        const responseText = await response.text()

        let ObjectData
        try {
            ObjectData = JSON.parse(responseText)
        } catch {
            return new NextResponse(responseText, { status: response.status })
        }

        return NextResponse.json(ObjectData, { status: response.status })
    } catch (error: any) {
        return NextResponse.json(
            { error: error.message || 'Failed to post to reasoning service' },
            { status: 500 },
        )
    }
}
