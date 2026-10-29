import { readFileSync } from 'node:fs';

export const USER_CORE_GRPC_AUDIENCE = 'user-core-grpc' as const;

export type UserCoreGrpcClientCredential = Readonly<{
  credentialId: string;
  principal: 'auth-core';
  audience: typeof USER_CORE_GRPC_AUDIENCE;
  token: string;
}>;

type CredentialEnvironment = Readonly<{
  NODE_ENV?: string;
  USER_CORE_GRPC_CLIENT_CREDENTIAL?: string;
  USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE?: string;
}>;

const maximumCredentialBytes = 64 * 1024;
const placeholderPrefixes = [
  'test',
  'placeholder',
  'change-me',
  'replace-with',
  'your-',
] as const;

function identifier(value: unknown): string {
  if (typeof value !== 'string' || value !== value.trim()) return '';
  return /^[a-z0-9][a-z0-9._-]{1,127}$/i.test(value) ? value : '';
}

function token(value: unknown): string {
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

export function parseUserCoreGrpcClientCredential(
  raw: string | undefined,
): UserCoreGrpcClientCredential {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw?.trim() || 'null');
  } catch {
    throw new Error('USER_CORE_GRPC_CLIENT_CREDENTIAL is malformed');
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('USER_CORE_GRPC_CLIENT_CREDENTIAL must be an object');
  }
  const candidate = parsed as Record<string, unknown>;
  if (
    Object.keys(candidate).sort().join(',') !==
      'audience,credentialId,principal,token' ||
    !identifier(candidate.credentialId) ||
    candidate.principal !== 'auth-core' ||
    candidate.audience !== USER_CORE_GRPC_AUDIENCE ||
    !token(candidate.token)
  ) {
    throw new Error(
      'USER_CORE_GRPC_CLIENT_CREDENTIAL contains an invalid principal policy',
    );
  }
  return Object.freeze({
    credentialId: candidate.credentialId as string,
    principal: 'auth-core',
    audience: USER_CORE_GRPC_AUDIENCE,
    token: candidate.token as string,
  });
}

export function loadUserCoreGrpcClientCredential(
  environment: CredentialEnvironment = process.env as CredentialEnvironment,
): UserCoreGrpcClientCredential {
  const credentialFile =
    environment.USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE?.trim();
  if (credentialFile) {
    let raw: string;
    try {
      raw = readFileSync(credentialFile, 'utf8');
    } catch {
      throw new Error(
        'USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE could not be read',
      );
    }
    if (Buffer.byteLength(raw, 'utf8') > maximumCredentialBytes) {
      throw new Error('USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE is too large');
    }
    return parseUserCoreGrpcClientCredential(raw);
  }

  const runtime = environment.NODE_ENV?.trim().toLowerCase() ?? '';
  if (runtime && runtime !== 'development' && runtime !== 'test') {
    throw new Error(
      'USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE is required outside development',
    );
  }
  return parseUserCoreGrpcClientCredential(
    environment.USER_CORE_GRPC_CLIENT_CREDENTIAL,
  );
}
