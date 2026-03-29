import { NextRequest, NextResponse } from 'next/server';

// Get backend URL from environment variable (Docker internal URL)
const getAuthServiceUrl = () => {
  return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
};

export async function POST(request: NextRequest) {
  const backendUrl = getAuthServiceUrl();
  const targetUrl = `${backendUrl}/api/v2/auth/consent/update`;

  console.log(`🔄 Proxying consent update request: POST -> ${targetUrl}`);

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
    forwardHeaders.set(
      'x-internal-api-key',
      process.env.INTERNAL_API_KEY || 'dev-super-secret-internal-api-key',
    );

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

    return new NextResponse(responseData, {
      status: response.status,
      statusText: response.statusText,
      headers: {
        ...Object.fromEntries(response.headers.entries()),
        // Ensure CORS headers are set
        'Access-Control-Allow-Origin': '*',
        'Access-Control-Allow-Methods': 'POST, OPTIONS',
        'Access-Control-Allow-Headers': 'Content-Type, Authorization',
      },
    });
  } catch (error) {
    console.error('❌ Consent update proxy error:', error);
    return NextResponse.json(
      {
        error: 'Consent update proxy failed',
        message: error instanceof Error ? error.message : 'Unknown error',
      },
      { status: 500 },
    );
  }
}

export async function OPTIONS() {
  return new NextResponse(null, {
    status: 204,
    headers: {
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'POST, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type, Authorization',
    },
  });
}
