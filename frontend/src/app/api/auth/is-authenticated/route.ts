import { NextRequest, NextResponse } from 'next/server';

// Get backend URL from environment variable
const getBackendUrl = () => {
  return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
};

async function handler(request: NextRequest) {
  const backendUrl = getBackendUrl();
  const targetUrl = `${backendUrl}/api/v2/auth/getSession`;

  console.log(
    `🔄 Proxying auth status request: ${request.method} -> ${targetUrl}`,
  );

  try {
    // Forward only cookies and minimal headers required to validate session
    const forwardHeaders = new Headers();
    const cookieHeader = request.headers.get('cookie');
    if (cookieHeader) forwardHeaders.set('cookie', cookieHeader);
    forwardHeaders.set('content-type', 'application/json');

    const response = await fetch(targetUrl, {
      method: 'POST',
      headers: forwardHeaders,
      body: JSON.stringify({}),
      cache: 'no-store',
    });

    if (!response.ok) {
      return NextResponse.json({ authenticated: false }, { status: 200 });
    }

    const data = await response.json().catch(() => null);
    // Better Auth typically returns { data: { user, session } } or null
    const payload = data && 'data' in data ? data.data : data;
    const isAuthed = !!payload?.user?.id;

    return NextResponse.json({ authenticated: isAuthed }, { status: 200 });
  } catch (error) {
    console.error('❌ Failed to determine auth status:', error);
    return NextResponse.json({ authenticated: false }, { status: 200 });
  }
}

export { handler as GET };
