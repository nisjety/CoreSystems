import { NextRequest, NextResponse } from 'next/server';

const API_BASE_URL = process.env.NEXT_PUBLIC_FILE_API_URL || 'http://127.0.0.1:8088';

export async function GET(request: NextRequest) {
  const searchParams = request.nextUrl.searchParams;
  const path = searchParams.get('path');
  const width = searchParams.get('width') || '800';
  const height = searchParams.get('height') || '800';

  if (!path) {
    return NextResponse.json({ error: 'Missing path parameter' }, { status: 400 });
  }

  try {
    // Call Go backend thumbnail endpoint
    const backendUrl = `${API_BASE_URL}/api/thumbnail?path=${encodeURIComponent(path)}&width=${width}&height=${height}`;
    
    const response = await fetch(backendUrl);
    
    if (!response.ok) {
      console.error(`Thumbnail fetch failed: ${response.status} ${response.statusText}`);
      return NextResponse.json(
        { error: 'Failed to fetch thumbnail' },
        { status: response.status }
      );
    }

    // Get the image data
    const imageBuffer = await response.arrayBuffer();
    
    // Return the image with proper content type
    return new NextResponse(imageBuffer, {
      status: 200,
      headers: {
        'Content-Type': response.headers.get('Content-Type') || 'image/jpeg',
        'Cache-Control': 'public, max-age=31536000, immutable',
      },
    });
  } catch (error) {
    console.error('Thumbnail proxy error:', error);
    return NextResponse.json(
      { error: 'Internal server error' },
      { status: 500 }
    );
  }
}
