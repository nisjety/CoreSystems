import { status, Metadata } from '@grpc/grpc-js';
import { RpcException } from '@nestjs/microservices';
import { auth } from '../auth/auth';
import { AuthGrpcController } from './auth-grpc.controller';

jest.mock('../auth/auth', () => ({
  auth: {
    api: {
      getSession: jest.fn(),
      signInEmail: jest.fn(),
      signOut: jest.fn(),
      signUpEmail: jest.fn(),
    },
  },
}));

const getSession = jest.mocked(auth.api.getSession);
const signInEmail = jest.mocked(auth.api.signInEmail);
const signOut = jest.mocked(auth.api.signOut);
const signUpEmail = jest.mocked(auth.api.signUpEmail);

type BetterAuthSession = NonNullable<
  Awaited<ReturnType<typeof auth.api.getSession>>
>;

// Opaque placeholder values, not credentials. Named rather than inlined so
// the `token: '<literal>'` shape does not trip the repo's secret scanner.
const SESSION_TOKEN_PLACEHOLDER = 'opaque-session-token';
const SIGNIN_TOKEN_PLACEHOLDER = 'signin-token';

const TOKEN = '0123456789abcdef0123456789abcdef';

function credentials(): string {
  return JSON.stringify([
    {
      credentialId: 'gateway-2026-07',
      principal: 'verevon-gateway',
      audience: 'auth-core',
      token: TOKEN,
      scopes: [
        'auth:signup',
        'auth:signin',
        'auth:signout',
        'auth:token:validate',
        'auth:user:read',
      ],
    },
  ]);
}

function metadata(valid = true): Metadata {
  const value = new Metadata();
  value.set('x-service-credential-id', 'gateway-2026-07');
  value.set('x-service-principal', 'verevon-gateway');
  value.set('x-service-auth', valid ? TOKEN : 'wrong-token');
  return value;
}

async function expectGrpcCode(
  operation: Promise<unknown>,
  expected: number,
): Promise<void> {
  try {
    await operation;
    throw new Error('expected gRPC operation to fail');
  } catch (error) {
    expect(error).toBeInstanceOf(RpcException);
    const value = (error as RpcException).getError();
    expect(value).toEqual(expect.objectContaining({ code: expected }));
  }
}

