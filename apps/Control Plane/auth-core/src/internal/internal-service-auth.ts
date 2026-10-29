import { status } from '@grpc/grpc-js';
import { createHash, timingSafeEqual } from 'node:crypto';
import { readFileSync } from 'node:fs';

export const AUTH_INTERNAL_AUDIENCE = 'auth-core-internal' as const;
export const AUTH_INTERNAL_SCOPES = [
  'oauth:token:read',
  'oauth:token:refresh',
  'agent:provision',
  'nats:authenticate',
  'auth:admin',
] as const;

export type AuthInternalScope = (typeof AUTH_INTERNAL_SCOPES)[number];

export type AuthInternalServiceCredential = Readonly<{
  credentialId: string;
  principal: string;
  audience: typeof AUTH_INTERNAL_AUDIENCE;
  token: string;
  scopes: readonly AuthInternalScope[];
}>;

export type AuthInternalCredentialTuple = Readonly<{
  credentialId?: string;
  principal?: string;
  token?: string;
}>;

type CredentialEnvironment = Readonly<{
  NODE_ENV?: string;
  AUTH_INTERNAL_SERVICE_CREDENTIALS?: string;
  AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE?: string;
}>;

const allowedScopes = new Set<string>(AUTH_INTERNAL_SCOPES);
const maximumRegistryBytes = 1024 * 1024;
const placeholderPrefixes = [
  'test',
  'placeholder',
  'change-me',
  'replace-with',
  'your-',
] as const;

export class AuthInternalServiceAuthorizationError extends Error {
  constructor(readonly code: status) {
    super(
      code === status.PERMISSION_DENIED
        ? 'Auth internal service principal lacks the required scope'
        : 'Auth internal service credential is invalid',
    );
  }
}

function exactKeys(candidate: Record<string, unknown>): boolean {
  return (
    Object.keys(candidate).sort().join(',') ===
    'audience,credentialId,principal,scopes,token'
  );
}

function policyIdentifier(value: unknown): string {
  if (typeof value !== 'string' || value !== value.trim()) return '';
  return /^[a-z0-9][a-z0-9._-]{1,127}$/i.test(value) ? value : '';
}

function policyToken(value: unknown): string {
  if (typeof value !== 'string' || value !== value.trim()) return '';
  const normalized = value.toLowerCase();
  if (
    value.length < 32 ||
    placeholderPrefixes.some((prefix) => normalized.startsWith(prefix))
  ) {
    return '';
  }
  return value;
}

function secureEqual(expected: string, received: string): boolean {
  const expectedDigest = createHash('sha256').update(expected).digest();
  const receivedDigest = createHash('sha256').update(received).digest();
  return timingSafeEqual(expectedDigest, receivedDigest);
}

export function parseAuthInternalServiceCredentials(
  raw: string | undefined,
): readonly AuthInternalServiceCredential[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw?.trim() || 'null');
  } catch {
    throw new Error('AUTH_INTERNAL_SERVICE_CREDENTIALS is malformed');
  }
  if (!Array.isArray(parsed) || parsed.length === 0) {
    throw new Error(
      'AUTH_INTERNAL_SERVICE_CREDENTIALS must be a non-empty array',
    );
  }

  const credentialIds = new Set<string>();
  const tokens = new Set<string>();
  const credentials = parsed.map((value): AuthInternalServiceCredential => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
      throw new Error(
        'AUTH_INTERNAL_SERVICE_CREDENTIALS contains an invalid entry',
      );
    }
    const candidate = value as Record<string, unknown>;
    const credentialId = policyIdentifier(candidate.credentialId);
    const principal = policyIdentifier(candidate.principal);
    const token = policyToken(candidate.token);
    const scopes = candidate.scopes;
    if (
      !exactKeys(candidate) ||
      !credentialId ||
      !principal ||
      candidate.audience !== AUTH_INTERNAL_AUDIENCE ||
      !token ||
      !Array.isArray(scopes) ||
      scopes.length === 0 ||
      scopes.some(
        (scope) => typeof scope !== 'string' || !allowedScopes.has(scope),
      ) ||
      new Set(scopes).size !== scopes.length ||
      credentialIds.has(credentialId) ||
      tokens.has(token)
    ) {
      throw new Error(
        'AUTH_INTERNAL_SERVICE_CREDENTIALS contains an invalid principal policy',
      );
    }
    credentialIds.add(credentialId);
    tokens.add(token);
    return Object.freeze({
      credentialId,
      principal,
      audience: AUTH_INTERNAL_AUDIENCE,
      token,
      scopes: Object.freeze([...(scopes as AuthInternalScope[])]),
    });
  });
  return Object.freeze(credentials);
}

