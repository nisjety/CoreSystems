import { authorizePlaneServicePrincipal } from './plane-service-principal';

const configured = JSON.stringify({
  'graph-worker': {
    credential: 'test-only-caller-key',
    audiences: ['data-plane'],
    orgIds: ['org-a'],
    scopes: ['graph:read'],
  },
});

describe('plane service-principal issuance policy', () => {
  it('binds caller, audience, tenant, scope, and reason from an allowlist', () => {
    expect(
      authorizePlaneServicePrincipal(configured, {
        serviceId: 'graph-worker',
        credential: 'test-only-caller-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['graph:read'],
        reason: 'serve graph retrieval',
      }),
    ).toEqual({
      serviceId: 'graph-worker',
      subject: 'service:graph-worker',
      orgId: 'org-a',
      scopes: ['graph:read'],
      reason: 'serve graph retrieval',
      zdr: true,
    });
  });

  it('allows persistent posture only for an explicitly authorized principal', () => {
    const persistenceAuthorized = JSON.stringify({
      'ingestion-writer': {
        credential: 'test-only-ingestion-key',
        audiences: ['data-plane'],
        orgIds: [],
        allowAnyOrg: true,
        allowPersistentData: true,
        scopes: ['documents:write', 'org:data:write_all'],
      },
    });

    expect(
      authorizePlaneServicePrincipal(persistenceAuthorized, {
        serviceId: 'ingestion-writer',
        credential: 'test-only-ingestion-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['documents:write', 'org:data:write_all'],
        reason: 'persist approved ingestion fixture',
        zdr: false,
      }),
    ).toMatchObject({ zdr: false, orgId: 'org-a' });

    expect(() =>
      authorizePlaneServicePrincipal(configured, {
        serviceId: 'graph-worker',
        credential: 'test-only-caller-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['graph:read'],
        reason: 'attempt persistence downgrade',
        zdr: false,
      }),
    ).toThrow();
  });

  it.each([
    { orgId: 'org-b' },
    { requestedScopes: ['data:search:rebuild:global'] },
    { audience: 'model-plane' },
    { credential: 'wrong-test-key' },
    { reason: '' },
  ])('rejects an unbounded or ambiguous request: %o', (override) => {
    expect(() =>
      authorizePlaneServicePrincipal(configured, {
        serviceId: 'graph-worker',
        credential: 'test-only-caller-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['graph:read'],
        reason: 'serve graph retrieval',
        ...override,
      }),
    ).toThrow();
  });

  it('allows an explicitly deployment-authorized cross-tenant worker', () => {
    const crossTenant = JSON.stringify({
      'retrieval-engine': {
        credential: 'test-only-retrieval-key',
        audiences: ['control-policy'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['data:authorization:decide'],
        scopesByAudience: {
          'control-policy': ['data:authorization:decide'],
        },
      },
    });

    expect(
      authorizePlaneServicePrincipal(crossTenant, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        orgId: 'org-created-after-deploy',
        requestedScopes: ['data:authorization:decide'],
        reason: 'authorize retrieval request',
      }),
    ).toMatchObject({
      serviceId: 'retrieval-engine',
      orgId: 'org-created-after-deploy',
      scopes: ['data:authorization:decide'],
    });
  });

  it('selects exact scopes by audience for retrieval and graph inference', () => {
    const multiAudience = JSON.stringify({
      'retrieval-engine': {
        credential: 'test-only-retrieval-key',
        audiences: ['control-policy', 'inference-core'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['data:authorization:decide', 'inference:invoke'],
        scopesByAudience: {
          'control-policy': ['data:authorization:decide'],
          'inference-core': ['inference:invoke'],
        },
      },
      'graph-index': {
        credential: 'test-only-graph-index-key',
        audiences: ['inference-core'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['inference:invoke'],
        scopesByAudience: {
          'inference-core': ['inference:invoke'],
        },
      },
    });

    expect(
      authorizePlaneServicePrincipal(multiAudience, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        orgId: 'org-a',
        requestedScopes: ['data:authorization:decide'],
        reason: 'authorize retrieval request',
      }).scopes,
    ).toEqual(['data:authorization:decide']);
    expect(
      authorizePlaneServicePrincipal(multiAudience, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'inference-core',
        orgId: 'org-a',
        requestedScopes: ['inference:invoke'],
        reason: 'embed retrieval query',
      }).scopes,
    ).toEqual(['inference:invoke']);
    expect(
      authorizePlaneServicePrincipal(multiAudience, {
        serviceId: 'graph-index',
        credential: 'test-only-graph-index-key',
        audience: 'inference-core',
        orgId: 'org-a',
        requestedScopes: ['inference:invoke'],
        reason: 'extract graph entities',
      }).scopes,
    ).toEqual(['inference:invoke']);

    for (const request of [
      {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        requestedScopes: ['inference:invoke'],
      },
      {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'inference-core',
        requestedScopes: ['data:authorization:decide'],
      },
      {
        serviceId: 'graph-index',
        credential: 'test-only-graph-index-key',
        audience: 'inference-core',
        requestedScopes: ['data:authorization:decide'],
      },
    ]) {
      expect(() =>
        authorizePlaneServicePrincipal(multiAudience, {
          ...request,
          orgId: 'org-a',
          reason: 'attempt cross-audience authority',
        }),
      ).toThrow();
    }
  });

  it.each([
    {
      scopesByAudience: {
        'control-policy': ['data:authorization:decide'],
      },
    },
    {
      scopesByAudience: {
        'control-policy': ['data:authorization:decide'],
        'inference-core': ['inference:invoke'],
        unknown: ['inference:invoke'],
      },
    },
    {
      scopesByAudience: {
        'control-policy': ['data:authorization:decide'],
        'inference-core': ['inference:admin'],
      },
    },
  ])('rejects malformed per-audience scope mappings: %o', (override) => {
    const malformed = JSON.stringify({
      'retrieval-engine': {
        credential: 'test-only-retrieval-key',
        audiences: ['control-policy', 'inference-core'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['data:authorization:decide', 'inference:invoke'],
        ...override,
      },
    });

    expect(() =>
      authorizePlaneServicePrincipal(malformed, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        orgId: 'org-a',
        requestedScopes: ['data:authorization:decide'],
        reason: 'authorize retrieval request',
      }),
    ).toThrow();
  });

  it('rejects malformed cross-tenant authorization policy', () => {
    const malformed = JSON.stringify({
      'retrieval-engine': {
        credential: 'test-only-retrieval-key',
        audiences: ['control-policy'],
        orgIds: [],
        allowAnyOrg: 'yes',
        scopes: ['data:authorization:decide'],
      },
    });

    expect(() =>
      authorizePlaneServicePrincipal(malformed, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        orgId: 'org-a',
        requestedScopes: ['data:authorization:decide'],
        reason: 'authorize retrieval request',
      }),
    ).toThrow();
  });

  it('rejects a malformed persistent-data authorization policy', () => {
    const malformed = JSON.stringify({
      'graph-worker': {
        credential: 'test-only-caller-key',
        audiences: ['data-plane'],
        orgIds: ['org-a'],
        scopes: ['graph:read'],
        allowPersistentData: 'yes',
      },
    });

    expect(() =>
      authorizePlaneServicePrincipal(malformed, {
        serviceId: 'graph-worker',
        credential: 'test-only-caller-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['graph:read'],
        reason: 'serve graph retrieval',
      }),
    ).toThrow();
  });
});