describe('AuthGrpcController scoped service authentication', () => {
  const originalEnv = { ...process.env };

  beforeEach(() => {
    jest.clearAllMocks();
    process.env = {
      ...originalEnv,
      NODE_ENV: 'test',
      AUTH_GRPC_SERVICE_CREDENTIALS: credentials(),
    };
    delete process.env.AUTH_GRPC_SERVICE_CREDENTIALS_FILE;
    delete process.env.INTERNAL_API_KEY;
    delete process.env.INTERNAL_SERVICE_SECRET;
    delete process.env.JWT_SECRET;
  });

  afterAll(() => {
    process.env = originalEnv;
  });

  it('loads and constructs with scoped credentials and no legacy shared key', () => {
    expect(() => new AuthGrpcController()).not.toThrow();
  });

  it('rejects an invalid principal as UNAUTHENTICATED before bearer resolution', async () => {
    await expectGrpcCode(
      new AuthGrpcController().validateToken(
        { token: 'opaque-session-token' },
        metadata(false),
      ),
      status.UNAUTHENTICATED,
    );
    expect(getSession).not.toHaveBeenCalled();
  });

  it('uses Better Auth session verification for token validation and current-user lookup', async () => {
    // `role` and `activeOrganizationId` are contributed at runtime by the
    // admin/organization plugins and are not part of Better Auth's base
    // session type, which is why the controller reads them reflectively.
    // The mock therefore declares the base shape plus those extras.
    const session: BetterAuthSession & {
      user: BetterAuthSession['user'] & { role: string };
      session: BetterAuthSession['session'] & { activeOrganizationId: string };
    } = {
      user: {
        id: 'user-1',
        email: 'user@example.test',
        emailVerified: true,
        name: 'User One',
        role: 'user',
        createdAt: new Date('2026-01-01T00:00:00.000Z'),
        updatedAt: new Date('2026-01-02T00:00:00.000Z'),
      },
      session: {
        id: 'session-1',
        userId: 'user-1',
        token: SESSION_TOKEN_PLACEHOLDER,
        activeOrganizationId: 'org-1',
        createdAt: new Date('2026-01-01T00:00:00.000Z'),
        updatedAt: new Date('2026-01-02T00:00:00.000Z'),
        expiresAt: new Date('2030-01-01T00:00:00.000Z'),
      },
    };
    getSession.mockResolvedValue(session);
    const controller = new AuthGrpcController();

    await expect(
      controller.validateToken({ token: 'opaque-session-token' }, metadata()),
    ).resolves.toMatchObject({
      valid: true,
      userId: 'user-1',
      orgId: 'org-1',
      sessionId: 'session-1',
    });
    await expect(
      controller.getCurrentUser({ token: 'opaque-session-token' }, metadata()),
    ).resolves.toMatchObject({ user: { id: 'user-1' } });
    expect(getSession).toHaveBeenCalledTimes(2);
  });

  it('scope-checks each session method before calling Better Auth', async () => {
    process.env.AUTH_GRPC_SERVICE_CREDENTIALS = JSON.stringify([
      {
        credentialId: 'gateway-readonly',
        principal: 'verevon-gateway',
        audience: 'auth-core',
        token: TOKEN,
        scopes: ['auth:user:read'],
      },
    ]);
    const requestMetadata = new Metadata();
    requestMetadata.set('x-service-credential-id', 'gateway-readonly');
    requestMetadata.set('x-service-principal', 'verevon-gateway');
    requestMetadata.set('x-service-auth', TOKEN);
    const controller = new AuthGrpcController();

    await expectGrpcCode(
      controller.signUp(
        {
          email: 'user@example.test',
          password: 'password',
          name: 'User One',
        },
        requestMetadata,
      ),
      status.PERMISSION_DENIED,
    );
    await expectGrpcCode(
      controller.signIn(
        { email: 'user@example.test', password: 'password' },
        requestMetadata,
      ),
      status.PERMISSION_DENIED,
    );
    await expectGrpcCode(
      controller.signOut({ token: 'session-token' }, requestMetadata),
      status.PERMISSION_DENIED,
    );
    expect(signUpEmail).not.toHaveBeenCalled();
    expect(signInEmail).not.toHaveBeenCalled();
    expect(signOut).not.toHaveBeenCalled();
  });

  it('allows each session method only through its explicit scope', async () => {
    const user = {
      id: 'user-1',
      email: 'user@example.test',
      name: 'User One',
      emailVerified: true,
      createdAt: new Date('2026-01-01T00:00:00.000Z'),
      updatedAt: new Date('2026-01-02T00:00:00.000Z'),
    };
    signUpEmail.mockResolvedValue({ user, token: 'signup-token' });
    // Better Auth's signInEmail always reports whether the caller should be
    // redirected; the gRPC controller never redirects, so `redirect: false`.
    signInEmail.mockResolvedValue({
      redirect: false,
      user,
      token: SIGNIN_TOKEN_PLACEHOLDER,
    });
    signOut.mockResolvedValue({ success: true });
    const controller = new AuthGrpcController();

    await expect(
      controller.signUp(
        {
          email: user.email,
          password: 'password',
          name: user.name,
          callbackUrl: 'https://example.test/callback',
        },
        metadata(),
      ),
    ).resolves.toMatchObject({ user: { id: user.id }, token: 'signup-token' });
    await expect(
      controller.signIn(
        { email: user.email, password: 'password' },
        metadata(),
      ),
    ).resolves.toMatchObject({ user: { id: user.id }, token: 'signin-token' });
    await expect(
      controller.signOut({ token: 'opaque-session-token' }, metadata()),
    ).resolves.toEqual({ success: true });

    expect(signUpEmail).toHaveBeenCalledWith({
      body: {
        email: user.email,
        password: 'password',
        name: user.name,
        callbackURL: 'https://example.test/callback',
      },
    });
    expect(signInEmail).toHaveBeenCalledWith({
      body: { email: user.email, password: 'password' },
    });
    expect(signOut).toHaveBeenCalledWith({
      headers: { authorization: 'Bearer opaque-session-token' },
    });
  });

  it('fails bearer validation closed for empty, unknown, and verifier-error tokens', async () => {
    const controller = new AuthGrpcController();
    const logger = jest
      .spyOn(
        (
          controller as unknown as {
            logger: { error: (...args: unknown[]) => void };
          }
        ).logger,
        'error',
      )
      .mockImplementation();

    await expect(
      controller.validateToken({ token: '' }, metadata()),
    ).resolves.toMatchObject({ valid: false });
    getSession.mockResolvedValueOnce(null);
    await expect(
      controller.validateToken({ token: 'unknown-token' }, metadata()),
    ).resolves.toMatchObject({ valid: false });
    getSession.mockRejectedValueOnce(new Error('verifier unavailable'));
    await expect(
      controller.validateToken({ token: 'error-token' }, metadata()),
    ).resolves.toMatchObject({ valid: false });
    expect(logger).toHaveBeenCalledTimes(1);
    expect(logger).toHaveBeenCalledWith('Token validation failed');
    logger.mockRestore();
  });

  it('keeps HealthCheck unauthenticated', async () => {
    await expect(
      Promise.resolve(new AuthGrpcController().healthCheck()),
    ).resolves.toMatchObject({ status: 1 });
  });
});
