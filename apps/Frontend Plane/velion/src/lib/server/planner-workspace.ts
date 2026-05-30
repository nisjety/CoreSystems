import { cookies } from 'next/headers';

const AFFINE_CORE_URL = process.env.AFFINE_CORE_URL ?? 'http://affine-core:3180';
const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY ??
  process.env.INTERNAL_SERVICE_SECRET ??
  '';

export interface PlannerWorkspaceResolution {
  workspaceId: string;
  orgId: string | null;
  createdBy: string | null;
  scope: 'organization' | 'personal' | 'anonymous';
}

export async function resolvePlannerWorkspace(): Promise<PlannerWorkspaceResolution | null> {
  const cookieHeader = (await cookies()).toString();

  const upstream = await fetch(`${AFFINE_CORE_URL}/api/v1/workspaces/resolve`, {
    method: 'GET',
    headers: {
      'x-internal-api-key': INTERNAL_API_KEY,
      cookie: cookieHeader,
    },
    cache: 'no-store',
  });

  if (upstream.status === 401) {
    return null;
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

  return {
    workspaceId: payload.workspace_id ?? 'planner-anonymous',
    orgId: payload.org_id ?? null,
    createdBy: payload.created_by ?? null,
    scope: payload.scope ?? (payload.org_id ? 'organization' : 'anonymous'),
  };
}
