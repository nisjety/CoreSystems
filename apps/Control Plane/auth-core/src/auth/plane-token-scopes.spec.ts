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
});
