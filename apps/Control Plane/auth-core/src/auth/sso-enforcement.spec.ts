/**
 * WS05-S7: SSO enforcement. The pure decision is tested with an injected
 * lookup; the org-core lookup with a stubbed fetch; the session hook with a
 * stubbed database, audit publisher and credential. Nothing here talks to a
 * network or a database.
 */
const mockPublishAudit = jest.fn();
const mockRequiredToken = jest.fn();
const mockSelectRows: { current: unknown[] } = { current: [] };
const mockSelectCalls = jest.fn();
const mockInsertValues = jest.fn();
const mockInsertShouldFail: { current: boolean } = { current: false };

jest.mock('better-auth/api', () => ({
  APIError: class APIError extends Error {
    constructor(
      readonly status: string,
      readonly body: Record<string, unknown>,
    ) {
      super(String(body?.message ?? status));
    }
  },
}));
jest.mock('./audit-plugin', () => ({
  publishVerevonAuditDurable: (...args: unknown[]) => mockPublishAudit(...args),
}));
jest.mock('./control-service-credentials', () => ({
  requiredScopedServiceToken: (...args: unknown[]) =>
    mockRequiredToken(...args),
}));
jest.mock('../db', () => {
  const chain = () => {
    const link: Record<string, unknown> = {};
    link.from = () => link;
    link.where = () => link;
    link.limit = () => {
      mockSelectCalls();
      return Promise.resolve(mockSelectRows.current);
    };
    return link;
  };
  return {
    db: {
      select: () => chain(),
      insert: () => ({
        values: (values: unknown) => {
          if (mockInsertShouldFail.current) {
            return Promise.reject(new Error('db down'));
          }
          mockInsertValues(values);
          return Promise.resolve();
        },
      }),
    },
  };
});

import {
  clearSsoPolicyCache,
  decideSsoSignIn,
  emailDomain,
  enforceSsoOnSessionCreate,
  isOrganizationAdmin,
  isSsoSignInPath,
  lookupSsoPolicyForDomain,
  lowerMode,
  recordSsoSignInEvidence,
  resolveSsoEnforcementCeiling,
  ssoProviderIdFromPath,
  SsoPolicyUnavailableError,
  type SsoMode,
  type SsoPolicyForDomain,
} from './sso-enforcement';

const PASSWORD_PATH = '/sign-in/email';
const SOCIAL_PATH = '/sign-in/social';
const PASSKEY_PATH = '/passkey/verify-authentication';
const SSO_PATH = '/sso/callback/acme-oidc';
const NON_SSO_PATHS = [
  PASSWORD_PATH,
  SOCIAL_PATH,
  '/callback/google',
  PASSKEY_PATH,
  '/admin/impersonate-user',
  undefined,
];
const SSO_PATHS = [
  '/sso/callback/acme-oidc',
  '/sso/callback',
  '/sso/callback/acme-oidc?code=abc&state=xyz',
  '/sso/saml2/callback/acme-saml',
  '/sso/saml2/sp/acs/acme-saml',
];

const policy = (
  mode: SsoMode,
  breakGlass: string[] = [],
): SsoPolicyForDomain => ({
  orgId: 'org-1',
  mode,
  breakGlassUserIds: breakGlass,
});

const input = (
  path: string | undefined,
  userId = 'user-1',
  email: string | null = 'user@acme.example',
) => ({
  userId,
  email,
  path,
});

