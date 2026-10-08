import {
  modelGatewayScopesForRole,
  planeScopesForRole,
} from './plane-token-scopes';

describe('plane user scope contract', () => {
  it('gives an ordinary member only user-level Data and wiki capabilities', () => {
    expect(planeScopesForRole('member')).toEqual([
      'data:read',
      'documents:read',
      'documents:write',
      'wiki.read',
      'wiki.write',
    ]);
  });

  it('adds audited tenant-wide and administrative capabilities for owners/admins', () => {
    expect(planeScopesForRole('admin')).toEqual(
      expect.arrayContaining([
        'data:admin',
        'data:orchestrate',
        'org:data:read_all',
        'org:data:write_all',
        'wiki.approve',
        'wiki.maintenance.write',
      ]),
    );
  });

  it('never grants destructive search rebuild to an interactive user role', () => {
    expect(planeScopesForRole('owner')).not.toContain('data:search:rebuild');
  });

  it('never grants Space retrieval binding authority to an interactive user', () => {
    for (const role of ['viewer', 'member', 'admin', 'owner']) {
      expect(planeScopesForRole(role, 'data-plane')).not.toContain(
        'data:space-binding:write',
      );
    }
  });

  it('grants only the exact interactive scopes required by each hardened Model service', () => {
    expect(planeScopesForRole('member', 'inference-core')).toEqual([
      'inference:invoke',
    ]);
    for (const audience of [
      'session-core',
      'execution-core',
      'cost-core',
      'letta-bridge',
      'browser-broker',
      'sandbox-manager',
      'bridge-core',
    ] as const) {
      expect(planeScopesForRole('member', audience)).toEqual([]);
    }
  });

  it.each([
    ['member', ['capability:read']],
    ['viewer', ['capability:read']],
    [null, ['capability:read']],
    ['admin', ['capability:read', 'capability:write']],
    ['owner', ['capability:read', 'capability:write']],
    [' ADMIN ', ['capability:read', 'capability:write']],
  ] as const)(
    'derives least-privilege capability scopes for role %p',
    (role, expectedScopes) => {
      const scopes = planeScopesForRole(role, 'capability-core');

      expect(scopes).toEqual(expectedScopes);
      if (role === 'member' || role === 'viewer' || role === null) {
        expect(scopes).not.toEqual(
          expect.arrayContaining([
            'capability:write',
            'capability:admin',
            'capability:global:write',
            'capability:health:write',
            'mcp:write',
            'mcp:admin',
          ]),
        );
      }
    },
  );

  it('keeps model-gateway privilege separate and role-derived', () => {
    expect(modelGatewayScopesForRole('member')).toEqual([]);
    expect(modelGatewayScopesForRole('admin')).toEqual(['admin']);
    expect(modelGatewayScopesForRole('owner')).toEqual(['admin']);
  });

  it('grants organization cost reads only to verified owners and admins', () => {
    for (const role of ['owner', 'admin', ' ADMIN ']) {
      expect(planeScopesForRole(role, 'cost-core')).toEqual(['cost:org:read']);
    }
    for (const role of ['member', 'viewer', '', null, undefined]) {
      expect(planeScopesForRole(role, 'cost-core')).toEqual([]);
    }
    expect(planeScopesForRole('owner', 'cost-core')).not.toContain(
      'cost:write',
    );
  });

  // Scopes that exist for ONE service principal each (Control Plane principal
  // bundle, plan W0-4; config/plane-service-principals.json holds each in one
  // row): the web rerank of quarry-edge, the usage forwarder of data-orchestrator
  // and the BFF's retention decision. A role-derived (interactive) token must never
  // carry any of them, whatever the role and whichever audience it is minted for;
  // an interactive caller reaching one of these routes with its own token is an
  // authority nobody granted.
  it('never derives a service-only scope for any role on any audience', () => {
    const serviceOnly = ['data:rerank', 'cost:write', 'retention:zdr-decide'];
    const audiences = [
      'data-plane',
      'quarry',
      'ingestion',
      'control-plane',
      'application-plane',
      'session-core',
      'inference-core',
      'execution-core',
      'cost-core',
      'capability-core',
      'letta-bridge',
      'browser-broker',
      'sandbox-manager',
      'bridge-core',
    ] as const;
    const roles = [
      'owner',
      'admin',
      'member',
      'viewer',
      ' ADMIN ',
      '',
      null,
      undefined,
    ];
    let combinations = 0;
    for (const audience of audiences) {
      for (const role of roles) {
        const scopes = planeScopesForRole(role, audience);
        for (const scope of serviceOnly) {
          expect({
            audience,
            role,
            scope,
            held: scopes.includes(scope),
          }).toEqual({ audience, role, scope, held: false });
        }
        combinations += 1;
      }
    }
    expect(combinations).toBe(audiences.length * roles.length);
    for (const role of roles) {
      for (const scope of serviceOnly) {
        expect(modelGatewayScopesForRole(role)).not.toContain(scope);
      }
    }
  });
});
