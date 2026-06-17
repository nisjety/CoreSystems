import { NextRequest, NextResponse } from 'next/server'
import { authErrorResponse, requireSession } from '../../../_lib/control-plane-auth'

export async function GET(request: NextRequest) {
  try {
    const session = await requireSession(request)
    const name = session.user.name ?? ''

    return NextResponse.json({
      id: session.user.id,
      name,
      email: session.user.email,
      avatar: session.user.image || null,
      profile: {
        displayName: name,
        givenName: name.split(' ')[0],
        surname: name.split(' ').slice(1).join(' '),
        userPrincipalName: session.user.email,
      },
    })
  } catch (error) {
    return authErrorResponse(error)
  }
}
