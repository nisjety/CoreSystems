import { NextRequest, NextResponse } from 'next/server';

// Get backend URL from environment variable
const getBackendUrl = () => {
  return process.env.BACKEND_URL || 'http://auth-service:3011';
};

async function handler(request: NextRequest, context: { params: Promise<{ path: string[] }> }) {
  const { path } = await context.params;
  const pathString = path.join('/');
  const backendUrl = getBackendUrl();
  const targetUrl = `${backendUrl}/.well-known/${pathString}`;

  console.log(`Proxying well-known request: ${request.method} ${pathString} -> ${targetUrl}`);

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
      method: request.method,
      headers: forwardHeaders,
      body: request.method !== 'GET' && request.method !== 'HEAD' ? await request.arrayBuffer() : undefined,
    });

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
    console.error(`Failed to proxy well-known request ${pathString}:`, error);
    return new NextResponse('Internal Server Error', { status: 500 });
  }
}

// Export handlers for all HTTP methods
export { handler as GET, handler as POST, handler as PUT, handler as DELETE, handler as PATCH, handler as OPTIONS };
