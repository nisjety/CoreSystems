import { NextRequest, NextResponse } from 'next/server'
import {
  authErrorResponse,
  buildControlPlaneHeaders,
  getUserServiceUrl,
  readJsonOrNull,
  requireSession,
} from '../../../_lib/control-plane-auth'

export async function POST(request: NextRequest) {
  try {
    const session = await requireSession(request)
    const response = await fetch(
      `${getUserServiceUrl()}/api/v1/users/onboarding/complete`,
      {
        method: 'POST',
        headers: buildControlPlaneHeaders(request, session),
        body: JSON.stringify({}),
        cache: 'no-store',
      },
    )

    const payload = await readJsonOrNull(response)
    if (!response.ok) {
      return NextResponse.json(
        payload ?? {
          error: {
            code: 'onboarding_complete_failed',
            message: 'Failed to mark onboarding complete',
          },
        },
        { status: response.status },
      )
    }

    return NextResponse.json(payload ?? { success: true }, { status: response.status })
  } catch (error) {
    return authErrorResponse(error)
  }
}
