import { NextRequest, NextResponse } from 'next/server';
import { getOptionalInternalApiKey } from '../../../_lib/control-plane-auth';

const getAuthServiceUrl = () =>
  process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';

async function handler(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  const { path } = await context.params;
  // Prepend 'sign-in/' so the full path is correctly forwarded
  const authPath = `sign-in/${path.join('/')}`;
  const backendUrl = getAuthServiceUrl();
  const searchParams = request.nextUrl.searchParams.toString();
  const queryString = searchParams ? `?${searchParams}` : '';
  const targetUrl = `${backendUrl}/api/auth/${authPath}${queryString}`;

  console.log(`🔄 Proxying sign-in request: ${request.method} ${authPath} -> ${targetUrl}`);

  try {
    const forwardHeaders = new Headers();
    request.headers.forEach((value, key) => {
      if (key.toLowerCase() !== 'host') {
        forwardHeaders.set(key, value);
      }
    });
    const internalApiKey = getOptionalInternalApiKey();
    if (internalApiKey) {
      forwardHeaders.set('x-internal-api-key', internalApiKey);
    }

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

    const responseData = await response.arrayBuffer();
    const responseHeaders = new Headers(response.headers);

    if (!responseHeaders.has('Access-Control-Allow-Origin')) {
      const origin = request.headers.get('origin');
      if (origin) {
        responseHeaders.set('Access-Control-Allow-Origin', origin);
        responseHeaders.set('Vary', 'Origin');
      }
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

    return new NextResponse(responseData, {
      status: response.status,
      statusText: response.statusText,
      headers: responseHeaders,
    });
  } catch (error) {
    console.error(`❌ Failed to proxy sign-in request ${authPath}:`, error);
    return NextResponse.json(
      { success: false, error: 'Internal Server Error' },
      { status: 500 },
    );
  }
}

export {
  handler as GET,
  handler as POST,
  handler as PUT,
  handler as DELETE,
  handler as PATCH,
  handler as OPTIONS,
};
