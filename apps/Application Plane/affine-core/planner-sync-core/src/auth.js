import { getPlannerDocument } from './documents.js';

const AUTH_SERVICE_URL = process.env.AUTH_SERVICE_URL || 'http://auth-core:3011';
const USER_SERVICE_URL = process.env.USER_SERVICE_URL || 'http://user-core:3012';
const AFFINE_CORE_URL = process.env.AFFINE_CORE_URL || 'http://affine-core:3180';
const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY ||
  process.env.INTERNAL_SERVICE_SECRET ||
  'change-me-internal-service-secret';

export function parsePlannerRequest(request) {
  const url = new URL(request.url || '/', 'http://planner-sync-core.local');

  return {
    room: decodeURIComponent(url.pathname.replace(/^\/+/, '')),
    documentId: url.searchParams.get('documentId') || '',
    workspaceId: url.searchParams.get('workspaceId') || '',
    source: url.searchParams.get('source') || 'planner',
  };
}

function buildInternalHeaders(session) {
  return {
    'Content-Type': 'application/json',
    'X-Internal-Api-Key': INTERNAL_API_KEY,
    'X-User-Id': session.user.id,
    'X-User-Email': session.user.email || '',
    'X-User-Name': session.user.name || '',
  };
}

async function fetchSession(cookieHeader) {
  const response = await fetch(`${AUTH_SERVICE_URL}/api/v2/auth/getSession`, {
    method: 'POST',
    headers: {
      Cookie: cookieHeader,
      'Content-Type': 'application/json',
      'X-Internal-Api-Key': INTERNAL_API_KEY,
    },
    body: JSON.stringify({}),
  });

  if (!response.ok) {
    return null;
  }

  const payload = await response.json();
  if (!payload || payload.authenticated === false) {
    return null;
  }

  return payload.data || payload;
}

async function fetchSessionContext(session) {
  const response = await fetch(`${USER_SERVICE_URL}/api/v1/me/session-context`, {
    method: 'GET',
    headers: buildInternalHeaders(session),
  });

  if (!response.ok) {
    throw new Error(`session-context lookup failed: ${response.status}`);
  }

  return (await response.json()) || null;
}

async function fetchResolvedWorkspace(cookieHeader) {
  const response = await fetch(`${AFFINE_CORE_URL}/api/v1/workspaces/resolve`, {
    method: 'GET',
    headers: {
      'x-internal-api-key': INTERNAL_API_KEY,
      Cookie: cookieHeader,
    },
  });

  if (!response.ok) {
    throw new Error(`workspace resolution failed: ${response.status}`);
  }

  const payload = await response.json();
  return {
    workspaceId: payload.workspace_id || '',
    scope: payload.scope || null,
    orgId: payload.org_id || null,
  };
}

async function authorizeDocumentAccess(session, plannerRequest) {
  const document = await getPlannerDocument(plannerRequest.workspaceId, plannerRequest.documentId);

  if (!document) {
    return {
      ok: false,
      status: 404,
      error: 'planner document not found',
    };
  }

  if (document.archivedAt) {
    return {
      ok: false,
      status: 403,
      error: 'planner document is archived',
    };
  }

  if (
    document.space === 'private' &&
    document.ownerExternalAuthId &&
    document.ownerExternalAuthId !== session.user.id
  ) {
    return {
      ok: false,
      status: 403,
      error: 'document access denied',
    };
  }

  return {
    ok: true,
    document,
  };
}

function authorizeCollaborationRole(sessionContext, resolvedWorkspace, document) {
  const role = sessionContext?.role || null;
  const space = document.space || 'private';

  if (resolvedWorkspace.scope === 'organization') {
    if (!role) {
      return {
        ok: false,
        status: 403,
        error: 'organization role is required',
      };
    }

    if ((space === 'shared' || space === 'collection') && role === 'viewer') {
      return {
        ok: false,
        status: 403,
        error: 'viewer role cannot join collaborative transport',
      };
    }
  }

  return { ok: true };
}

export async function authenticatePlannerRequest(request) {
  const plannerRequest = parsePlannerRequest(request);
  const cookieHeader = request.headers.cookie || '';

  if (!plannerRequest.room || !plannerRequest.workspaceId || !plannerRequest.documentId) {
    return {
      ok: false,
      status: 400,
      error: 'missing planner transport room context',
    };
  }

  if (plannerRequest.room !== `${plannerRequest.workspaceId}::${plannerRequest.documentId}`) {
    return {
      ok: false,
      status: 400,
      error: 'invalid planner transport room',
    };
  }

  if (!cookieHeader) {
    return {
      ok: false,
      status: 401,
      error: 'authentication required',
    };
  }

  try {
    const session = await fetchSession(cookieHeader);
    if (!session?.user?.id) {
      return {
        ok: false,
        status: 401,
        error: 'invalid session',
      };
    }

    const sessionContext = await fetchSessionContext(session);
    const resolvedWorkspace = await fetchResolvedWorkspace(cookieHeader);

    if (!resolvedWorkspace.workspaceId) {
      return {
        ok: false,
        status: 403,
        error: 'no authorized planner workspace found',
      };
    }

    if (resolvedWorkspace.workspaceId !== plannerRequest.workspaceId) {
      return {
        ok: false,
        status: 403,
        error: 'workspace access denied',
      };
    }

    const documentAccess = await authorizeDocumentAccess(session, plannerRequest);
    if (!documentAccess.ok) {
      return documentAccess;
    }

    const collaborationRole = authorizeCollaborationRole(
      sessionContext,
      resolvedWorkspace,
      documentAccess.document,
    );
    if (!collaborationRole.ok) {
      return collaborationRole;
    }

    return {
      ok: true,
      context: {
        room: plannerRequest.room,
        workspaceId: plannerRequest.workspaceId,
        documentId: plannerRequest.documentId,
        source: plannerRequest.source,
        userId: session.user.id,
        userEmail: session.user.email || null,
        userName: session.user.name || null,
        sessionId: session.session?.id || null,
        orgId: sessionContext?.orgId || null,
        role: sessionContext?.role || null,
        workspaceScope: resolvedWorkspace.scope,
        documentSpace: documentAccess.document.space || 'private',
        documentOwnerExternalAuthId: documentAccess.document.ownerExternalAuthId || null,
      },
    };
  } catch (error) {
    return {
      ok: false,
      status: 503,
      error: error instanceof Error ? error.message : 'session validation failed',
    };
  }
}

export function rejectUpgrade(socket, statusCode, message) {
  socket.write(
    `HTTP/1.1 ${statusCode} ${message}\r\n` +
      'Connection: close\r\n' +
      'Content-Type: text/plain\r\n' +
      `Content-Length: ${Buffer.byteLength(message)}\r\n` +
      '\r\n' +
      message,
  );
  socket.destroy();
}
