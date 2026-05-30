/**
 * Control-plane auth-core session verifier.
 *
 * Calls `POST /internal/sessions/verify` on auth-core with the Bearer token
 * extracted from the incoming request.  Returns a strongly-typed principal
 * object on success; throws HttpError(401) on invalid/expired sessions.
 */
import { HttpError } from '../http/http-error';

// ─── Types ────────────────────────────────────────────────────────────────────

export interface AuthPrincipal {
  /** auth-core session / user id */
  userId: string;
  /** Active organization the session is scoped to */
  organizationId: string;
  /** Active workspace the session is scoped to */
  workspaceId: string;
  /** Role within the organization, e.g. "admin" | "member" */
  role: string;
  /** Email address of the authenticated user */
  email: string;
}

/** Resolved once, at startup; avoids per-request construction overhead. */
export interface AuthClient {
  /**
   * Verify a Bearer token against auth-core and return the principal.
   * Throws HttpError(401) for invalid, expired, or revoked tokens.
   * Throws HttpError(503) when auth-core is unreachable.
   */
  verifyToken(token: string): Promise<AuthPrincipal>;
}

// ─── auth-core response schema ────────────────────────────────────────────────

interface AuthCoreVerifyOkBody {
  success: true;
  data: {
    userId: string;
    organizationId: string;
    workspaceId: string;
    role: string;
    email: string;
    [key: string]: unknown;
  };
}

interface AuthCoreVerifyErrorBody {
  success: false;
  error: { code: string; message: string };
}

type AuthCoreVerifyBody = AuthCoreVerifyOkBody | AuthCoreVerifyErrorBody;

// ─── Implementation ───────────────────────────────────────────────────────────

export class HttpAuthClient implements AuthClient {
  private readonly verifyUrl: string;

  constructor(
    private readonly authCoreBaseUrl: string,
    /** Forwarded as `x-internal-api-key` so auth-core accepts the inbound call */
    private readonly internalApiKey: string,
    private readonly timeoutMs: number = 5_000
  ) {
    this.verifyUrl = `${authCoreBaseUrl.replace(/\/$/, '')}/internal/sessions/verify`;
  }

  async verifyToken(token: string): Promise<AuthPrincipal> {
    let response: Response;

    try {
      response = await fetch(this.verifyUrl, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          // auth-core accepts its own internal key so we can make machine calls
          'x-internal-api-key': this.internalApiKey
        },
        body: JSON.stringify({ token }),
        signal: AbortSignal.timeout(this.timeoutMs)
      });
    } catch (cause) {
      throw new HttpError(503, 'auth_core_unreachable', 'Unable to reach auth-core for token verification', {
        cause: cause instanceof Error ? cause.message : String(cause)
      });
    }

    let body: AuthCoreVerifyBody;

    try {
      body = (await response.json()) as AuthCoreVerifyBody;
    } catch {
      throw new HttpError(503, 'auth_core_bad_response', 'auth-core returned a non-JSON response');
    }

    if (response.status === 401 || response.status === 403 || !response.ok) {
      const msg = body.success === false ? body.error.message : 'Token verification failed';
      throw new HttpError(401, 'unauthorized', msg);
    }

    if (!body.success) {
      throw new HttpError(401, 'unauthorized', body.error.message);
    }

    const { userId, organizationId, workspaceId, role, email } = body.data;

    if (!userId || !organizationId) {
      throw new HttpError(503, 'auth_core_incomplete_response', 'auth-core response is missing required principal fields');
    }

    return {
      userId,
      organizationId,
      workspaceId: workspaceId ?? '',
      role: role ?? 'member',
      email: email ?? ''
    };
  }
}
