import {
  API_KEY_AGENT_MAX_EXPIRY_SECONDS,
  API_KEY_REVOKED_SUBJECT,
  DEFAULT_API_KEY_BUDGET_CEILING,
  apiKeyAgentExchangeEnabled,
  apiKeyHoldsBrowserAgent,
  apiKeyRevocationTarget,
  browserAgentPermissions,
  buildApiKeyRevokedEvent,
  canGrantBrowserAgent,
  normalizeApiKeyHost,
  normalizeApiKeyHostCeiling,
  plainScopesRefusal,
  publishApiKeyRevoked,
  readApiKeyAgentMetadata,
  revocationForUpdate,
  stripAgentMetadata,
  sweepApiKeyRevocations,
  touchesReservedAgentMetadata,
  validateApiKeyAgentRequest,
  withoutBrowserAgentPermission,
  type ApiKeyAgentRequest,
  type ApiKeyRevocationRow,
} from './api-key-metadata';

// Same pattern DirectNatsService.publishAuditDurable enforces.
const AUDIT_EVENT_ID = /^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$/;
const AUDIT_SUBJECT = /^verevon\.audit\.v2\.control\.auth-core\.([a-z0-9_]+)$/;

const NOW = new Date('2026-11-03T12:00:00.000Z');
const DAY = 24 * 60 * 60 * 1000;

function request(overrides: Partial<ApiKeyAgentRequest> = {}): ApiKeyAgentRequest {
  return {
    organizationId: 'org_1',
    hosts: ['Example.com', 'docs.example.com'],
    ...overrides,
  };
}

const admin = { userId: 'user_admin', orgRole: 'admin' };
const enabled = { exchangeEnabled: true, now: NOW };

function agentRow(overrides: Partial<ApiKeyRevocationRow> = {}): ApiKeyRevocationRow {
  return {
    id: 'key_1',
    configId: 'user-keys',
    referenceId: 'user_admin',
    enabled: true,
    expiresAt: new Date(NOW.getTime() + 30 * DAY),
    updatedAt: new Date('2026-11-01T00:00:00.000Z'),
    permissions: JSON.stringify({ api: ['browser:agent'] }),
    metadata: JSON.stringify({
      metadata_version: 1,
      org_id: 'org_1',
      key_hosts: ['example.com'],
      budget_ceiling: { micro_usd: 1_000_000, max_steps: 50, max_runtime_s: 600 },
      zdr: false,
      browser_agent_granted_by: 'user_admin',
    }),
    ...overrides,
  };
}