describe('sso path and e-mail helpers', () => {
  it('knows the SSO callback endpoints and nothing else', () => {
    for (const path of SSO_PATHS) expect(isSsoSignInPath(path)).toBe(true);
    for (const path of NON_SSO_PATHS) expect(isSsoSignInPath(path)).toBe(false);
    for (const path of [
      '/sso/callback/a/b',
      '/sso/register',
      '/sso/sign-in',
      '/sign-in/sso',
      '/x/sso/callback/y',
      null,
      '',
    ]) {
      expect(isSsoSignInPath(path as string)).toBe(false);
    }
  });

  it('reads the provider id from a callback path', () => {
    expect(ssoProviderIdFromPath('/sso/callback/acme-oidc')).toBe('acme-oidc');
    expect(ssoProviderIdFromPath('/sso/callback/acme-oidc?code=1')).toBe(
      'acme-oidc',
    );
    expect(ssoProviderIdFromPath('/sso/callback/acme%20x')).toBe('acme x');
    expect(ssoProviderIdFromPath('/sso/callback')).toBeUndefined();
    expect(ssoProviderIdFromPath('/sign-in/email')).toBeUndefined();
    expect(ssoProviderIdFromPath(undefined)).toBeUndefined();
  });

  it('lower-cases the e-mail domain and refuses malformed addresses', () => {
    expect(emailDomain('User@ACME.Example')).toBe('acme.example');
    expect(emailDomain('a@b@acme.example')).toBe('acme.example');
    for (const bad of [
      'no-at-sign',
      '@acme.example',
      'user@',
      '',
      null,
      undefined,
    ]) {
      expect(emailDomain(bad as string)).toBeNull();
    }
  });

  it('takes the lower of two modes', () => {
    expect(lowerMode('enforce', 'report')).toBe('report');
    expect(lowerMode('report', 'enforce')).toBe('report');
    expect(lowerMode('enforce', 'off')).toBe('off');
    expect(lowerMode('off', 'enforce')).toBe('off');
    expect(lowerMode('enforce', 'enforce')).toBe('enforce');
  });
});

describe('resolveSsoEnforcementCeiling', () => {
  it('defaults to report and accepts the three modes in any case', () => {
    expect(resolveSsoEnforcementCeiling({} as NodeJS.ProcessEnv)).toBe(
      'report',
    );
    expect(
      resolveSsoEnforcementCeiling({
        SSO_ENFORCEMENT: '',
      } as NodeJS.ProcessEnv),
    ).toBe('report');
    expect(
      resolveSsoEnforcementCeiling({
        SSO_ENFORCEMENT: '   ',
      } as NodeJS.ProcessEnv),
    ).toBe('report');
    expect(
      resolveSsoEnforcementCeiling({
        SSO_ENFORCEMENT: ' ENFORCE ',
      } as NodeJS.ProcessEnv),
    ).toBe('enforce');
    expect(
      resolveSsoEnforcementCeiling({
        SSO_ENFORCEMENT: 'Off',
      } as NodeJS.ProcessEnv),
    ).toBe('off');
    expect(
      resolveSsoEnforcementCeiling({
        SSO_ENFORCEMENT: 'report',
      } as NodeJS.ProcessEnv),
    ).toBe('report');
  });

  it('refuses a bad value instead of guessing', () => {
    for (const bad of ['block', 'true', '1', 'enforced', 'on']) {
      expect(() =>
        resolveSsoEnforcementCeiling({
          SSO_ENFORCEMENT: bad,
        } as NodeJS.ProcessEnv),
      ).toThrow('SSO_ENFORCEMENT must be one of off, report, enforce');
    }
  });
});

