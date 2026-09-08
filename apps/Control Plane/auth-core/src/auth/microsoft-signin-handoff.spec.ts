import { createCipheriv, randomBytes } from 'node:crypto';
import {
  buildSignInHandoffPayload,
  decryptProviderToken,
  pushMicrosoftSignInHandoff,
  splitScopes,
  type MicrosoftAccountTokens,
} from './microsoft-signin-handoff';

jest.mock('../db', () => ({ db: { select: jest.fn() }, sqlClient: jest.fn() }));

function encryptLikeAuthTs(token: string, keyB64: string): string {
  const key = Buffer.from(keyB64, 'base64');
  const iv = randomBytes(12);
  const cipher = createCipheriv('aes-256-gcm', key, iv);
  const enc = Buffer.concat([cipher.update(token, 'utf8'), cipher.final()]);
  return `${iv.toString('base64')}.${cipher.getAuthTag().toString('base64')}.${enc.toString('base64')}`;
}

// The token ref and provider account id are deliberately unmistakable
// fixtures rather than realistic-looking strings: the earlier values were
// long, high-entropy and read exactly like live credentials, so the secret
// gate refused this file. Nothing here is length- or format-constrained --
// both fields are plain `string`, and the assertions compare the fixture to
// itself -- so the shape is free.
const account: MicrosoftAccountTokens = {
  tokenRef: 'fixture-token-ref-not-a-real-value',
  providerAccountId: 'fixture-provider-account-id-not-a-real-value',
  accessToken: 'graph-access-token',
  refreshTokenPresent: true,
  expiresAt: new Date('2026-09-06T15:00:00.000Z'),
  scope: 'email,openid,profile,User.Read,Mail.Read,Files.Read.All',
};

const now = new Date('2026-09-06T14:00:00.000Z');

describe('decryptProviderToken', () => {
  it('round-trips the iv.tag.ciphertext format auth.ts writes', () => {
    const key = randomBytes(32).toString('base64');
    expect(
      decryptProviderToken(encryptLikeAuthTs('secret-token', key), key),
    ).toBe('secret-token');
  });

  it('passes plaintext through when no key is configured or the value is not encrypted', () => {
    expect(decryptProviderToken('plain', undefined)).toBe('plain');
    expect(
      decryptProviderToken('plain', randomBytes(32).toString('base64')),
    ).toBe('plain');
  });
});

describe('splitScopes', () => {
  it('accepts Better Auth commas and provider spaces and dedupes', () => {
    expect(splitScopes('email,openid, profile User.Read User.Read')).toEqual([
      'email',
      'openid',
      'profile',
      'User.Read',
    ]);
    expect(splitScopes(null)).toEqual([]);
  });
});

describe('buildSignInHandoffPayload', () => {
  it('maps the session and account onto the integration-core contract', () => {
    const built = buildSignInHandoffPayload(
      { userId: 'user-1', activeOrganizationId: 'org-1' },
      account,
      'ima.dacosta@aquatiq.com',
      now,
    );
    expect(built).toEqual({
      payload: {
        organizationId: 'org-1',
        userId: 'user-1',
        userEmail: 'ima.dacosta@aquatiq.com',
        providerAccountId: account.providerAccountId,
        accessToken: 'graph-access-token',
        expiresAt: '2026-09-06T15:00:00.000Z',
        scopes: [
          'email',
          'openid',
          'profile',
          'User.Read',
          'Mail.Read',
          'Files.Read.All',
        ],
        tokenRef: account.tokenRef,
      },
    });
  });

  it('skips when the session has no active organization — a connection is org-scoped', () => {
    expect(
      buildSignInHandoffPayload({ userId: 'user-1' }, account, undefined, now),
    ).toEqual({
      skip: 'skipped:no-organization',
    });
    expect(
      buildSignInHandoffPayload(
        { userId: 'user-1', activeOrganizationId: '  ' },
        account,
        undefined,
        now,
      ),
    ).toEqual({ skip: 'skipped:no-organization' });
  });

  it('skips users without a linked Microsoft account', () => {
    expect(
      buildSignInHandoffPayload(
        { userId: 'user-1', activeOrganizationId: 'org-1' },
        null,
        undefined,
        now,
      ),
    ).toEqual({ skip: 'skipped:no-microsoft-account' });
  });

  // A password login does not refresh a linked Microsoft account; pushing its
  // stale token would overwrite a fresher one integration-core already holds.
  it('skips an expired token instead of pushing it', () => {
    expect(
      buildSignInHandoffPayload(
        { userId: 'user-1', activeOrganizationId: 'org-1' },
        { ...account, expiresAt: new Date('2026-09-04T11:58:24.000Z') },
        undefined,
        now,
      ),
    ).toEqual({ skip: 'skipped:token-expired' });
  });
});

