import { NextRequest, NextResponse } from 'next/server';
import { getOptionalInternalApiKey } from '../../_lib/control-plane-auth';

const getAuthServiceUrl = () => {
  return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
};

async function handler(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  const { path } = await context.params;
  const proxyPath = path.join('/');
  const backendUrl = getAuthServiceUrl();
  const searchParams = request.nextUrl.searchParams.toString();
  const queryString = searchParams ? `?${searchParams}` : '';
  const targetUrl = `${backendUrl}/api/convex-auth/${proxyPath}${queryString}`;

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
      cache: 'no-store',
    });

    const responseData = await response.arrayBuffer();
    const responseHeaders = new Headers(response.headers);
    return new NextResponse(responseData, {
      status: response.status,
      statusText: response.statusText,
      headers: responseHeaders,
    });
  } catch (error) {
    console.error(`❌ Failed to proxy Convex auth request ${proxyPath}:`, error);
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