export function loadAuthInternalServiceCredentials(
  environment: CredentialEnvironment = process.env as CredentialEnvironment,
): readonly AuthInternalServiceCredential[] {
  const credentialFile =
    environment.AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE?.trim();
  if (credentialFile) {
    let raw: string;
    try {
      raw = readFileSync(credentialFile, 'utf8');
    } catch {
      throw new Error(
        'AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE could not be read',
      );
    }
    if (Buffer.byteLength(raw, 'utf8') > maximumRegistryBytes) {
      throw new Error('AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE is too large');
    }
    return parseAuthInternalServiceCredentials(raw);
  }

  const runtime = environment.NODE_ENV?.trim().toLowerCase() ?? '';
  if (runtime && runtime !== 'development' && runtime !== 'test') {
    throw new Error(
      'AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE is required outside development',
    );
  }
  return parseAuthInternalServiceCredentials(
    environment.AUTH_INTERNAL_SERVICE_CREDENTIALS,
  );
}

export function authorizeAuthInternalService(
  tuple: AuthInternalCredentialTuple,
  credentials: readonly AuthInternalServiceCredential[],
  requiredScope: AuthInternalScope,
): Readonly<{
  credentialId: string;
  principal: string;
  audience: typeof AUTH_INTERNAL_AUDIENCE;
}> {
  const credentialId = tuple.credentialId?.trim() ?? '';
  const principal = tuple.principal?.trim() ?? '';
  const token = tuple.token?.trim() ?? '';
  const credential = credentials.find(
    (candidate) => candidate.credentialId === credentialId,
  );
  const tokenMatches = secureEqual(
    credential?.token ?? 'invalid-auth-internal-service-credential',
    token,
  );
  if (
    !credentialId ||
    tuple.credentialId !== credentialId ||
    !principal ||
    tuple.principal !== principal ||
    !token ||
    tuple.token !== token ||
    !credential ||
    credential.principal !== principal ||
    !tokenMatches
  ) {
    throw new AuthInternalServiceAuthorizationError(status.UNAUTHENTICATED);
  }
  if (!credential.scopes.includes(requiredScope)) {
    throw new AuthInternalServiceAuthorizationError(status.PERMISSION_DENIED);
  }
  return Object.freeze({
    credentialId,
    principal,
    audience: AUTH_INTERNAL_AUDIENCE,
  });
}

// Token-only compatibility is limited to the existing oRPC adapter header.
// The token still resolves to exactly one deployment-owned principal and scope;
// caller-supplied identity headers never participate in the decision.
export function authorizeAuthInternalServiceToken(
  token: string | undefined,
  credentials: readonly AuthInternalServiceCredential[],
  requiredScope: AuthInternalScope,
  requiredPrincipal: string,
): Readonly<{
  credentialId: string;
  principal: string;
  audience: typeof AUTH_INTERNAL_AUDIENCE;
}> {
  const received = token?.trim() ?? '';
  const matching = credentials.find((credential) =>
    secureEqual(credential.token, received),
  );
  if (
    !received ||
    token !== received ||
    !matching ||
    matching.principal !== requiredPrincipal
  ) {
    throw new AuthInternalServiceAuthorizationError(status.UNAUTHENTICATED);
  }
  if (!matching.scopes.includes(requiredScope)) {
    throw new AuthInternalServiceAuthorizationError(status.PERMISSION_DENIED);
  }
  return Object.freeze({
    credentialId: matching.credentialId,
    principal: matching.principal,
    audience: AUTH_INTERNAL_AUDIENCE,
  });
}
