import {
  API_KEY_EXCHANGE_AUDIENCE_SCOPES,
  apiKeyTokenZdr,
  apiKeyVerificationDenial,
  decideApiKeyExchange,
  isApiKeyExchangeAudience,
  type ApiKeyExchangeDenialReason,
  type VerifiedApiKey,
} from './api-key-exchange';
import type { CanonicalTokenContext } from './plane-token-membership';

const now = Date.parse('2026-11-03T12:00:00.000Z');

const key = (override: Partial<VerifiedApiKey> = {}): VerifiedApiKey => ({
  id: 'key-1',
  configId: 'user-keys',
  referenceId: 'user-a',
  enabled: true,
  expiresAt: null,
  permissions: { agent: ['runs'], browser: ['agent'] },
  metadata: { org_id: 'org-a' },
  ...override,
});

const member = { orgId: 'org-a', role: 'member' };

type ExchangeRequest = {
  audience: string;
  orgId: string;
  scopes: readonly string[];
};

type RefusalCase = {
  key?: Partial<VerifiedApiKey>;
  membership?: CanonicalTokenContext | null;
  request?: Partial<ExchangeRequest>;
};

const request = (override: Partial<ExchangeRequest> = {}): ExchangeRequest => ({
  audience: 'model-gateway',
  orgId: 'org-a',
  scopes: ['agent:runs'],
  ...override,
});

