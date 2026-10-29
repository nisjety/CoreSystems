import { NextRequest, NextResponse } from 'next/server';

import { getSessionUser } from '../../../lib/auth-session';

export async function GET(request: NextRequest): Promise<NextResponse> {
  const user = await getSessionUser(request);

  if (!user) {
    return NextResponse.json({ error: 'Unauthorized' }, { status: 401 });
  }

  return NextResponse.json({
    authenticated: true,
    user,
  });
}