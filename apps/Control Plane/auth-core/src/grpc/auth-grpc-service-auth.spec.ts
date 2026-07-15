import { status, Metadata } from '@grpc/grpc-js';
import { RpcException } from '@nestjs/microservices';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  authorizeAuthGrpcService,
  loadAuthGrpcServiceCredentials,
  parseAuthGrpcServiceCredentials,
} from './auth-grpc-service-auth';

const RETRIEVAL_TOKEN_OLD = '0123456789abcdef0123456789abcdef';
const RETRIEVAL_TOKEN_NEW = 'abcdef0123456789abcdef0123456789';
const GATEWAY_TOKEN = 'fedcba9876543210fedcba9876543210';

type RegistryOptions = Readonly<{
  includeOld?: boolean;
  includeNew?: boolean;
}>;

function registry({
  includeOld = true,
  includeNew = true,
}: RegistryOptions = {}): string {
  return JSON.stringify([
    ...(includeOld
      ? [
          {
            credentialId: 'retrieval-2026-07-old',
            principal: 'retrieval-engine',
            audience: 'auth-core',
            token: RETRIEVAL_TOKEN_OLD,
            scopes: ['auth:token:validate'],
          },
        ]
      : []),
    ...(includeNew
      ? [
          {
            credentialId: 'retrieval-2026-07-new',
            principal: 'retrieval-engine',
            audience: 'auth-core',
            token: RETRIEVAL_TOKEN_NEW,
            scopes: ['auth:token:validate'],
          },
        ]
      : []),
    {
      credentialId: 'gateway-2026-07',
      principal: 'velion-gateway',
      audience: 'auth-core',
      token: GATEWAY_TOKEN,
      scopes: ['auth:signin', 'auth:signout', 'auth:user:read'],
    },
  ]);
}

function metadata(
  credentialId?: string,
  principal?: string,
  token?: string,
): Metadata {
  const value = new Metadata();
  if (credentialId) value.set('x-service-credential-id', credentialId);
  if (principal) value.set('x-service-principal', principal);
  if (token) value.set('x-service-auth', token);
  return value;
}

function grpcCode(error: unknown): number | undefined {
  if (!(error instanceof RpcException)) return undefined;
  const value = error.getError();
  return typeof value === 'object' && value !== null && 'code' in value
    ? (value.code as number)
    : undefined;
}

