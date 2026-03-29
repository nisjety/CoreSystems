import { NextRequest, NextResponse } from 'next/server';
import { fetchFromAuthService } from '@/lib/auth/auth-service-url';

export async function GET(request: NextRequest) {
  const allowDevFallback = process.env.NODE_ENV !== 'production';

  try {
    const { response, targetUrl } = await fetchFromAuthService('/api/v2/auth/getSession', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Cookie: request.headers.get('cookie') || '',
      },
      body: JSON.stringify({}),
      retryOn5xx: allowDevFallback,
    });

    console.log(
      `🔄 Proxying Better Auth session request: GET get-session -> POST ${targetUrl}`,
    );

    console.log(`📡 Backend response: ${response.status} ${response.statusText}`);

    if (allowDevFallback && response.status >= 500) {
      return NextResponse.json(null, { status: 200 });
    }

    const responseData = await response.json();

    return NextResponse.json(responseData, {
      status: response.status,
      headers: {
        // Forward important headers
        'Set-Cookie': response.headers.get('set-cookie') || '',
      },
    });
  } catch (error) {
    console.error(`❌ Failed to proxy session request:`, error);

    if (allowDevFallback) {
      return NextResponse.json(null, { status: 200 });
    }

    return NextResponse.json(
      { error: 'Internal Server Error' },
      { status: 500 },
    );
  }
}
