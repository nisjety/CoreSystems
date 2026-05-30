import { NextRequest, NextResponse } from 'next/server';
import { getOptionalInternalApiKey } from '../../../_lib/control-plane-auth';

// Get backend URL from environment variable (Docker internal URL)
const getAuthServiceUrl = () => {
  return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
};

export async function POST(request: NextRequest) {
  const backendUrl = getAuthServiceUrl();
  const targetUrl = `${backendUrl}/api/v2/auth/consent/get`;

  console.log(`🔄 Proxying consent get request: POST -> ${targetUrl}`);

  try {
    // Prepare headers for forwarding
    const forwardHeaders = new Headers();
    request.headers.forEach((value, key) => {
      // Skip host header to avoid conflicts
      if (key.toLowerCase() !== 'host') {
        forwardHeaders.set(key, value);
      }
    });

    // Add internal API key for service-to-service authentication
    const internalApiKey = getOptionalInternalApiKey();
    if (internalApiKey) {
      forwardHeaders.set('x-internal-api-key', internalApiKey);
    }

    // Get request body
    const body = await request.arrayBuffer();

    // Forward the request to the backend
    const response = await fetch(targetUrl, {
      method: 'POST',
      headers: forwardHeaders,
      body: body,
    });

    console.log(`📡 Backend response: ${response.status} ${response.statusText}`);

    // Get response data
    const responseData = await response.arrayBuffer();

    const responseHeaders = new Headers(response.headers);
    const origin = request.headers.get('origin');
    if (origin) {
      responseHeaders.set('Access-Control-Allow-Origin', origin);
      responseHeaders.set('Vary', 'Origin');
    }
    responseHeaders.set('Access-Control-Allow-Methods', 'POST, OPTIONS');
    responseHeaders.set('Access-Control-Allow-Headers', 'Content-Type, Authorization');

    return new NextResponse(responseData, {
      status: response.status,
      statusText: response.statusText,
      headers: responseHeaders,
    });
  } catch (error) {
    console.error('❌ Consent get proxy error:', error);
    return NextResponse.json(
      {
        error: 'Consent get proxy failed',
        message: error instanceof Error ? error.message : 'Unknown error',
      },
      { status: 500 },
    );
  }
}

export async function OPTIONS(request: NextRequest) {
  const headers = new Headers({
    'Access-Control-Allow-Methods': 'POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization',
  });
  const origin = request.headers.get('origin');
  if (origin) {
    headers.set('Access-Control-Allow-Origin', origin);
    headers.set('Vary', 'Origin');
  }
  return new NextResponse(null, {
    status: 204,
    headers,
  });
}