describe('INNH-10 CP-2 browser:agent key creation validation matrix', () => {
  it('accepts an admin request and normalises the ceiling', () => {
    const result = validateApiKeyAgentRequest(request(), admin, enabled);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.metadata).toEqual({
      metadata_version: 1,
      org_id: 'org_1',
      key_hosts: ['docs.example.com', 'example.com'],
      budget_ceiling: { ...DEFAULT_API_KEY_BUDGET_CEILING },
      zdr: false,
      browser_agent_granted_by: 'user_admin',
    });
    expect(result.value.expiresIn).toBe(API_KEY_AGENT_MAX_EXPIRY_SECONDS);
  });

  it('accepts an owner and keeps the zdr flag', () => {
    const result = validateApiKeyAgentRequest(
      request({ zdr: true }),
      { userId: 'user_owner', orgRole: 'owner' },
      enabled,
    );
    expect(result.ok && result.value.metadata.zdr).toBe(true);
  });

  it.each<[string, Partial<ApiKeyAgentRequest>, { orgRole: string | null } | null, boolean, string]>([
    ['exchange flag off', {}, null, false, 'api_key_agent_exchange_disabled'],
    ['no org', { organizationId: undefined }, null, true, 'api_key_org_required'],
    ['blank org', { organizationId: '  ' }, null, true, 'api_key_org_required'],
    ['plain member', {}, { orgRole: 'member' }, true, 'api_key_browser_agent_forbidden'],
    ['not a member', {}, { orgRole: null }, true, 'api_key_browser_agent_forbidden'],
    ['no hosts', { hosts: [] }, null, true, 'api_key_hosts_required'],
    ['hosts missing', { hosts: undefined }, null, true, 'api_key_hosts_required'],
    ['wildcard host', { hosts: ['*.example.com'] }, null, true, 'api_key_host_wildcard_refused'],
    ['scheme in host', { hosts: ['https://example.com'] }, null, true, 'api_key_host_invalid'],
    ['port in host', { hosts: ['example.com:8443'] }, null, true, 'api_key_host_invalid'],
    ['path in host', { hosts: ['example.com/a'] }, null, true, 'api_key_host_invalid'],
    ['ipv4 host', { hosts: ['10.0.0.1'] }, null, true, 'api_key_host_invalid'],
    ['single-label host', { hosts: ['localhost'] }, null, true, 'api_key_host_invalid'],
    ['too many hosts', { hosts: Array.from({ length: 33 }, (_, i) => `h${i}.example.com`) }, null, true, 'api_key_hosts_too_many'],
    ['zero budget', { budget: { microUsd: 0 } }, null, true, 'api_key_budget_invalid'],
    ['budget above limit', { budget: { maxRuntimeSeconds: 901 } }, null, true, 'api_key_budget_invalid'],
    ['fractional steps', { budget: { maxSteps: 1.5 } }, null, true, 'api_key_budget_invalid'],
    ['expiry beyond 90 days', { expiresAt: new Date(NOW.getTime() + 91 * DAY) }, null, true, 'api_key_expiry_too_long'],
    ['expiry in the past', { expiresAt: new Date(NOW.getTime() - DAY) }, null, true, 'api_key_expiry_too_short'],
  ])('refuses %s', (_name, overrides, actorOverride, exchangeEnabled, reason) => {
    const result = validateApiKeyAgentRequest(
      request(overrides),
      { ...admin, ...(actorOverride ?? {}) },
      { exchangeEnabled, now: NOW },
    );
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.reason).toBe(reason);
  });

  it('accepts exactly 32 hosts after de-duplication', () => {
    const hosts = Array.from({ length: 32 }, (_, i) => `h${i}.example.com`);
    const result = normalizeApiKeyHostCeiling([...hosts, 'H0.EXAMPLE.COM']);
    expect(result.ok && result.value.length).toBe(32);
  });

  it('accepts expiry of exactly 90 days', () => {
    const result = validateApiKeyAgentRequest(
      request({ expiresAt: new Date(NOW.getTime() + 90 * DAY) }),
      admin,
      enabled,
    );
    expect(result.ok && result.value.expiresIn).toBe(API_KEY_AGENT_MAX_EXPIRY_SECONDS);
  });

  it('normalises hosts to the broker canonical form', () => {
    expect(normalizeApiKeyHost(' Example.COM. ')).toEqual({ ok: true, value: 'example.com' });
    expect(normalizeApiKeyHost('bücher.example')).toEqual({
      ok: true,
      value: 'xn--bcher-kva.example',
    });
    expect(normalizeApiKeyHost('-bad.example.com').ok).toBe(false);
    expect(normalizeApiKeyHost('user@example.com').ok).toBe(false);
  });

  it('only owner or admin may grant browser:agent', () => {
    expect(canGrantBrowserAgent('owner')).toBe(true);
    expect(canGrantBrowserAgent('member,admin')).toBe(true);
    expect(canGrantBrowserAgent('member')).toBe(false);
    expect(canGrantBrowserAgent(null)).toBe(false);
  });

  it('browser:agent is unavailable unless both API_KEY_ENABLED and the exchange flag are on', () => {
    expect(apiKeyAgentExchangeEnabled({})).toBe(false);
    expect(apiKeyAgentExchangeEnabled({ API_KEY_ENABLED: 'true' })).toBe(false);
    expect(
      apiKeyAgentExchangeEnabled({ API_KEY_AGENT_EXCHANGE_ENABLED: 'true' }),
    ).toBe(false);
    expect(
      apiKeyAgentExchangeEnabled({
        API_KEY_ENABLED: 'true',
        API_KEY_AGENT_EXCHANGE_ENABLED: 'true',
      }),
    ).toBe(true);
  });

  it('refuses browser:agent in plain scopes (it needs the metadata path)', () => {
    expect(plainScopesRefusal(['read', 'browser:agent'])).toMatchObject({
      ok: false,
      reason: 'api_key_browser_agent_requires_metadata',
    });
    expect(plainScopesRefusal(['read'])).toBeNull();
  });
});

