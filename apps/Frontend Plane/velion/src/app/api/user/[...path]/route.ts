import { NextRequest, NextResponse } from 'next/server'
import {
  authErrorResponse,
  buildControlPlaneHeaders,
  getUserServiceUrl,
  readJsonOrNull,
  requireSession,
} from '../../_lib/control-plane-auth'

type Method = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

const buildTargetUrl = (request: NextRequest, pathSegments: string[]) => {
  const userPath = pathSegments.join('/')
  const query = request.nextUrl.searchParams.toString()
  return `${getUserServiceUrl()}/api/v1/${userPath}${query ? `?${query}` : ''}`
}

const responseFromUpstream = async (response: Response) => {
  if (response.status === 204) {
    return new NextResponse(null, { status: 204 })
  }

  const payload = await readJsonOrNull(response)
  return NextResponse.json(payload, { status: response.status })
}

const forwardRequest = async (
  request: NextRequest,
  pathSegments: string[],
  method: Method,
  body?: unknown,
) => {
  const session = await requireSession(request)
  const response = await fetch(buildTargetUrl(request, pathSegments), {
    method,
    headers: buildControlPlaneHeaders(request, session),
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    cache: 'no-store',
  })

  return responseFromUpstream(response)
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    return await forwardRequest(request, path, 'GET')
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    return await forwardRequest(request, path, 'POST', await request.json())
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function PUT(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    return await forwardRequest(request, path, 'PUT', await request.json())
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    return await forwardRequest(request, path, 'PATCH', await request.json())
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function DELETE(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await params
    return await forwardRequest(request, path, 'DELETE')
  } catch (error) {
    return authErrorResponse(error)
  }
}
