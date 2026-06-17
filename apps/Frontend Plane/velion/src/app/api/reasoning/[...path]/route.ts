import { NextRequest, NextResponse } from 'next/server'
import {
    reasoningProxyTarget,
    translateReasoningProxyBody,
    translateReasoningProxyResponse,
} from '@/lib/model-plane/reasoning'

const INTERNAL_API_KEY = (process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET) as string

if (!INTERNAL_API_KEY) {
  throw new Error(
    'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET environment variable is required for inter-service authentication'
  )
}

export async function POST(
    request: NextRequest,
    { params }: { params: Promise<{ path: string[] }> },
) {
    try {
        const { path } = await params
        const apiPath = path.join('/')
        const body = await request.json() as Record<string, unknown>
        const targetUrl = reasoningProxyTarget(apiPath)

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
            body: JSON.stringify(translateReasoningProxyBody(apiPath, body)),
        })

        const translatedResponse = await translateReasoningProxyResponse(apiPath, response, body)
        const responseText = await translatedResponse.text()

        let ObjectData
        try {
            ObjectData = JSON.parse(responseText)
        } catch {
            return new NextResponse(responseText, { status: translatedResponse.status })
        }

        return NextResponse.json(ObjectData, { status: translatedResponse.status })
    } catch (error: any) {
        return NextResponse.json(
            { error: error.message || 'Failed to post to reasoning service' },
            { status: 500 },
        )
    }
}
