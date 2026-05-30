import { NextRequest, NextResponse } from 'next/server';

const getAuthServiceUrl = () => {
	return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
};

async function handler(request: NextRequest) {
	const backendUrl = getAuthServiceUrl();
	const searchParams = request.nextUrl.searchParams.toString();
	const queryString = searchParams ? `?${searchParams}` : '';

	const targetUrl = `${backendUrl}/api/auth/sign-in/oauth${queryString}`;

	try {
		const forwardHeaders = new Headers();
		request.headers.forEach((value, key) => {
			if (key.toLowerCase() !== 'host') {
				forwardHeaders.set(key, value);
			}
		});

		const response = await fetch(targetUrl, {
			method: request.method,
			headers: forwardHeaders,
			body:
				request.method !== 'GET' && request.method !== 'HEAD'
					? await request.arrayBuffer()
					: undefined,
			redirect: 'manual',
		});

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

		return new NextResponse(responseData, {
			status: response.status,
			statusText: response.statusText,
			headers: responseHeaders,
		});
	} catch (error) {
		console.error('❌ Failed to proxy sign-in/oauth request:', error);
		return NextResponse.json(
			{ success: false, error: 'Internal Server Error' },
			{ status: 500 },
		);
	}
}

export { handler as GET, handler as POST, handler as PUT, handler as DELETE, handler as PATCH, handler as OPTIONS };
