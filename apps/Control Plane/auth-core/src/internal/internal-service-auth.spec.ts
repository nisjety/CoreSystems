import { status } from '@grpc/grpc-js';
import {
  authorizeAuthInternalService,
  authorizeAuthInternalServiceToken,
  loadAuthInternalServiceCredentials,
  parseAuthInternalServiceCredentials,
} from './internal-service-auth';

const USER_TOKEN = 'user-core-0123456789abcdef0123456789abcdef';
const USER_NEXT_TOKEN = 'user-core-next-0123456789abcdef0123456789abcdef';
const QUARRY_TOKEN = 'quarry-0123456789abcdef0123456789abcdef';

function expectAuthorizationCode(action: () => unknown, code: status): void {
  let caught: unknown;
  try {
    action();
  } catch (error) {
    caught = error;
  }
  expect(caught).toMatchObject({ code });
}

function registry(): string {
  return JSON.stringify([
    {
      credentialId: 'user-core-2026-07-a',
      principal: 'user-core',
      audience: 'auth-core-internal',
      token: USER_TOKEN,
      scopes: [
        'oauth:token:read',
        'oauth:token:refresh',
        'nats:authenticate',
        'auth:admin',
      ],
    },
    {
      credentialId: 'user-core-2026-07-b',
      principal: 'user-core',
      audience: 'auth-core-internal',
      token: USER_NEXT_TOKEN,
      scopes: ['oauth:token:read', 'nats:authenticate', 'auth:admin'],
    },
    {
      credentialId: 'quarry-2026-07',
      principal: 'quarry-control',
      audience: 'auth-core-internal',
      token: QUARRY_TOKEN,
      scopes: ['agent:provision'],
    },
  ]);
}

describe('Auth internal service-principal registry', () => {
  it('authorizes only an exact credential tuple with the required scope', () => {
    const credentials = parseAuthInternalServiceCredentials(registry());

    expect(
      authorizeAuthInternalService(
        {
          credentialId: 'user-core-2026-07-a',
          principal: 'user-core',
          token: USER_TOKEN,
        },
        credentials,
        'oauth:token:read',
      ),
    ).toEqual({
      credentialId: 'user-core-2026-07-a',
      principal: 'user-core',
      audience: 'auth-core-internal',
    });
  });

  it('denies cross-principal use and authenticated scope crossing', () => {
    const credentials = parseAuthInternalServiceCredentials(registry());

    expectAuthorizationCode(
      () =>
        authorizeAuthInternalService(
          {
            credentialId: 'user-core-2026-07-a',
            principal: 'quarry-control',
            token: USER_TOKEN,
          },
          credentials,
          'oauth:token:read',
        ),
      status.UNAUTHENTICATED,
    );

    expectAuthorizationCode(
      () =>
        authorizeAuthInternalService(
          {
            credentialId: 'quarry-2026-07',
            principal: 'quarry-control',
            token: QUARRY_TOKEN,
          },
          credentials,
          'oauth:token:read',
        ),
      status.PERMISSION_DENIED,
    );
  });

  it('supports overlap rotation and rejects the retired credential', () => {
    const overlap = parseAuthInternalServiceCredentials(registry());
    for (const [credentialId, token] of [
      ['user-core-2026-07-a', USER_TOKEN],
      ['user-core-2026-07-b', USER_NEXT_TOKEN],
    ] as const) {
      expect(() =>
        authorizeAuthInternalService(
          { credentialId, principal: 'user-core', token },
          overlap,
          'auth:admin',
        ),
      ).not.toThrow();
    }

    const retired = parseAuthInternalServiceCredentials(
      JSON.stringify([
        {
          credentialId: 'user-core-2026-07-b',
          principal: 'user-core',
          audience: 'auth-core-internal',
          token: USER_NEXT_TOKEN,
          scopes: ['auth:admin'],
        },
      ]),
    );
    expectAuthorizationCode(
      () =>
        authorizeAuthInternalService(
          {
            credentialId: 'user-core-2026-07-a',
            principal: 'user-core',
            token: USER_TOKEN,
          },
          retired,
          'auth:admin',
        ),
      status.UNAUTHENTICATED,
    );
  });

  it.each([
    ['', 'missing'],
    ['[', 'malformed'],
    [
      JSON.stringify([
        {
          credentialId: 'user-core-a',
          principal: 'user-core',
          audience: 'wrong',
          token: USER_TOKEN,
          scopes: ['auth:admin'],
        },
      ]),
      'wrong audience',
    ],
    [
      JSON.stringify([
        {
          credentialId: 'user-core-a',
          principal: 'user-core',
          audience: 'auth-core-internal',
          token: 'short',
          scopes: ['auth:admin'],
        },
      ]),
      'short token',
    ],
    [
      JSON.stringify([
        {
          credentialId: 'user-core-a',
          principal: 'user-core',
          audience: 'auth-core-internal',
          token: USER_TOKEN,
          scopes: ['*'],
        },
      ]),
      'wildcard scope',
    ],
  ])('rejects invalid registry: %s (%s)', (raw) => {
    expect(() => parseAuthInternalServiceCredentials(raw)).toThrow();
  });

  it('requires a deployment-owned file outside development/test', () => {
    expect(() =>
      loadAuthInternalServiceCredentials({
        NODE_ENV: 'production',
        AUTH_INTERNAL_SERVICE_CREDENTIALS: registry(),
      }),
    ).toThrow(/FILE is required outside development/);
  });

  it('does not consult legacy shared-key environment variables', () => {
    process.env.INTERNAL_API_KEY = USER_TOKEN;
    process.env.INTERNAL_SERVICE_SECRET = USER_TOKEN;
    const credentials = parseAuthInternalServiceCredentials(registry());
    expectAuthorizationCode(
      () =>
        authorizeAuthInternalService(
          { credentialId: '', principal: '', token: USER_TOKEN },
          credentials,
          'auth:admin',
        ),
      status.UNAUTHENTICATED,
    );
    delete process.env.INTERNAL_API_KEY;
    delete process.env.INTERNAL_SERVICE_SECRET;
  });

  it('resolves the legacy oRPC adapter header only through the scoped registry', () => {
    const credentials = parseAuthInternalServiceCredentials(registry());
    expect(
      authorizeAuthInternalServiceToken(
        USER_TOKEN,
        credentials,
        'auth:admin',
        'user-core',
      ),
    ).toMatchObject({
      credentialId: 'user-core-2026-07-a',
      principal: 'user-core',
    });
    expectAuthorizationCode(
      () =>
        authorizeAuthInternalServiceToken(
          QUARRY_TOKEN,
          credentials,
          'auth:admin',
          'user-core',
        ),
      status.UNAUTHENTICATED,
    );
  });
});
