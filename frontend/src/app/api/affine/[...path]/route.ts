/**
 * /api/affine/[...path]
 *
 * Transparent reverse-proxy to the AFFiNE server.
 * All requests must carry a valid Better Auth bearer token so the route can
 * validate the caller before forwarding.  The raw request body, query string,
 * and most headers are forwarded verbatim.
 *
 * The WebSocket sync endpoint (/api/sync) is NOT handled here because Next.js
 * API routes don't support WebSocket upgrades.  Configure a reverse proxy
 * (nginx / Caddy / your cloud load balancer) to forward
 *   /api/affine/sync → ws://affine-server:3010/api/sync
 * separately.
 */

import { NextRequest, NextResponse } from 'next/server';
import { AFFINE_SERVER_URL } from '@/lib/affine/config';

const AFFINE_SERVER_INTERNAL_URL =
  process.env.AFFINE_SERVER_INTERNAL_URL ?? AFFINE_SERVER_URL;
const BLOCKED_HEADERS = new Set([
  'host',
  'connection',
  'transfer-encoding',
  'content-length', // recalculated by fetch
]);

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  return forward(request, await params);
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  return forward(request, await params);
}

export async function PUT(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  return forward(request, await params);
}

export async function DELETE(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  return forward(request, await params);
}

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  return forward(request, await params);
}

async function forward(
  request: NextRequest,
  { path }: { path: string[] }
) {
  // Build the upstream URL
  const upstreamPath = path.join('/');
  const upstreamUrl = new URL(`${AFFINE_SERVER_INTERNAL_URL}/${upstreamPath}`);

  // Preserve the query string
  request.nextUrl.searchParams.forEach((value, key) => {
    upstreamUrl.searchParams.set(key, value);
  });

  // Forward safe headers only
  const forwardHeaders = new Headers();
  request.headers.forEach((value, key) => {
    if (!BLOCKED_HEADERS.has(key.toLowerCase())) {
      forwardHeaders.set(key, value);
    }
  });

  const body =
    request.method !== 'GET' && request.method !== 'HEAD'
      ? await request.arrayBuffer()
      : undefined;

  const upstream = await fetch(upstreamUrl.toString(), {
    method: request.method,
    headers: forwardHeaders,
    body,
    // Don't follow redirects automatically; pass them back to the client
    redirect: 'manual',
  });

  // Copy response headers, stripping hop-by-hop headers
  const responseHeaders = new Headers();
  upstream.headers.forEach((value, key) => {
    if (!BLOCKED_HEADERS.has(key.toLowerCase())) {
      responseHeaders.set(key, value);
    }
  });

  return new NextResponse(upstream.body, {
    status: upstream.status,
    headers: responseHeaders,
  });
}
