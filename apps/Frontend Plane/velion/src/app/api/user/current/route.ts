import { NextRequest, NextResponse } from 'next/server'
import {
  authErrorResponse,
  buildControlPlaneHeaders,
  getUserServiceUrl,
  readJsonOrNull,
  requireSession,
} from '../../_lib/control-plane-auth'

async function forwardToUserCore(
  request: NextRequest,
  method: 'GET' | 'PATCH',
  body?: unknown,
) {
  const session = await requireSession(request)
  const path = method === 'PATCH' ? '/api/v1/users/me' : '/api/v1/users/current'
  const response = await fetch(`${getUserServiceUrl()}${path}`, {
    method,
    headers: buildControlPlaneHeaders(request, session),
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    cache: 'no-store',
  })

  const payload = await readJsonOrNull(response)
  return NextResponse.json(payload ?? {}, { status: response.status })
}

export async function GET(request: NextRequest) {
  try {
    return await forwardToUserCore(request, 'GET')
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function PATCH(request: NextRequest) {
  try {
    return await forwardToUserCore(request, 'PATCH', await request.json())
  } catch (error) {
    return authErrorResponse(error)
  }
}
