import { NextRequest, NextResponse } from 'next/server';

const getAuthServiceUrl = () => {
  return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
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
      process.env.INTERNAL_API_KEY || 'dev-super-secret-internal-api-key',
    );
    forwardHeaders.set('Content-Type', 'application/json');

    const authResponse = await fetch(authUrl, {
      method: 'POST',
      headers: forwardHeaders,
      body: JSON.stringify({}),
      credentials: 'include',
    });

    if (!authResponse.ok) {
      return NextResponse.json(
        { error: 'Unauthorized' },
        { status: 401 }
      );
    }

    const session = await authResponse.json();

    if (!session?.user) {
      return NextResponse.json(
        { error: 'Unauthorized' },
        { status: 401 }
      );
    }

    // For now, return a basic profile structure from the auth session
    // In the future, this could be enriched with Microsoft Graph API data
    const profile = {
      id: session.user.id,
      name: session.user.name,
      email: session.user.email,
      avatar: session.user.image || null,
      // Add any additional profile fields from the session
      profile: {
        displayName: session.user.name,
        givenName: session.user.name?.split(' ')[0],
        surname: session.user.name?.split(' ').slice(1).join(' '),
        userPrincipalName: session.user.email,
      }
    };

    return NextResponse.json(profile);
  } catch (error) {
    console.error('[API] Error fetching graph profile:', error);
    return NextResponse.json(
      { error: 'Internal server error' },
      { status: 500 }
    );
  }
}