describe('API-key exchange policy', () => {
  it('grants a user key bound to the org, owned by a member, holding the audience scope', () => {
    expect(decideApiKeyExchange(key(), member, request(), now)).toEqual({
      allowed: true,
      grant: {
        audience: 'model-gateway',
        keyId: 'key-1',
        actingUserId: 'user-a',
        orgId: 'org-a',
        scopes: ['agent:runs'],
        keyZdr: false,
      },
    });
  });

  it.each<[string, ApiKeyExchangeDenialReason, RefusalCase]>([
    [
      'an unknown audience',
      'audience_not_allowed',
      { request: { audience: 'data-plane' } },
    ],
    [
      'a service-only audience',
      'audience_not_allowed',
      { request: { audience: 'control-policy' } },
    ],
    [
      'a disabled key',
      'key_disabled',
      { key: { enabled: false } },
    ],
    [
      'an expired key',
      'key_expired',
      { key: { expiresAt: new Date(now - 1) } },
    ],
    [
      'a key expiring this instant',
      'key_expired',
      { key: { expiresAt: new Date(now) } },
    ],
    [
      'an unreadable expiry',
      'key_expired',
      { key: { expiresAt: 'not-a-date' } },
    ],
    [
      'an org key',
      'org_key_refused',
      { key: { configId: 'org-keys', referenceId: 'org-a' } },
    ],
    [
      'a key of the default config',
      'org_key_refused',
      { key: { configId: 'default' } },
    ],
    [
      'a key id with a space',
      'key_invalid',
      { key: { id: 'key 1' } },
    ],
    [
      'a key id over 128 bytes',
      'key_invalid',
      { key: { id: 'k'.repeat(129) } },
    ],
    [
      'an owner that is itself a key',
      'key_invalid',
      { key: { referenceId: 'apikey:other' } },
    ],
    [
      'a padded owner',
      'key_invalid',
      { key: { referenceId: ' user-a' } },
    ],
    [
      'a key bound to another org',
      'org_mismatch',
      { key: { metadata: { org_id: 'org-b' } } },
    ],
    [
      'a key with no org binding',
      'org_mismatch',
      { key: { metadata: null } },
    ],
    [
      'a non-string org binding',
      'org_mismatch',
      { key: { metadata: { org_id: ['org-a'] } } },
    ],
    [
      'an empty request org',
      'org_mismatch',
      { request: { orgId: '  ' } },
    ],
    [
      'an owner who left the org',
      'owner_not_member',
      { membership: null },
    ],
    [
      'a membership in another org',
      'owner_not_member',
      { membership: { orgId: 'org-b', role: 'owner' } },
    ],
    [
      'no requested scope',
      'scope_not_granted',
      { request: { scopes: [] } },
    ],
    [
      'a scope the key does not hold',
      'scope_not_granted',
      { key: { permissions: { browser: ['agent'] } } },
    ],
    [
      'a key with no permissions',
      'scope_not_granted',
      { key: { permissions: null } },
    ],
    [
      'a scope outside the audience table',
      'scope_not_granted',
      {
        key: { permissions: { agent: ['runs'], admin: ['all'] } },
        request: { scopes: ['agent:runs', 'admin:all'] },
      },
    ],
    [
      'the browser scope on an Agent API audience',
      'scope_not_granted',
      { request: { scopes: ['browser:agent'] } },
    ],
    [
      'a bare resource permission',
      'scope_not_granted',
      { key: { permissions: { agent: [] } } },
    ],
  ])('refuses %s', (_case, reason, refusal) => {
    const membership =
      refusal.membership === undefined ? member : refusal.membership;
    expect(
      decideApiKeyExchange(
        key(refusal.key),
        membership,
        request(refusal.request),
        now,
      ),
    ).toEqual({ allowed: false, reason });
  });

  it('accepts a key that expires later and dedupes the requested scopes', () => {
    const decision = decideApiKeyExchange(
      key({ expiresAt: new Date(now + 60_000).toISOString() }),
      member,
      request({ scopes: ['agent:runs', ' agent:runs'] }),
      now,
    );
    expect(decision).toMatchObject({
      allowed: true,
      grant: { scopes: ['agent:runs'] },
    });
  });

  it('serves the browser audiences with browser:agent only', () => {
    for (const audience of ['quarry', 'browser-broker']) {
      expect(
        decideApiKeyExchange(
          key(),
          member,
          request({ audience, scopes: ['browser:agent'] }),
          now,
        ),
      ).toMatchObject({ allowed: true, grant: { audience } });
      expect(
        decideApiKeyExchange(key(), member, request({ audience }), now),
      ).toEqual({ allowed: false, reason: 'scope_not_granted' });
    }
  });

  it('keeps the audience table closed', () => {
    expect(API_KEY_EXCHANGE_AUDIENCE_SCOPES).toEqual({
      'model-gateway': ['agent:runs'],
      'session-core': ['agent:runs'],
      'inference-core': ['agent:runs'],
      quarry: ['browser:agent'],
      'browser-broker': ['browser:agent'],
    });
    expect(isApiKeyExchangeAudience('execution-core')).toBe(false);
    expect(isApiKeyExchangeAudience('toString')).toBe(false);
    expect(isApiKeyExchangeAudience('__proto__')).toBe(false);
  });

  it('reads the key zdr flag only as the literal true', () => {
    for (const [zdr, expected] of [
      [true, true],
      ['true', false],
      [1, false],
      [undefined, false],
    ] as const) {
      expect(
        decideApiKeyExchange(
          key({ metadata: { org_id: 'org-a', zdr } }),
          member,
          request(),
          now,
        ),
      ).toMatchObject({ allowed: true, grant: { keyZdr: expected } });
    }
  });

  it('lets the key raise the org posture to zero retention, never lower it', () => {
    expect(apiKeyTokenZdr(false, false)).toBe(false);
    expect(apiKeyTokenZdr(false, true)).toBe(true);
    expect(apiKeyTokenZdr(true, false)).toBe(true);
    expect(apiKeyTokenZdr(true, true)).toBe(true);
  });

  it('maps the plugin refusal codes and hides every other one behind key_invalid', () => {
    expect(apiKeyVerificationDenial('KEY_DISABLED')).toBe('key_disabled');
    expect(apiKeyVerificationDenial('KEY_EXPIRED')).toBe('key_expired');
    expect(apiKeyVerificationDenial('RATE_LIMITED')).toBe('key_rate_limited');
    expect(apiKeyVerificationDenial('USAGE_EXCEEDED')).toBe('key_rate_limited');
    expect(apiKeyVerificationDenial('KEY_NOT_FOUND')).toBe('key_invalid');
    expect(apiKeyVerificationDenial('INVALID_API_KEY')).toBe('key_invalid');
    expect(apiKeyVerificationDenial(undefined)).toBe('key_invalid');
  });
});
