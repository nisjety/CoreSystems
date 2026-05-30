import { NextRequest, NextResponse } from 'next/server';
import { getOptionalInternalApiKey } from '../../_lib/control-plane-auth';

// Get backend URL from environment variable (Docker internal URL)
const getAuthServiceUrl = () => {
  return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
};

async function handler(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  const { path } = await context.params;
  const authPath = path.join('/');
  const backendUrl = getAuthServiceUrl();

  // Preserve any query params for OAuth callback/sign-in flows.
  const searchParams = request.nextUrl.searchParams.toString();
  const queryString = searchParams ? `?${searchParams}` : '';

  // Proxy to Better Auth endpoint.
  const targetUrl = `${backendUrl}/api/auth/${authPath}${queryString}`;

  console.log(
    `🔄 Proxying Better Auth request: ${request.method} ${authPath} -> ${targetUrl}`,
  );

  try {
    // Prepare headers for forwarding
    const forwardHeaders = new Headers();
    request.headers.forEach((value, key) => {
      // Skip host header to avoid conflicts
      if (key.toLowerCase() !== 'host') {
        forwardHeaders.set(key, value);
      }
    });

    const internalApiKey = getOptionalInternalApiKey();
    if (internalApiKey) {
      forwardHeaders.set('x-internal-api-key', internalApiKey);
    }

    // Forward the request to the backend. Use manual redirects to preserve cookies.
    const response = await fetch(targetUrl, {
      method: request.method,
      headers: forwardHeaders,
      body:
        request.method !== 'GET' && request.method !== 'HEAD'
          ? await request.arrayBuffer()
          : undefined,
      redirect: 'manual',
    });

    console.log(`📡 Backend response: ${response.status} ${response.statusText}`);

    // Extra logging for OAuth callback/sign-in debugging.
    if (authPath.includes('callback') || authPath.includes('sign-in/social')) {
      console.log(
        '🔍 Response headers:',
        Array.from(response.headers.entries()).map(([key, value]) =>
          `${key}=${value.substring(0, 60)}${value.length > 60 ? '...' : ''}`,
        ),
      );
      const setCookieHeaders = response.headers.getSetCookie?.() || [];
      if (setCookieHeaders.length > 0) {
        console.log(`🍪 Set-Cookie count: ${setCookieHeaders.length}`);
      } else {
        console.log('⚠️  NO Set-Cookie headers in response');
      }
    }

    // Get response data
    const responseData = await response.arrayBuffer();

    const responseHeaders = new Headers(response.headers);

    if (!responseHeaders.has('Access-Control-Allow-Origin')) {
      const origin = request.headers.get('origin') || 'https://tools.aquatiq.com';
      responseHeaders.set('Access-Control-Allow-Origin', origin);
    }
    if (!responseHeaders.has('Access-Control-Allow-Credentials')) {
      responseHeaders.set('Access-Control-Allow-Credentials', 'true');
    }
    if (!responseHeaders.has('Access-Control-Allow-Methods')) {
      responseHeaders.set('Access-Control-Allow-Methods', 'GET, POST, PUT, DELETE, OPTIONS');
    }
    if (!responseHeaders.has('Access-Control-Allow-Headers')) {
      responseHeaders.set('Access-Control-Allow-Headers', 'Content-Type, Authorization');
    }

    // Better Auth may return 400 when no active session exists during sign-out.
    // Normalize this case to 200 so the frontend can complete logout without noisy errors.
    if (authPath === 'sign-out' && response.status === 400) {
      return NextResponse.json(
        { success: true, message: 'Already signed out' },
        {
          status: 200,
          headers: responseHeaders,
        },
      );
    }

    return new NextResponse(responseData, {
      status: response.status,
      statusText: response.statusText,
      headers: responseHeaders,
    });
  } catch (error) {
    console.error(`❌ Failed to proxy API request ${authPath}:`, error);
    return NextResponse.json(
      { success: false, error: 'Internal Server Error' },
      { status: 500 },
    );
  }
}

// Export handlers for all HTTP methods
export {
  handler as GET,
  handler as POST,
  handler as PUT,
  handler as DELETE,
  handler as PATCH,
  handler as OPTIONS,
};