describe('decideSsoSignIn', () => {
  const lookupOf = (value: SsoPolicyForDomain | null) =>
    jest.fn(async () => value);

  it.each<SsoMode>(['off', 'report', 'enforce'])(
    'lets an SSO callback through in every mode (ceiling %s)',
    async (ceiling) => {
      for (const mode of ['off', 'report', 'enforce'] as SsoMode[]) {
        for (const path of SSO_PATHS) {
          const lookup = lookupOf(policy(mode));
          await expect(
            decideSsoSignIn(input(path), ceiling, lookup),
          ).resolves.toEqual({ action: 'allow' });
          expect(lookup).not.toHaveBeenCalled();
        }
      }
    },
  );

  it('blocks e-mail, social, passkey and impersonation sign-ins under enforce', async () => {
    for (const path of NON_SSO_PATHS) {
      await expect(
        decideSsoSignIn(input(path), 'enforce', lookupOf(policy('enforce'))),
      ).resolves.toEqual({
        action: 'block',
        orgId: 'org-1',
        domain: 'acme.example',
      });
    }
  });

  it('only reports under a report policy, whatever the ceiling allows', async () => {
    await expect(
      decideSsoSignIn(
        input(PASSWORD_PATH),
        'enforce',
        lookupOf(policy('report')),
      ),
    ).resolves.toEqual({
      action: 'would_block',
      orgId: 'org-1',
      domain: 'acme.example',
    });
  });

  it('the ceiling downgrades enforce to would_block', async () => {
    await expect(
      decideSsoSignIn(
        input(PASSWORD_PATH),
        'report',
        lookupOf(policy('enforce')),
      ),
    ).resolves.toEqual({
      action: 'would_block',
      orgId: 'org-1',
      domain: 'acme.example',
    });
  });

  it('a ceiling of off skips the lookup altogether', async () => {
    const lookup = lookupOf(policy('enforce'));
    await expect(
      decideSsoSignIn(input(PASSWORD_PATH), 'off', lookup),
    ).resolves.toEqual({ action: 'allow' });
    expect(lookup).not.toHaveBeenCalled();
  });

  it('allows when there is nothing to apply: no policy, policy off, no usable e-mail', async () => {
    await expect(
      decideSsoSignIn(input(PASSWORD_PATH), 'enforce', lookupOf(null)),
    ).resolves.toEqual({ action: 'allow' });
    await expect(
      decideSsoSignIn(input(PASSWORD_PATH), 'enforce', lookupOf(policy('off'))),
    ).resolves.toEqual({ action: 'allow' });
    for (const email of [null, 'no-at-sign', 'user@']) {
      const lookup = lookupOf(policy('enforce'));
      await expect(
        decideSsoSignIn(
          input(PASSWORD_PATH, 'user-1', email),
          'enforce',
          lookup,
        ),
      ).resolves.toEqual({ action: 'allow' });
      expect(lookup).not.toHaveBeenCalled();
    }
  });

  it('looks the policy up by the lower-cased e-mail domain', async () => {
    const lookup = lookupOf(null);
    await decideSsoSignIn(
      input(PASSWORD_PATH, 'user-1', 'User@ACME.Example'),
      'enforce',
      lookup,
    );
    expect(lookup).toHaveBeenCalledWith('acme.example');
  });

  it('lets a break-glass owner in under enforce, flagged for audit', async () => {
    await expect(
      decideSsoSignIn(
        input(PASSWORD_PATH, 'owner-1'),
        'enforce',
        lookupOf(policy('enforce', ['owner-1', 'owner-2'])),
      ),
    ).resolves.toEqual({
      action: 'break_glass',
      orgId: 'org-1',
      domain: 'acme.example',
    });
    // Another user of the same org is still blocked.
    await expect(
      decideSsoSignIn(
        input(PASSWORD_PATH, 'user-9'),
        'enforce',
        lookupOf(policy('enforce', ['owner-1'])),
      ),
    ).resolves.toMatchObject({ action: 'block' });
  });

  it('a break-glass owner is not reported under a report policy either', async () => {
    await expect(
      decideSsoSignIn(
        input(PASSWORD_PATH, 'owner-1'),
        'enforce',
        lookupOf(policy('report', ['owner-1'])),
      ),
    ).resolves.toEqual({ action: 'allow' });
  });

  it('fails closed only while the ceiling is enforce when org-core cannot be read', async () => {
    const down = jest.fn(async () => {
      throw new SsoPolicyUnavailableError('org_core_unreachable');
    });
    await expect(
      decideSsoSignIn(input(PASSWORD_PATH), 'enforce', down),
    ).resolves.toEqual({ action: 'unavailable' });
    await expect(
      decideSsoSignIn(input(PASSWORD_PATH), 'report', down),
    ).resolves.toEqual({ action: 'allow' });
    // An SSO callback never asks, so an outage cannot lock out SSO.
    await expect(
      decideSsoSignIn(input(SSO_PATH), 'enforce', down),
    ).resolves.toEqual({ action: 'allow' });
    expect(down).toHaveBeenCalledTimes(2);
  });
});