describe('INNH-10 CP-2 stored metadata and permissions', () => {
  it('round-trips validated metadata, including double-encoded JSON', () => {
    const result = validateApiKeyAgentRequest(request(), admin, enabled);
    if (!result.ok) throw new Error('expected ok');
    const stored = JSON.stringify(JSON.stringify({ description: 'd', ...result.value.metadata }));
    expect(readApiKeyAgentMetadata(stored)).toEqual(result.value.metadata);
  });

  it('fails closed on hand-edited metadata', () => {
    const base = JSON.parse(agentRow().metadata as string) as Record<string, unknown>;
    expect(readApiKeyAgentMetadata({ ...base, key_hosts: ['*.example.com'] })).toBeNull();
    expect(readApiKeyAgentMetadata({ ...base, key_hosts: ['b.example.com', 'a.example.com'] })).toBeNull();
    expect(readApiKeyAgentMetadata({ ...base, budget_ceiling: { micro_usd: 1 } })).toBeNull();
    expect(readApiKeyAgentMetadata({ ...base, zdr: 'yes' })).toBeNull();
    expect(readApiKeyAgentMetadata({ ...base, metadata_version: 2 })).toBeNull();
    expect(readApiKeyAgentMetadata({ ...base, org_id: '' })).toBeNull();
    expect(readApiKeyAgentMetadata('not json')).toBeNull();
  });

  it('detects client writes of auth-core-only fields', () => {
    expect(touchesReservedAgentMetadata({ description: 'x' })).toBe(false);
    expect(touchesReservedAgentMetadata({ org_id: 'org_2' })).toBe(true);
    expect(touchesReservedAgentMetadata(JSON.stringify({ key_hosts: [] }))).toBe(true);
    expect(stripAgentMetadata(agentRow().metadata)).toEqual({});
  });

  it('adds and removes the browser:agent permission', () => {
    expect(browserAgentPermissions([])).toEqual({ api: ['browser:agent'] });
    expect(browserAgentPermissions(['read', 'browser:agent'])).toEqual({
      api: ['read', 'browser:agent'],
    });
    expect(apiKeyHoldsBrowserAgent(JSON.stringify({ api: ['browser:agent'] }))).toBe(true);
    expect(
      withoutBrowserAgentPermission(JSON.stringify({ api: ['read', 'browser:agent'] })),
    ).toEqual({ api: ['read'] });
  });
});

describe('INNH-10 CP-3 api_key_revoked event', () => {
  it('builds the durable payload with details {key_id, org_id, reason}', () => {
    const target = apiKeyRevocationTarget(agentRow());
    expect(target).toEqual({ keyId: 'key_1', orgId: 'org_1', ownerUserId: 'user_admin' });
    const payload = buildApiKeyRevokedEvent({
      target: target!,
      reason: 'deleted',
      occurredAt: NOW,
      actorUserId: 'user_admin',
    });
    expect(payload).toMatchObject({
      occurred_at: NOW.toISOString(),
      event_id: 'api_key:key_1:revoked:deleted',
      org_id: 'org_1',
      user_id: 'user_admin',
      plane: 'control',
      producer: 'auth-core',
      event: 'api_key_revoked',
      resource_id: 'key_1',
      outcome: 'ok',
      details: { key_id: 'key_1', org_id: 'org_1', reason: 'deleted' },
    });
    expect(payload.event_id).toMatch(AUDIT_EVENT_ID);
    expect(AUDIT_SUBJECT.exec(API_KEY_REVOKED_SUBJECT)?.[1]).toBe(payload.event);
  });

  it('gives repeated disables distinct event ids and sanitises odd key ids', () => {
    const target = { keyId: 'key with/space', orgId: 'org_1', ownerUserId: 'u' };
    const first = buildApiKeyRevokedEvent({
      target,
      reason: 'disabled',
      occurredAt: NOW,
      discriminator: new Date('2026-11-01T00:00:00.000Z'),
    });
    const second = buildApiKeyRevokedEvent({
      target,
      reason: 'disabled',
      occurredAt: NOW,
      discriminator: new Date('2026-11-02T00:00:00.000Z'),
    });
    expect(first.event_id).not.toBe(second.event_id);
    expect(first.event_id).toMatch(AUDIT_EVENT_ID);
    expect(first.actor_role).toBe('system');
  });

  it('emits only for org-bound browser:agent keys', () => {
    expect(apiKeyRevocationTarget(agentRow({ metadata: JSON.stringify({ description: 'x' }) }))).toBeNull();
    expect(
      apiKeyRevocationTarget(
        agentRow({ permissions: JSON.stringify({ api: ['read'] }), metadata: JSON.stringify({ org_id: 'org_1' }) }),
      ),
    ).toBeNull();
    expect(apiKeyRevocationTarget(null)).toBeNull();
  });

  it('publishes on the api_key_revoked subject and surfaces publish failures', async () => {
    const publish = jest.fn().mockResolvedValue({ stream: 'AUDIT', seq: 1 });
    const target = apiKeyRevocationTarget(agentRow())!;
    await publishApiKeyRevoked(publish, { target, reason: 'deleted', occurredAt: NOW });
    expect(publish).toHaveBeenCalledWith(
      API_KEY_REVOKED_SUBJECT,
      expect.objectContaining({ event: 'api_key_revoked' }),
    );
    const failing = jest.fn().mockRejectedValue(new Error('no PubAck'));
    await expect(
      publishApiKeyRevoked(failing, { target, reason: 'deleted', occurredAt: NOW }),
    ).rejects.toThrow('no PubAck');
  });

  it('maps updates to the disable revocation', () => {
    const row = agentRow();
    expect(revocationForUpdate(row, { enabled: false })).toBe('disabled');
    expect(revocationForUpdate(agentRow({ enabled: false }), { enabled: false })).toBeNull();
    expect(revocationForUpdate(row, { permissions: { api: ['read'] } })).toBe('disabled');
    expect(revocationForUpdate(row, { permissions: { api: ['browser:agent'] } })).toBeNull();
    expect(revocationForUpdate(row, { enabled: true })).toBeNull();
  });
});

