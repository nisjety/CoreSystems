import { NextRequest, NextResponse } from 'next/server';

// Get backend URL from environment variable
const getBackendUrl = () => {
  return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
};

async function handler(request: NextRequest) {
  const backendUrl = getBackendUrl();
  const targetUrl = `${backendUrl}/api/v2/auth/getSession`;

  console.log(`🔄 Proxying session request: ${request.method} -> ${targetUrl}`);

  try {
    // Prepare headers for forwarding
    const forwardHeaders = new Headers();
    request.headers.forEach((value, key) => {
      // Skip host header to avoid conflicts
      if (key.toLowerCase() !== 'host') {
        forwardHeaders.set(key, value);
      }
    });

    // Forward the request to the backend
    const response = await fetch(targetUrl, {
      method: 'POST',
      headers: forwardHeaders,
      body:
        request.method !== 'GET' && request.method !== 'HEAD'
          ? await request.arrayBuffer()
          : JSON.stringify({}),
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
        'Access-Control-Allow-Methods': 'GET, POST, PUT, DELETE, OPTIONS',
        'Access-Control-Allow-Headers': 'Content-Type, Authorization',
      },
    });
  } catch (error) {
    console.error(`❌ Failed to proxy session request:`, error);
    return NextResponse.json(
      { success: false, error: 'Internal Server Error' },
      { status: 500 },
    );
  }
}

// Export handlers for all HTTP methods
export { handler as GET, handler as POST, handler as OPTIONS };