describe('lookupSsoPolicyForDomain', () => {
  const realFetch = global.fetch;
  const fetchMock = jest.fn();

  beforeEach(() => {
    clearSsoPolicyCache();
    fetchMock.mockReset();
    mockRequiredToken
      .mockReset()
      .mockReturnValue('org-core-token-0123456789abcdef0123');
    global.fetch = fetchMock as unknown as typeof fetch;
  });
  afterEach(() => {
    global.fetch = realFetch;
    delete process.env.ORG_SERVICE_URL;
    delete process.env.ORG_CORE_URL;
  });

  const ok = (data: unknown) =>
    new Response(JSON.stringify({ data }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });

  it('asks org-core by domain with the service credential and parses the policy', async () => {
    process.env.ORG_SERVICE_URL = 'http://org.internal:8080/';
    fetchMock.mockResolvedValue(
      ok({
        org_id: 'org-1',
        mode: 'enforce',
        break_glass_user_ids: ['owner-1', 7, null],
      }),
    );
    await expect(lookupSsoPolicyForDomain('acme.example')).resolves.toEqual({
      orgId: 'org-1',
      mode: 'enforce',
      breakGlassUserIds: ['owner-1'],
    });
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe(
      'http://org.internal:8080/internal/orgs/by-domain/acme.example/sso-policy',
    );
    expect(init.method).toBe('GET');
    expect(init.headers['x-service-id']).toBe('auth-core');
    expect(init.headers['x-service-token']).toBe(
      'org-core-token-0123456789abcdef0123',
    );
    expect(init.signal).toBeDefined();
    expect(mockRequiredToken).toHaveBeenCalledWith('ORG_CORE_SERVICE_TOKEN');
  });

  it('encodes the domain into the path', async () => {
    fetchMock.mockResolvedValue(ok({ org_id: 'org-1', mode: 'report' }));
    await lookupSsoPolicyForDomain('a/b?c.example');
    expect(String(fetchMock.mock.calls[0][0])).toContain(
      '/internal/orgs/by-domain/a%2Fb%3Fc.example/sso-policy',
    );
  });

  it('treats a 404 as no policy and an unknown mode as off', async () => {
    fetchMock.mockResolvedValueOnce(new Response('{}', { status: 404 }));
    await expect(lookupSsoPolicyForDomain('none.example')).resolves.toBeNull();
    fetchMock.mockResolvedValueOnce(ok({ org_id: 'org-2', mode: 'block' }));
    await expect(lookupSsoPolicyForDomain('odd.example')).resolves.toEqual({
      orgId: 'org-2',
      mode: 'off',
      breakGlassUserIds: [],
    });
  });

  it('caches a successful answer, including none, for 60 seconds and not longer', async () => {
    fetchMock.mockImplementation(async () =>
      ok({ org_id: 'org-1', mode: 'report' }),
    );
    const t0 = 1_000_000;
    await lookupSsoPolicyForDomain('cache.example', t0);
    await lookupSsoPolicyForDomain('cache.example', t0 + 59_999);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    await lookupSsoPolicyForDomain('cache.example', t0 + 60_000);
    expect(fetchMock).toHaveBeenCalledTimes(2);

    fetchMock
      .mockReset()
      .mockImplementation(async () => new Response('{}', { status: 404 }));
    await lookupSsoPolicyForDomain('nothing.example', t0);
    await lookupSsoPolicyForDomain('nothing.example', t0 + 1);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('never caches a failure', async () => {
    fetchMock.mockResolvedValue(new Response('boom', { status: 502 }));
    await expect(
      lookupSsoPolicyForDomain('flaky.example', 5),
    ).rejects.toMatchObject({ reason: 'org_core_status_502' });
    fetchMock.mockResolvedValue(ok({ org_id: 'org-1', mode: 'enforce' }));
    await expect(
      lookupSsoPolicyForDomain('flaky.example', 6),
    ).resolves.toMatchObject({ mode: 'enforce' });
  });

  it('reports each failure with a closed reason', async () => {
    fetchMock.mockRejectedValueOnce(new Error('ECONNREFUSED'));
    await expect(lookupSsoPolicyForDomain('a.example')).rejects.toMatchObject({
      reason: 'org_core_unreachable',
    });
    fetchMock.mockResolvedValueOnce(new Response('not json', { status: 200 }));
    await expect(lookupSsoPolicyForDomain('b.example')).rejects.toMatchObject({
      reason: 'org_core_malformed_response',
    });
    fetchMock.mockResolvedValueOnce(ok({ mode: 'enforce' }));
    await expect(lookupSsoPolicyForDomain('c.example')).rejects.toMatchObject({
      reason: 'org_core_malformed_response',
    });
    fetchMock.mockResolvedValueOnce(ok(['not', 'an', 'object']));
    await expect(lookupSsoPolicyForDomain('d.example')).rejects.toBeInstanceOf(
      SsoPolicyUnavailableError,
    );
    fetchMock.mockResolvedValueOnce(new Response('denied', { status: 403 }));
    await expect(lookupSsoPolicyForDomain('e.example')).rejects.toMatchObject({
      reason: 'org_core_status_403',
    });
  });

  it('is unavailable, without calling org-core, when the credential is missing', async () => {
    mockRequiredToken.mockImplementation(() => {
      throw new Error('missing');
    });
    await expect(lookupSsoPolicyForDomain('f.example')).rejects.toMatchObject({
      reason: 'org_core_credential_missing',
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe('enforceSsoOnSessionCreate', () => {
  const realFetch = global.fetch;
  const fetchMock = jest.fn();
  const previousCeiling = process.env.SSO_ENFORCEMENT;
  const answer = (mode: SsoMode, breakGlass: string[] = []) =>
    fetchMock.mockImplementation(
      async () =>
        new Response(
          JSON.stringify({
            data: { org_id: 'org-1', mode, break_glass_user_ids: breakGlass },
          }),
          { status: 200 },
        ),
    );

  beforeEach(() => {
    clearSsoPolicyCache();
    fetchMock.mockReset();
    mockPublishAudit.mockReset().mockResolvedValue(undefined);
    mockSelectCalls.mockReset();
    mockSelectRows.current = [{ email: 'user@acme.example' }];
    mockRequiredToken
      .mockReset()
      .mockReturnValue('org-core-token-0123456789abcdef0123');
    global.fetch = fetchMock as unknown as typeof fetch;
    process.env.SSO_ENFORCEMENT = 'enforce';
    jest.spyOn(console, 'warn').mockImplementation(() => undefined);
  });
  afterEach(() => {
    global.fetch = realFetch;
    jest.restoreAllMocks();
    if (previousCeiling === undefined) delete process.env.SSO_ENFORCEMENT;
    else process.env.SSO_ENFORCEMENT = previousCeiling;
  });

  it('does nothing, and reads nothing, when the ceiling is off', async () => {
    process.env.SSO_ENFORCEMENT = 'off';
    await expect(
      enforceSsoOnSessionCreate({ userId: 'user-1' }, { path: PASSWORD_PATH }),
    ).resolves.toBeUndefined();
    expect(mockSelectCalls).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('lets an SSO callback session through without a user or policy lookup', async () => {
    await expect(
      enforceSsoOnSessionCreate({ userId: 'user-1' }, { path: SSO_PATH }),
    ).resolves.toBeUndefined();
    expect(mockSelectCalls).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('refuses a covered password sign-in with SSO_REQUIRED and the domain', async () => {
    answer('enforce');
    const failure = await enforceSsoOnSessionCreate(
      { userId: 'user-1' },
      { path: PASSWORD_PATH },
    ).catch((e: unknown) => e);
    expect(failure).toMatchObject({
      status: 'FORBIDDEN',
      body: { code: 'SSO_REQUIRED', domain: 'acme.example' },
    });
    expect(mockPublishAudit).not.toHaveBeenCalled();
  });

  it('refuses with SSO_POLICY_UNAVAILABLE when org-core is down and the ceiling is enforce', async () => {
    fetchMock.mockRejectedValue(new Error('down'));
    const failure = await enforceSsoOnSessionCreate(
      { userId: 'user-1' },
      { path: PASSWORD_PATH },
    ).catch((e: unknown) => e);
    expect(failure).toMatchObject({
      status: 'SERVICE_UNAVAILABLE',
      body: { code: 'SSO_POLICY_UNAVAILABLE' },
    });
  });

  it('lets the sign-in through when org-core is down but the ceiling is only report', async () => {
    process.env.SSO_ENFORCEMENT = 'report';
    fetchMock.mockRejectedValue(new Error('down'));
    await expect(
      enforceSsoOnSessionCreate({ userId: 'user-1' }, { path: PASSWORD_PATH }),
    ).resolves.toBeUndefined();
  });

  it('lets a user with no organisation policy through', async () => {
    fetchMock.mockImplementation(
      async () => new Response('{}', { status: 404 }),
    );
    await expect(
      enforceSsoOnSessionCreate({ userId: 'user-1' }, { path: PASSWORD_PATH }),
    ).resolves.toBeUndefined();
    mockSelectRows.current = [];
    await expect(
      enforceSsoOnSessionCreate({ userId: 'ghost' }, { path: PASSWORD_PATH }),
    ).resolves.toBeUndefined();
  });

  it('audits a break-glass sign-in with ids only: no e-mail, no domain', async () => {
    answer('enforce', ['user-1']);
    await expect(
      enforceSsoOnSessionCreate({ userId: 'user-1' }, { path: PASSWORD_PATH }),
    ).resolves.toBeUndefined();
    expect(mockPublishAudit).toHaveBeenCalledTimes(1);
    const event = mockPublishAudit.mock.calls[0][0];
    expect(event).toMatchObject({
      event: 'sso_break_glass_signin',
      org_id: 'org-1',
      user_id: 'user-1',
      plane: 'control',
      outcome: 'ok',
      details: { path_kind: 'email' },
    });
    expect(event.event_id).toMatch(/^sso:sso_break_glass_signin:/);
    expect(JSON.stringify(event)).not.toContain('acme.example');
    expect(JSON.stringify(event)).not.toContain('@');
  });

  it('audits would-block under a report policy and lets the sign-in through', async () => {
    answer('report');
    await expect(
      enforceSsoOnSessionCreate({ userId: 'user-1' }, { path: SOCIAL_PATH }),
    ).resolves.toBeUndefined();
    expect(mockPublishAudit.mock.calls[0][0]).toMatchObject({
      event: 'sso_enforcement_would_block',
      details: { path_kind: 'social' },
    });
  });

  it('a failing audit transport never changes the sign-in decision', async () => {
    mockPublishAudit.mockRejectedValue(new Error('nats down'));
    answer('report');
    await expect(
      enforceSsoOnSessionCreate({ userId: 'user-1' }, { path: PASSKEY_PATH }),
    ).resolves.toBeUndefined();
    answer('enforce', ['user-1']);
    await expect(
      enforceSsoOnSessionCreate({ userId: 'user-1' }, { path: PASSWORD_PATH }),
    ).resolves.toBeUndefined();
  });

  it('a bad SSO_ENFORCEMENT value throws rather than guessing', async () => {
    process.env.SSO_ENFORCEMENT = 'block';
    await expect(
      enforceSsoOnSessionCreate({ userId: 'user-1' }, { path: PASSWORD_PATH }),
    ).rejects.toThrow('SSO_ENFORCEMENT must be one of');
  });
});

describe('recordSsoSignInEvidence', () => {
  beforeEach(() => {
    mockInsertValues.mockReset();
    mockInsertShouldFail.current = false;
    jest.spyOn(console, 'warn').mockImplementation(() => undefined);
  });
  afterEach(() => jest.restoreAllMocks());

  it('records the user, organisation and provider for an SSO callback', async () => {
    mockSelectRows.current = [{ organizationId: 'org-1' }];
    await recordSsoSignInEvidence({ userId: 'user-1' }, { path: SSO_PATH });
    expect(mockInsertValues).toHaveBeenCalledWith({
      userId: 'user-1',
      organizationId: 'org-1',
      providerId: 'acme-oidc',
    });
  });

  it('records nothing for a non-SSO path, an unknown provider or a provider with no organisation', async () => {
    mockSelectRows.current = [{ organizationId: 'org-1' }];
    await recordSsoSignInEvidence(
      { userId: 'user-1' },
      { path: PASSWORD_PATH },
    );
    await recordSsoSignInEvidence(
      { userId: 'user-1' },
      { path: '/sso/callback' },
    );
    await recordSsoSignInEvidence({ userId: 'user-1' }, undefined);
    mockSelectRows.current = [];
    await recordSsoSignInEvidence({ userId: 'user-1' }, { path: SSO_PATH });
    mockSelectRows.current = [{ organizationId: null }];
    await recordSsoSignInEvidence({ userId: 'user-1' }, { path: SSO_PATH });
    expect(mockInsertValues).not.toHaveBeenCalled();
  });

  it('never fails the sign-in when the evidence cannot be written', async () => {
    mockSelectRows.current = [{ organizationId: 'org-1' }];
    mockInsertShouldFail.current = true;
    await expect(
      recordSsoSignInEvidence({ userId: 'user-1' }, { path: SSO_PATH }),
    ).resolves.toBeUndefined();
  });
});

describe('isOrganizationAdmin', () => {
  it('is true for owner and admin only, whatever the case', async () => {
    for (const [role, expected] of [
      ['owner', true],
      ['Admin', true],
      ['ADMIN', true],
      ['member', false],
      ['viewer', false],
      ['superadmin', false],
    ] as const) {
      mockSelectRows.current = [{ role }];
      await expect(isOrganizationAdmin('org-1', 'user-1')).resolves.toBe(
        expected,
      );
    }
    mockSelectRows.current = [];
    await expect(isOrganizationAdmin('org-1', 'stranger')).resolves.toBe(false);
  });
});