describe('INNH-10 CP-3 expiry and owner-removal sweep', () => {
  function deps(rows: ApiKeyRevocationRow[], members: Set<string>, publishOk = true) {
    const published: Array<Record<string, unknown>> = [];
    const disabled: string[] = [];
    return {
      published,
      disabled,
      deps: {
        listCandidates: () => Promise.resolve(rows),
        isActiveMember: (userId: string, orgId: string) =>
          Promise.resolve(members.has(`${userId}@${orgId}`)),
        publish: (_subject: string, payload: Record<string, unknown>) => {
          if (!publishOk) return Promise.reject(new Error('no PubAck'));
          published.push(payload);
          return Promise.resolve({ stream: 'AUDIT', seq: published.length });
        },
        disable: (keyId: string) => {
          disabled.push(keyId);
          return Promise.resolve();
        },
        now: () => NOW,
      },
    };
  }

  it('revokes an expired key and a key whose owner left the org, then disables them', async () => {
    const expired = agentRow({ id: 'key_expired', expiresAt: new Date(NOW.getTime() - 1000) });
    const orphaned = agentRow({ id: 'key_orphan', referenceId: 'user_gone' });
    const healthy = agentRow({ id: 'key_ok' });
    const plain = agentRow({
      id: 'key_plain',
      permissions: JSON.stringify({ api: ['read'] }),
      metadata: JSON.stringify({ description: 'x' }),
      expiresAt: new Date(NOW.getTime() - 1000),
    });
    const h = deps([expired, orphaned, healthy, plain], new Set(['user_admin@org_1']));
    const result = await sweepApiKeyRevocations(h.deps);
    expect(result.revoked).toEqual([
      { keyId: 'key_expired', reason: 'expired' },
      { keyId: 'key_orphan', reason: 'owner_removed' },
    ]);
    expect(h.disabled).toEqual(['key_expired', 'key_orphan']);
    expect(h.published.map((p) => (p.details as { reason: string }).reason)).toEqual([
      'expired',
      'owner_removed',
    ]);
    for (const payload of h.published) {
      expect(payload.event_id).toMatch(AUDIT_EVENT_ID);
    }
  });

  it('does not disable a key whose event got no PubAck (retried next sweep)', async () => {
    const expired = agentRow({ expiresAt: new Date(NOW.getTime() - 1000) });
    const h = deps([expired], new Set(['user_admin@org_1']), false);
    const result = await sweepApiKeyRevocations(h.deps);
    expect(result.revoked).toEqual([]);
    expect(result.failed).toEqual([{ keyId: 'key_1', reason: 'expired' }]);
    expect(h.disabled).toEqual([]);
  });

  it('skips keys that are already disabled', async () => {
    const h = deps([agentRow({ enabled: false, expiresAt: new Date(0) })], new Set());
    const result = await sweepApiKeyRevocations(h.deps);
    expect(result.revoked).toEqual([]);
    expect(h.published).toEqual([]);
  });
});
