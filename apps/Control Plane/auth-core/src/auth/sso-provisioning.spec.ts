// WS05-S3: SSO-provisioned members always join as `member`.
//
// The old rule derived `admin` from IdP-asserted attributes (a job title containing
// manager/director/admin, or department IT): anyone who could set their own title at the IdP
// became an org admin on first sign-in. These tests load the real auth.ts with SSO on and read
// the options it hands to the SSO plugin, so they fail if a role is derived from attributes
// again, if SSO_DEFAULT_ROLE can raise the role, or if provisioning logs the user's email.

type CapturedSsoOptions = {
  provisionUser?: (input: {
    user: { id: string; email: string; name?: string };
    userInfo?: unknown;
    provider: { providerId: string };
  }) => unknown;
  organizationProvisioning?: {
    disabled?: boolean;
    defaultRole?: string;
    getRole?: unknown;
  };
};

// The plugin factory is replaced by a recorder; every other better-auth surface is the real one.
let captured = undefined as CapturedSsoOptions | undefined;

// better-auth ships ESM only, which this package's Jest setup does not transform, so its
// surface is stubbed: auth.ts itself (the code under test) is loaded for real.
const stubPlugin = (name: string) => ({ id: name });
jest.mock('better-auth', () => ({
  betterAuth: () => ({ handler: jest.fn() }),
}));
jest.mock('better-auth/api', () => ({
  APIError: class APIError extends Error {},
  createAuthMiddleware: (handler: unknown) => handler,
}));
jest.mock('better-auth/adapters/drizzle', () => ({
  drizzleAdapter: () => ({}),
}));
jest.mock(
  'better-auth/plugins',
  () =>
    new Proxy(
      {},
      {
        get: (_target, name) =>
          name === '__esModule' ? false : () => stubPlugin(String(name)),
      },
    ),
);
jest.mock('@better-auth/api-key', () => ({
  apiKey: () => stubPlugin('apiKey'),
}));
jest.mock('@better-auth/passkey', () => ({
  passkey: () => stubPlugin('passkey'),
}));
jest.mock('@better-auth/sso', () => ({
  sso: (options: CapturedSsoOptions) => {
    captured = options;
    return stubPlugin('sso');
  },
}));
jest.mock('./permissions', () => ({ ac: {}, roles: {} }));
jest.mock('./audit-plugin', () => ({ auditPlugin: () => stubPlugin('audit') }));
jest.mock('./user-service-integration.plugin', () => ({
  userServiceIntegrationPlugin: () => stubPlugin('user-service'),
}));
jest.mock('./organization-events.plugin', () => ({
  organizationEventsPlugin: () => stubPlugin('organization-events'),
}));
jest.mock('./sole-org-auto-activation', () => ({
  withSoleOrganizationActivated: (value: unknown) => value,
}));
jest.mock('./microsoft-signin-handoff', () => ({
  pushMicrosoftSignInHandoff: jest.fn(),
}));
// The publisher reads `schema.apikey` when it loads, and this spec's schema mock is empty.
jest.mock('../nats/api-key-revocation.publisher', () => ({
  loadApiKeyRevocationRow: jest.fn(),
}));
jest.mock('../db', () => ({ db: {}, sqlClient: jest.fn() }));
jest.mock('../db/schema', () => ({}));
jest.mock('../db/redis', () => ({ redisSecondaryStorage: undefined }));
jest.mock('../sms/twilio-verify.service', () => ({
  TwilioVerifyService: class TwilioVerifyService {},
}));
jest.mock('resend', () => ({ Resend: class Resend {} }));
jest.mock('jose', () => ({
  importPKCS8: jest.fn(),
  SignJWT: class SignJWT {},
}));

const SSO_ENV_KEYS = [
  'SSO_ENABLED',
  'SSO_DEFAULT_ROLE',
  'SSO_ORG_PROVISIONING_DISABLED',
] as const;

