import { status, type Metadata } from '@grpc/grpc-js';
import { RpcException } from '@nestjs/microservices';
import { createHash, timingSafeEqual } from 'node:crypto';
import { readFileSync } from 'node:fs';

export const AUTH_GRPC_AUDIENCE = 'auth-core' as const;

export const AUTH_GRPC_SCOPES = [
  'auth:signup',
  'auth:signin',
  'auth:signout',
  'auth:token:validate',
  'auth:user:read',
] as const;

export type AuthGrpcScope = (typeof AUTH_GRPC_SCOPES)[number];

export type AuthGrpcServiceCredential = Readonly<{
  credentialId: string;
  principal: string;
  audience: typeof AUTH_GRPC_AUDIENCE;
  token: string;
  scopes: readonly AuthGrpcScope[];
}>;

type CredentialEnvironment = Readonly<{
  NODE_ENV?: string;
  AUTH_GRPC_SERVICE_CREDENTIALS?: string;
  AUTH_GRPC_SERVICE_CREDENTIALS_FILE?: string;
}>;

const allowedScopes = new Set<string>(AUTH_GRPC_SCOPES);
const placeholderPrefixes = [
  'test',
  'placeholder',
  'change-me',
  'replace-with',
] as const;
const maximumRegistryBytes = 1024 * 1024;

function isNonPlaceholderToken(token: string): boolean {
  const normalized = token.toLowerCase();
  return (
    token.length >= 32 &&
    !placeholderPrefixes.some((prefix) => normalized.startsWith(prefix))
  );
}

function secureEqual(expected: string, received: string): boolean {
  const expectedDigest = createHash('sha256').update(expected).digest();
  const receivedDigest = createHash('sha256').update(received).digest();
  return timingSafeEqual(expectedDigest, receivedDigest);
}

function exactKeys(candidate: Record<string, unknown>): boolean {
  const keys = Object.keys(candidate).sort();
  return (
    keys.length === 5 &&
    keys[0] === 'audience' &&
    keys[1] === 'credentialId' &&
    keys[2] === 'principal' &&
    keys[3] === 'scopes' &&
    keys[4] === 'token'
  );
}

function policyIdentifier(value: unknown): string {
  if (typeof value !== 'string' || value !== value.trim()) return '';
  return /^[a-z0-9][a-z0-9._-]{1,127}$/i.test(value) ? value : '';
}

function policyToken(value: unknown): string {
  if (typeof value !== 'string' || value !== value.trim()) return '';
  return isNonPlaceholderToken(value) ? value : '';
}

function authorizationFailure(code: status): RpcException {
  const message =
    code === status.PERMISSION_DENIED
      ? 'Auth gRPC service principal lacks the required scope'
      : 'Auth gRPC service credential is invalid';
  return new RpcException({ code, message });
}

export function parseAuthGrpcServiceCredentials(
  raw: string | undefined,
): readonly AuthGrpcServiceCredential[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw?.trim() || 'null');
  } catch {
    throw new Error('AUTH_GRPC_SERVICE_CREDENTIALS is malformed');
  }

  if (!Array.isArray(parsed) || parsed.length === 0) {
    throw new Error('AUTH_GRPC_SERVICE_CREDENTIALS must be a non-empty array');
  }

  const credentialIds = new Set<string>();
  const tokens = new Set<string>();
  const credentials = parsed.map((value): AuthGrpcServiceCredential => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
      throw new Error(
        'AUTH_GRPC_SERVICE_CREDENTIALS contains an invalid entry',
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
      candidate.audience !== AUTH_GRPC_AUDIENCE ||
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
        'AUTH_GRPC_SERVICE_CREDENTIALS contains an invalid principal policy',
      );
    }

    credentialIds.add(credentialId);
    tokens.add(token);
    return Object.freeze({
      credentialId,
      principal,
      audience: AUTH_GRPC_AUDIENCE,
      token,
      scopes: Object.freeze([...(scopes as AuthGrpcScope[])]),
    });
  });

  return Object.freeze(credentials);
}

export function loadAuthGrpcServiceCredentials(
  environment: CredentialEnvironment = process.env as CredentialEnvironment,
): readonly AuthGrpcServiceCredential[] {
  const credentialFile = environment.AUTH_GRPC_SERVICE_CREDENTIALS_FILE?.trim();
  if (credentialFile) {
    let raw: string;
    try {
      raw = readFileSync(credentialFile, 'utf8');
    } catch {
      throw new Error('AUTH_GRPC_SERVICE_CREDENTIALS_FILE could not be read');
    }
    if (Buffer.byteLength(raw, 'utf8') > maximumRegistryBytes) {
      throw new Error('AUTH_GRPC_SERVICE_CREDENTIALS_FILE is too large');
    }
    return parseAuthGrpcServiceCredentials(raw);
  }

  const runtime = environment.NODE_ENV?.trim().toLowerCase() ?? '';
  if (runtime && runtime !== 'development' && runtime !== 'test') {
    throw new Error(
      'AUTH_GRPC_SERVICE_CREDENTIALS_FILE is required outside development',
    );
  }

  return parseAuthGrpcServiceCredentials(
    environment.AUTH_GRPC_SERVICE_CREDENTIALS,
  );
}

function exactMetadataString(metadata: Metadata, name: string): string {
  const values = metadata?.get(name) ?? [];
  if (values.length !== 1 || typeof values[0] !== 'string') return '';
  const value = values[0];
  return value && value === value.trim() ? value : '';
}

export function authorizeAuthGrpcService(
  metadata: Metadata,
  credentials: readonly AuthGrpcServiceCredential[],
  requiredScope: AuthGrpcScope,
): Readonly<{
  credentialId: string;
  principal: string;
  audience: typeof AUTH_GRPC_AUDIENCE;
}> {
  const credentialId = exactMetadataString(metadata, 'x-service-credential-id');
  const principal = exactMetadataString(metadata, 'x-service-principal');
  const token = exactMetadataString(metadata, 'x-service-auth');
  const credential = credentials.find(
    (candidate) => candidate.credentialId === credentialId,
  );
  const tokenMatches = secureEqual(
    credential?.token ?? 'invalid-auth-grpc-service-credential',
    token,
  );

  if (
    !credentialId ||
    !principal ||
    !token ||
    !credential ||
    credential.principal !== principal ||
    !tokenMatches
  ) {
    throw authorizationFailure(status.UNAUTHENTICATED);
  }

  if (!credential.scopes.includes(requiredScope)) {
    throw authorizationFailure(status.PERMISSION_DENIED);
  }

  return Object.freeze({
    credentialId,
    principal,
    audience: AUTH_GRPC_AUDIENCE,
  });
}