describe('Auth gRPC scoped service credentials', () => {
  it('parses a fixed-audience registry with per-method scopes and rotation overlap', () => {
    const credentials = parseAuthGrpcServiceCredentials(registry());

    expect(credentials).toHaveLength(3);
    expect(
      credentials.filter(
        (credential) => credential.principal === 'retrieval-engine',
      ),
    ).toHaveLength(2);
  });

  it.each([
    ['missing registry', ''],
    ['malformed JSON', 'not-json'],
    ['non-array registry', '{}'],
    [
      'wrong audience',
      JSON.stringify([
        {
          credentialId: 'retrieval-2026-07',
          principal: 'retrieval-engine',
          audience: 'other-service',
          token: RETRIEVAL_TOKEN_OLD,
          scopes: ['auth:token:validate'],
        },
      ]),
    ],
    [
      'placeholder token',
      JSON.stringify([
        {
          credentialId: 'retrieval-2026-07',
          principal: 'retrieval-engine',
          audience: 'auth-core',
          token: 'change-me-credential-that-is-long-enough',
          scopes: ['auth:token:validate'],
        },
      ]),
    ],
    [
      'legacy broad scope',
      JSON.stringify([
        {
          credentialId: 'retrieval-2026-07',
          principal: 'retrieval-engine',
          audience: 'auth-core',
          token: RETRIEVAL_TOKEN_OLD,
          scopes: ['auth:session:write'],
        },
      ]),
    ],
    [
      'duplicate credential ID',
      JSON.stringify([
        {
          credentialId: 'duplicate-id',
          principal: 'retrieval-engine',
          audience: 'auth-core',
          token: RETRIEVAL_TOKEN_OLD,
          scopes: ['auth:token:validate'],
        },
        {
          credentialId: 'duplicate-id',
          principal: 'velion-gateway',
          audience: 'auth-core',
          token: GATEWAY_TOKEN,
          scopes: ['auth:user:read'],
        },
      ]),
    ],
    [
      'duplicate token',
      JSON.stringify([
        {
          credentialId: 'retrieval-2026-07',
          principal: 'retrieval-engine',
          audience: 'auth-core',
          token: RETRIEVAL_TOKEN_OLD,
          scopes: ['auth:token:validate'],
        },
        {
          credentialId: 'gateway-2026-07',
          principal: 'velion-gateway',
          audience: 'auth-core',
          token: RETRIEVAL_TOKEN_OLD,
          scopes: ['auth:user:read'],
        },
      ]),
    ],
  ])('rejects %s', (_name, raw) => {
    expect(() => parseAuthGrpcServiceCredentials(raw)).toThrow();
  });

  it('loads a registry file and permits direct environment JSON only outside production', () => {
    const directory = mkdtempSync(join(tmpdir(), 'auth-grpc-credentials-'));
    const file = join(directory, 'registry.json');
    writeFileSync(file, registry(), { mode: 0o600 });

    try {
      expect(
        loadAuthGrpcServiceCredentials({
          NODE_ENV: 'production',
          AUTH_GRPC_SERVICE_CREDENTIALS_FILE: file,
          AUTH_GRPC_SERVICE_CREDENTIALS: 'not-json-and-must-not-be-read',
        }),
      ).toHaveLength(3);
      expect(
        loadAuthGrpcServiceCredentials({
          NODE_ENV: 'development',
          AUTH_GRPC_SERVICE_CREDENTIALS: registry(),
        }),
      ).toHaveLength(3);
      expect(() =>
        loadAuthGrpcServiceCredentials({
          NODE_ENV: 'production',
          AUTH_GRPC_SERVICE_CREDENTIALS: registry(),
        }),
      ).toThrow(/FILE/);
      expect(() =>
        loadAuthGrpcServiceCredentials({
          NODE_ENV: 'staging',
          AUTH_GRPC_SERVICE_CREDENTIALS: registry(),
        }),
      ).toThrow(/FILE/);
      expect(() =>
        loadAuthGrpcServiceCredentials({
          NODE_ENV: 'production',
          AUTH_GRPC_SERVICE_CREDENTIALS_FILE: join(directory, 'missing.json'),
        }),
      ).toThrow(/could not be read/);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it('authorizes both rotation credentials during overlap and only the survivor after retirement', () => {
    const overlapping = parseAuthGrpcServiceCredentials(registry());

    expect(
      authorizeAuthGrpcService(
        metadata(
          'retrieval-2026-07-old',
          'retrieval-engine',
          RETRIEVAL_TOKEN_OLD,
        ),
        overlapping,
        'auth:token:validate',
      ),
    ).toEqual({
      credentialId: 'retrieval-2026-07-old',
      principal: 'retrieval-engine',
      audience: 'auth-core',
    });
    expect(
      authorizeAuthGrpcService(
        metadata(
          'retrieval-2026-07-new',
          'retrieval-engine',
          RETRIEVAL_TOKEN_NEW,
        ),
        overlapping,
        'auth:token:validate',
      ),
    ).toMatchObject({ credentialId: 'retrieval-2026-07-new' });

    const retired = parseAuthGrpcServiceCredentials(
      registry({ includeOld: false }),
    );
    try {
      authorizeAuthGrpcService(
        metadata(
          'retrieval-2026-07-old',
          'retrieval-engine',
          RETRIEVAL_TOKEN_OLD,
        ),
        retired,
        'auth:token:validate',
      );
      throw new Error('expected retired credential to be rejected');
    } catch (error) {
      expect(grpcCode(error)).toBe(status.UNAUTHENTICATED);
    }
  });

  it('returns UNAUTHENTICATED for missing, mismatched, or ambiguous credential metadata', () => {
    const credentials = parseAuthGrpcServiceCredentials(registry());
    const ambiguousMetadata = [
      'x-service-credential-id',
      'x-service-principal',
      'x-service-auth',
    ].map((name) => {
      const value = metadata(
        'retrieval-2026-07-old',
        'retrieval-engine',
        RETRIEVAL_TOKEN_OLD,
      );
      value.add(name, 'second-value');
      return value;
    });

    const denied = [
      metadata(),
      metadata('retrieval-2026-07-old', 'retrieval-engine', 'wrong-token'),
      metadata('retrieval-2026-07-old', 'other-principal', RETRIEVAL_TOKEN_OLD),
      metadata('unknown-credential', 'retrieval-engine', RETRIEVAL_TOKEN_OLD),
      ...ambiguousMetadata,
    ];

    for (const requestMetadata of denied) {
      try {
        authorizeAuthGrpcService(
          requestMetadata,
          credentials,
          'auth:token:validate',
        );
        throw new Error('expected authorization to fail');
      } catch (error) {
        expect(grpcCode(error)).toBe(status.UNAUTHENTICATED);
      }
    }
  });

  it('returns PERMISSION_DENIED only after authenticating an exact credential tuple', () => {
    const credentials = parseAuthGrpcServiceCredentials(registry());

    try {
      authorizeAuthGrpcService(
        metadata('gateway-2026-07', 'velion-gateway', GATEWAY_TOKEN),
        credentials,
        'auth:signup',
      );
      throw new Error('expected authorization to fail');
    } catch (error) {
      expect(grpcCode(error)).toBe(status.PERMISSION_DENIED);
    }
  });

  it('does not treat legacy shared-key environment variables as credentials', () => {
    const previousApiKey = process.env.INTERNAL_API_KEY;
    const previousSecret = process.env.INTERNAL_SERVICE_SECRET;
    process.env.INTERNAL_API_KEY = RETRIEVAL_TOKEN_OLD;
    process.env.INTERNAL_SERVICE_SECRET = RETRIEVAL_TOKEN_OLD;
    try {
      const credentials = parseAuthGrpcServiceCredentials(registry());
      try {
        authorizeAuthGrpcService(
          metadata(
            'retrieval-2026-07-old',
            'retrieval-engine',
            'legacy-shared-key',
          ),
          credentials,
          'auth:token:validate',
        );
        throw new Error('expected authorization to fail');
      } catch (error) {
        expect(grpcCode(error)).toBe(status.UNAUTHENTICATED);
      }
    } finally {
      if (previousApiKey === undefined) delete process.env.INTERNAL_API_KEY;
      else process.env.INTERNAL_API_KEY = previousApiKey;
      if (previousSecret === undefined)
        delete process.env.INTERNAL_SERVICE_SECRET;
      else process.env.INTERNAL_SERVICE_SECRET = previousSecret;
    }
  });
});
