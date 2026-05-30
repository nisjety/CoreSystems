import { NextResponse } from 'next/server';

const AFFINE_CORE_URL = process.env.AFFINE_CORE_URL ?? 'http://affine-core:3180';

export async function GET(request: Request) {
  try {
    const upstream = await fetch(`${AFFINE_CORE_URL}/api/v1/workspaces/resolve`, {
      method: 'GET',
      headers: {
        'x-internal-api-key': process.env.INTERNAL_API_KEY ?? '',
        cookie: request.headers.get('cookie') ?? '',
      },
      cache: 'no-store',
    });

    if (upstream.status === 401) {
      return NextResponse.json({ error: 'Unauthorised' }, { status: 401 });
    }

    if (!upstream.ok) {
      throw new Error(`affine-core workspace resolve failed: ${upstream.status}`);
    }

    const payload = (await upstream.json()) as {
      workspace_id?: string;
      org_id?: string | null;
      created_by?: string | null;
      scope?: 'organization' | 'personal' | 'anonymous';
    };

    return NextResponse.json({
      workspaceId: payload.workspace_id ?? 'planner-anonymous',
      orgId: payload.org_id ?? null,
      createdBy: payload.created_by ?? null,
      scope: payload.scope ?? (payload.org_id ? 'organization' : 'anonymous'),
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : 'Failed to resolve workspace';
    return NextResponse.json({ error: message }, { status: 502 });
  }
}