describe('pushMicrosoftSignInHandoff', () => {
  const env = { INTERNAL_API_KEY: 'internal-key-internal-key-internal-key' };
  const silent = { warn: jest.fn(), log: jest.fn() };

  beforeEach(() => jest.clearAllMocks());

  it('POSTs the token to integration-core with the internal API key', async () => {
    const fetchMock = jest.fn().mockResolvedValue({ ok: true, status: 201 });
    const outcome = await pushMicrosoftSignInHandoff(
      { userId: 'user-1', activeOrganizationId: 'org-1' },
      {
        fetch: fetchMock as unknown as typeof fetch,
        env,
        now: () => now,
        loadAccount: () => Promise.resolve(account),
        loadUserEmail: () => Promise.resolve('ima.dacosta@aquatiq.com'),
        log: silent,
      },
    );
    expect(outcome).toBe('sent');
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe(
      'http://integration-api:3026/internal/providers/microsoft/sign-in-handoff',
    );
    expect(init.method).toBe('POST');
    expect((init.headers as Record<string, string>)['X-Internal-API-Key']).toBe(
      env.INTERNAL_API_KEY,
    );
    const body = JSON.parse(init.body as string) as Record<string, unknown>;
    expect(body.organizationId).toBe('org-1');
    expect(body.accessToken).toBe('graph-access-token');
    expect(body.tokenRef).toBe(account.tokenRef);
  });

  it('honours INTEGRATION_CORE_URL and trims trailing slashes', async () => {
    const fetchMock = jest.fn().mockResolvedValue({ ok: true, status: 200 });
    await pushMicrosoftSignInHandoff(
      { userId: 'user-1', activeOrganizationId: 'org-1' },
      {
        fetch: fetchMock as unknown as typeof fetch,
        env: {
          ...env,
          INTEGRATION_CORE_URL: 'http://integration-core.internal:3026/',
        },
        now: () => now,
        loadAccount: () => Promise.resolve(account),
        loadUserEmail: () => Promise.resolve(undefined),
        log: silent,
      },
    );
    expect((fetchMock.mock.calls[0] as [string])[0]).toBe(
      'http://integration-core.internal:3026/internal/providers/microsoft/sign-in-handoff',
    );
  });

  it('never throws: rejections, transport failures and a missing key are reported, not raised', async () => {
    const rejected = jest.fn().mockResolvedValue({ ok: false, status: 502 });
    await expect(
      pushMicrosoftSignInHandoff(
        { userId: 'user-1', activeOrganizationId: 'org-1' },
        {
          fetch: rejected as unknown as typeof fetch,
          env,
          now: () => now,
          loadAccount: () => Promise.resolve(account),
          loadUserEmail: () => Promise.resolve(undefined),
          log: silent,
        },
      ),
    ).resolves.toBe('failed');

    const exploding = jest.fn().mockRejectedValue(new Error('ECONNREFUSED'));
    await expect(
      pushMicrosoftSignInHandoff(
        { userId: 'user-1', activeOrganizationId: 'org-1' },
        {
          fetch: exploding as unknown as typeof fetch,
          env,
          now: () => now,
          loadAccount: () => Promise.resolve(account),
          loadUserEmail: () => Promise.resolve(undefined),
          log: silent,
        },
      ),
    ).resolves.toBe('failed');
    // Failure logs name the user and the failure class, never the token.
    const logged = (silent.warn.mock.calls as unknown[][])
      .map((call) => String(call[0]))
      .join('\n');
    expect(logged).not.toContain('graph-access-token');

    await expect(
      pushMicrosoftSignInHandoff(
        { userId: 'user-1', activeOrganizationId: 'org-1' },
        { env: {}, log: silent },
      ),
    ).resolves.toBe('skipped:not-configured');
  });

  it('does not call integration-core when there is nothing valid to hand over', async () => {
    const fetchMock = jest.fn();
    await expect(
      pushMicrosoftSignInHandoff(
        { userId: 'user-1' },
        {
          fetch: fetchMock as unknown as typeof fetch,
          env,
          now: () => now,
          loadAccount: () => Promise.resolve(account),
          log: silent,
        },
      ),
    ).resolves.toBe('skipped:no-organization');
    await expect(
      pushMicrosoftSignInHandoff(
        { userId: 'user-1', activeOrganizationId: 'org-1' },
        {
          fetch: fetchMock as unknown as typeof fetch,
          env,
          now: () => now,
          loadAccount: () => Promise.resolve(null),
          log: silent,
        },
      ),
    ).resolves.toBe('skipped:no-microsoft-account');
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('does not query the database for a session without an organization', async () => {
    const loadAccount = jest.fn();
    await expect(
      pushMicrosoftSignInHandoff(
        { userId: 'user-1' },
        { env, loadAccount, log: silent },
      ),
    ).resolves.toBe('skipped:no-organization');
    expect(loadAccount).not.toHaveBeenCalled();
  });
});
