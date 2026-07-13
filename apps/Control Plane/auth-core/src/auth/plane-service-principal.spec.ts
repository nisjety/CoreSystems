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
    });
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
});
