import { NextRequest, NextResponse } from 'next/server';

const getAuthServiceUrl = () => {
  return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
};

const getInternalApiKey = () => {
  return (
    process.env.INTERNAL_API_KEY ||
    process.env.INTERNAL_SERVICE_SECRET ||
    'change-me-internal-service-secret'
  );
};

export async function GET(request: NextRequest) {
  try {
    // Call auth service directly with cookies
    const authUrl = `${getAuthServiceUrl()}/api/v2/auth/getSession`;
    
    const forwardHeaders = new Headers();
    request.headers.forEach((value, key) => {
      if (key.toLowerCase() !== 'host') {
        forwardHeaders.set(key, value);
      }
    });
    
    forwardHeaders.set(
      'x-internal-api-key',
      getInternalApiKey(),
    );
    forwardHeaders.set('Content-Type', 'application/json');

    const authResponse = await fetch(authUrl, {
      method: 'POST',
      headers: forwardHeaders,
      body: JSON.stringify({}),
      credentials: 'include',
    });

    console.log(`[/api/user/current] Auth service response: ${authResponse.status}`);

    if (!authResponse.ok) {
      const errorText = await authResponse.text();
      console.error(`[/api/user/current] Auth error: ${errorText}`);
      return NextResponse.json(
        { error: 'Unauthorized' },
        { status: 401 }
      );
    }

    const session = await authResponse.json();
    console.log(`[/api/user/current] Session:`, JSON.stringify(session).substring(0, 200));

    if (!session?.user) {
      console.error(`[/api/user/current] No user in session`);
      return NextResponse.json(
        { error: 'Unauthorized' },
        { status: 401 }
      );
    }

    // Return the user data from the auth session
    // This includes all the user data that Better Auth has
    const user = {
      id: session.user.id,
      email: session.user.email,
      name: session.user.name,
      avatar: session.user.image || null,
      emailVerified: session.user.emailVerified,
      createdAt: session.user.createdAt,
      updatedAt: session.user.updatedAt,
    };

    return NextResponse.json(user);
  } catch (error) {
    console.error('[API] Error fetching current user:', error);
    return NextResponse.json(
      { error: 'Internal server error' },
      { status: 500 }
    );
  }
}