function loadAuthWithSso(env: Record<string, string | undefined>) {
  captured = undefined as CapturedSsoOptions | undefined;
  for (const key of SSO_ENV_KEYS) delete process.env[key];
  for (const [key, value] of Object.entries(env)) {
    if (value !== undefined) process.env[key] = value;
  }
  // Loading auth.ts logs (dotenv, the mock mail service); keep the output readable.
  const quiet = [jest.spyOn(console, 'log'), jest.spyOn(console, 'warn')];
  for (const spy of quiet) spy.mockImplementation(() => undefined);
  try {
    jest.isolateModules(() => {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      require('./auth');
    });
  } finally {
    for (const spy of quiet) spy.mockRestore();
  }
  return captured;
}

describe('SSO provisioning never derives a role from IdP attributes', () => {
  const saved: Record<string, string | undefined> = {};

  beforeAll(() => {
    for (const key of SSO_ENV_KEYS) saved[key] = process.env[key];
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  afterAll(() => {
    for (const key of SSO_ENV_KEYS) {
      if (saved[key] === undefined) delete process.env[key];
      else process.env[key] = saved[key];
    }
  });

  it('has no getRole hook in organizationProvisioning', () => {
    const options = loadAuthWithSso({ SSO_ENABLED: 'true' });
    expect(options).toBeDefined();
    expect(options?.organizationProvisioning).toBeDefined();
    expect(options?.organizationProvisioning).not.toHaveProperty('getRole');
    expect(options?.organizationProvisioning?.getRole).toBeUndefined();
  });

  it('provisions a user whose jobTitle is Director and department is IT as member', () => {
    const options = loadAuthWithSso({ SSO_ENABLED: 'true' });
    const provisioning = options?.organizationProvisioning;
    // With no getRole hook the plugin falls back to defaultRole for every user, whatever
    // attributes the IdP asserts; there is nothing left that could read them.
    expect(provisioning?.defaultRole).toBe('member');
    expect(typeof provisioning?.getRole).toBe('undefined');
    const attributes = { jobTitle: 'Director', department: 'IT' };
    expect(JSON.stringify(provisioning)).not.toContain(attributes.jobTitle);
    expect(JSON.stringify(provisioning)).not.toContain('admin');
  });

  it.each([
    ['unset', {}],
    ['empty', { SSO_DEFAULT_ROLE: '' }],
    ['blank', { SSO_DEFAULT_ROLE: '   ' }],
    ['member', { SSO_DEFAULT_ROLE: 'member' }],
    ['Member with padding', { SSO_DEFAULT_ROLE: ' Member ' }],
  ])('accepts SSO_DEFAULT_ROLE %s and uses member', (_label, extra) => {
    const options = loadAuthWithSso({ SSO_ENABLED: 'true', ...extra });
    expect(options?.organizationProvisioning?.defaultRole).toBe('member');
  });

  it.each(['admin', 'owner', 'Admin', ' admin ', 'superadmin', 'viewer'])(
    'refuses to build the config when SSO_DEFAULT_ROLE=%j and SSO is enabled',
    (role) => {
      expect(() =>
        loadAuthWithSso({ SSO_ENABLED: 'true', SSO_DEFAULT_ROLE: role }),
      ).toThrow(/SSO_DEFAULT_ROLE must be unset or "member"/);
    },
  );

  it('does not read SSO_DEFAULT_ROLE at all while SSO is disabled', () => {
    expect(() =>
      loadAuthWithSso({ SSO_ENABLED: 'false', SSO_DEFAULT_ROLE: 'admin' }),
    ).not.toThrow();
    expect(captured).toBeUndefined();
  });

  it('logs the user id and provider id when provisioning, never the email', () => {
    const options = loadAuthWithSso({ SSO_ENABLED: 'true' });
    const log = jest.spyOn(console, 'log').mockImplementation(() => undefined);

    options?.provisionUser?.({
      user: {
        id: 'user_sso_1',
        email: 'ada.lovelace@example.com',
        name: 'Ada Lovelace',
      },
      userInfo: { attributes: { jobTitle: 'Director', department: 'IT' } },
      provider: { providerId: 'okta-acme' },
    });

    expect(log).toHaveBeenCalledTimes(1);
    const line = String(log.mock.calls[0]?.join(' '));
    expect(line).toContain('user_sso_1');
    expect(line).toContain('okta-acme');
    expect(line).not.toContain('ada.lovelace@example.com');
    expect(line).not.toContain('@');
    expect(line).not.toContain('Ada Lovelace');
  });
});
