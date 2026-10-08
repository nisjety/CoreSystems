import {
  chmodSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  authorizePlaneServicePrincipal,
  authorizePlaneServiceTokenExchange,
  loadPlaneServicePrincipalRegistry,
} from './plane-service-principal';

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
      retention: 'zdr',
      zdr: true,
      orgBinding: 'fixed',
    });
  });

  it('derives persistent posture only from exact deployment-owned audience policy', () => {
    const persistenceAuthorized = JSON.stringify({
      'ingestion-writer': {
        credential: 'test-only-ingestion-key',
        audiences: ['data-plane'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['documents:write', 'org:data:write_all'],
        retentionByAudience: {
          'data-plane': 'persistent',
        },
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
      }),
    ).toMatchObject({ zdr: false, orgId: 'org-a' });

    const callerDowngrade = {
      serviceId: 'graph-worker',
      credential: 'test-only-caller-key',
      audience: 'data-plane',
      orgId: 'org-a',
      requestedScopes: ['graph:read'],
      reason: 'attempt persistence downgrade',
      zdr: false,
    };
    expect(
      authorizePlaneServicePrincipal(configured, callerDowngrade),
    ).toMatchObject({ zdr: true, orgId: 'org-a' });
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

  it('allows the Space binding scope only for an explicitly registered service principal', () => {
    const binderRegistry = JSON.stringify({
      'application-space-binder': {
        credential: 'test-only-space-binder-key',
        audiences: ['data-plane'],
        orgIds: ['org-a'],
        scopes: ['data:space-binding:write'],
        scopesByAudience: {
          'data-plane': ['data:space-binding:write'],
        },
      },
    });

    expect(
      authorizePlaneServicePrincipal(binderRegistry, {
        serviceId: 'application-space-binder',
        credential: 'test-only-space-binder-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['data:space-binding:write'],
        reason: 'provision approved Space retrieval binding',
      }),
    ).toMatchObject({
      subject: 'service:application-space-binder',
      scopes: ['data:space-binding:write'],
    });

    expect(() =>
      authorizePlaneServicePrincipal(binderRegistry, {
        serviceId: 'application-space-binder',
        credential: 'test-only-space-binder-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['data:admin'],
        reason: 'attempt broader Data authority',
      }),
    ).toThrow();
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

  it.each([
    { allowPersistentData: true },
    {
      retentionByAudience: {
        'data-plane': 'persistent',
        unknown: 'persistent',
      },
    },
    { retentionByAudience: {} },
    { retentionByAudience: { 'data-plane': 'sometimes' } },
  ])('rejects ambiguous retention policy: %o', (override) => {
    const malformed = JSON.stringify({
      'graph-worker': {
        credential: 'test-only-caller-key',
        audiences: ['data-plane'],
        orgIds: ['org-a'],
        scopes: ['graph:read'],
        ...override,
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

describe('tenant retention posture in the principal policy (the org decides; the controllers resolve it at mint time)', () => {
  const tenantRegistry = JSON.stringify({
    'background-worker': {
      credential: 'test-only-tenant-key',
      audiences: ['inference-core', 'data-plane'],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ['inference:invoke', 'graph:read'],
      scopesByAudience: {
        'inference-core': ['inference:invoke'],
        'data-plane': ['graph:read'],
      },
      retentionByAudience: {
        'inference-core': 'tenant',
        'data-plane': 'persistent',
      },
    },
  });
  const tenantRequest = {
    serviceId: 'background-worker',
    credential: 'test-only-tenant-key',
    audience: 'inference-core',
    orgId: 'org-a',
    requestedScopes: ['inference:invoke'],
    reason: 'contextualise indexed chunks',
  };

  it('accepts tenant as a per-audience retention posture and reports it as the principal retention', () => {
    expect(
      authorizePlaneServicePrincipal(tenantRegistry, tenantRequest),
    ).toMatchObject({ retention: 'tenant', orgId: 'org-a' });
  });

  it('never reports zdr:false for a tenant principal: the policy alone cannot know the org, so the safe value stands until a caller resolves it', () => {
    // The old `zdr: retentionPosture === 'zdr'` read a 'tenant' value as
    // persistent. A controller that forgot to resolve would then have minted a
    // non-zero-retention token for every org, ZDR orgs included.
    expect(
      authorizePlaneServicePrincipal(tenantRegistry, tenantRequest).zdr,
    ).toBe(true);
  });

  it('keeps every other audience of the same principal on its own posture', () => {
    expect(
      authorizePlaneServicePrincipal(tenantRegistry, {
        ...tenantRequest,
        audience: 'data-plane',
        requestedScopes: ['graph:read'],
      }),
    ).toMatchObject({ retention: 'persistent', zdr: false });
  });

  it('reports the registry retention for zero-retention, persistent and unconfigured audiences', () => {
    const registry = JSON.stringify({
      'background-worker': {
        credential: 'test-only-tenant-key',
        audiences: ['inference-core', 'data-plane', 'session-core'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['inference:invoke'],
        retentionByAudience: {
          'inference-core': 'zdr',
          'data-plane': 'persistent',
          'session-core': 'zdr',
        },
      },
      'unconfigured-worker': {
        credential: 'test-only-unconfigured-key',
        audiences: ['inference-core'],
        orgIds: ['org-a'],
        scopes: ['inference:invoke'],
      },
    });
    const mint = (serviceId: string, credential: string, audience: string) =>
      authorizePlaneServicePrincipal(registry, {
        ...tenantRequest,
        serviceId,
        credential,
        audience,
      });

    expect(
      mint('background-worker', 'test-only-tenant-key', 'inference-core'),
    ).toMatchObject({ retention: 'zdr', zdr: true });
    expect(
      mint('background-worker', 'test-only-tenant-key', 'data-plane'),
    ).toMatchObject({ retention: 'persistent', zdr: false });
    expect(
      mint(
        'unconfigured-worker',
        'test-only-unconfigured-key',
        'inference-core',
      ),
    ).toMatchObject({ retention: 'zdr', zdr: true });
  });

  it.each(['Tenant', 'TENANT', 'tenant ', 'org', 'org-policy', ''])(
    'rejects %p as a retention posture: the vocabulary is exactly zdr, persistent, tenant',
    (posture) => {
      const malformed = JSON.stringify({
        'background-worker': {
          credential: 'test-only-tenant-key',
          audiences: ['inference-core'],
          orgIds: ['org-a'],
          scopes: ['inference:invoke'],
          retentionByAudience: { 'inference-core': posture },
        },
      });

      expect(() =>
        authorizePlaneServicePrincipal(malformed, tenantRequest),
      ).toThrow();
    },
  );

  it('still lets no caller choose the posture: a body zdr field changes nothing for a tenant principal', () => {
    expect(
      authorizePlaneServicePrincipal(tenantRegistry, {
        ...tenantRequest,
        zdr: false,
      } as never),
    ).toMatchObject({ retention: 'tenant', zdr: true });
  });
});

describe('plane service-principal token exchange authorization (delegated identity §1, T-4)', () => {
  const exchangeConfigured = JSON.stringify({
    'model-execution': {
      credential: 'test-only-execution-key',
      audiences: ['ingestion'],
      orgIds: ['org-a'],
      scopes: ['integration:read'],
      tokenExchange: {
        ingestion: ['session-core'],
      },
    },
  });

  it('authorizes a token exchange only when the registry names the subject audience for that target', () => {
    expect(
      authorizePlaneServiceTokenExchange(exchangeConfigured, {
        serviceId: 'model-execution',
        credential: 'test-only-execution-key',
        audience: 'ingestion',
        orgId: 'org-a',
        requestedScopes: ['integration:read'],
        reason: 'execution-core integration:read',
        subjectAudience: 'session-core',
      }),
    ).toEqual({
      serviceId: 'model-execution',
      subject: 'service:model-execution',
      orgId: 'org-a',
      scopes: ['integration:read'],
      reason: 'execution-core integration:read',
    });
  });

  it('never carries a zdr/retention field — the exchange must source zdr from the verified subject token, not a registry default (T-5)', () => {
    const result = authorizePlaneServiceTokenExchange(exchangeConfigured, {
      serviceId: 'model-execution',
      credential: 'test-only-execution-key',
      audience: 'ingestion',
      orgId: 'org-a',
      requestedScopes: ['integration:read'],
      reason: 'execution-core integration:read',
      subjectAudience: 'session-core',
    });
    expect(result).not.toHaveProperty('zdr');
  });

  it('rejects a principal that is authorized for /internal-token but has no tokenExchange entry at all', () => {
    // configured has no tokenExchange field for `graph-worker` at all.
    expect(() =>
      authorizePlaneServiceTokenExchange(configured, {
        serviceId: 'graph-worker',
        credential: 'test-only-caller-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['graph:read'],
        reason: 'attempt exchange without opt-in',
        subjectAudience: 'session-core',
      }),
    ).toThrow(/not authorized/i);
  });

  it.each([
    { subjectAudience: 'execution-core' },
    { subjectAudience: '' },
    { audience: 'data-plane' }, // target audience has no tokenExchange entry
    { orgId: 'org-b' },
    { requestedScopes: ['integration:write'] },
    { credential: 'wrong-test-key' },
  ])('rejects an unbounded exchange request: %o', (override) => {
    expect(() =>
      authorizePlaneServiceTokenExchange(exchangeConfigured, {
        serviceId: 'model-execution',
        credential: 'test-only-execution-key',
        audience: 'ingestion',
        orgId: 'org-a',
        requestedScopes: ['integration:read'],
        reason: 'execution-core integration:read',
        subjectAudience: 'session-core',
        ...override,
      }),
    ).toThrow();
  });

  it.each([
    // Target audience not present in the principal's own `audiences` list.
    {
      tokenExchange: { 'control-policy': ['session-core'] },
    },
    // Empty allowed-subject-audiences array for a named target.
    { tokenExchange: { ingestion: [] } },
    // Not an object.
    { tokenExchange: 'session-core' },
    { tokenExchange: ['session-core'] },
  ])('rejects a malformed tokenExchange registry entry: %o', (override) => {
    const malformed = JSON.stringify({
      'model-execution': {
        credential: 'test-only-execution-key',
        audiences: ['ingestion'],
        orgIds: ['org-a'],
        scopes: ['integration:read'],
        ...override,
      },
    });

    expect(() =>
      authorizePlaneServiceTokenExchange(malformed, {
        serviceId: 'model-execution',
        credential: 'test-only-execution-key',
        audience: 'ingestion',
        orgId: 'org-a',
        requestedScopes: ['integration:read'],
        reason: 'execution-core integration:read',
        subjectAudience: 'session-core',
      }),
    ).toThrow();
  });

  it('is absent by default — no currently-registered principal gains this capability until the registry opts in', () => {
    // `configured` (the module-level fixture used throughout this file) has
    // no `tokenExchange` field on any entry — this pins that a principal
    // fully authorized for /internal-token gains ZERO new capability from
    // this feature shipping until a deployment explicitly edits the registry.
    expect(() =>
      authorizePlaneServiceTokenExchange(configured, {
        serviceId: 'graph-worker',
        credential: 'test-only-caller-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['graph:read'],
        reason: 'serve graph retrieval',
        subjectAudience: 'session-core',
      }),
    ).toThrow();
  });
});

describe('plane service-principal registry loading', () => {
  let directory: string;
  let registryFile: string;

  beforeEach(() => {
    directory = mkdtempSync(join(tmpdir(), 'plane-service-principals-'));
    registryFile = join(directory, 'registry.json');
    writeFileSync(registryFile, configured, { mode: 0o600 });
  });

  afterEach(() => {
    rmSync(directory, { recursive: true, force: true });
  });

  it('loads a private absolute registry file and preserves development JSON compatibility', () => {
    expect(
      loadPlaneServicePrincipalRegistry({
        NODE_ENV: 'development',
        PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
      }),
    ).toBe(configured);
    expect(
      loadPlaneServicePrincipalRegistry({
        NODE_ENV: 'development',
        PLANE_SERVICE_PRINCIPALS_JSON: configured,
      }),
    ).toBe(configured);
  });

  it.each(['production', 'staging', undefined])(
    'rejects unbound dynamic tenants from a %s file-backed registry',
    (runtime) => {
      const dynamicRegistry = JSON.stringify({
        'retrieval-engine': {
          credential: 'test-only-retrieval-key',
          audiences: ['inference-core'],
          orgIds: [],
          allowAnyOrg: true,
          scopes: ['inference:invoke'],
        },
      });
      writeFileSync(registryFile, dynamicRegistry, { mode: 0o600 });

      expect(() =>
        loadPlaneServicePrincipalRegistry({
          NODE_ENV: runtime,
          PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
        }),
      ).toThrow(/fixed organization allowlists/);
    },
  );

  it('requires the file-backed registry outside development and tests', () => {
    expect(() =>
      loadPlaneServicePrincipalRegistry({
        NODE_ENV: 'production',
        PLANE_SERVICE_PRINCIPALS_JSON: configured,
      }),
    ).toThrow(/FILE is required/);
    expect(() =>
      loadPlaneServicePrincipalRegistry({
        NODE_ENV: 'staging',
        PLANE_SERVICE_PRINCIPALS_JSON: configured,
      }),
    ).toThrow(/FILE is required/);
  });

  it('rejects ambiguous file and environment registries', () => {
    expect(() =>
      loadPlaneServicePrincipalRegistry({
        PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
        PLANE_SERVICE_PRINCIPALS_JSON: configured,
      }),
    ).toThrow(/both configured/);
  });

  // Windows cannot express POSIX permission/symlink hardening: chmod only
  // toggles the read-only attribute and file symlinks need a special
  // privilege, so those cases only run where the checks are enforceable.
  const posixOnly = process.platform !== 'win32';
  it.each(
    (
      [
        ['relative path', () => 'registry.json', /normalized absolute path/],
        [
          'symbolic link',
          () => {
            const link = join(directory, 'registry-link.json');
            symlinkSync(registryFile, link);
            return link;
          },
          /private regular file/,
          true,
        ],
        [
          'oversized file',
          () => {
            const oversized = join(directory, 'oversized.json');
            writeFileSync(oversized, Buffer.alloc(1024 * 1024 + 1), {
              mode: 0o600,
            });
            return oversized;
          },
          /too large/,
        ],
        [
          'unreadable file',
          () => {
            const unreadable = join(directory, 'unreadable.json');
            writeFileSync(unreadable, configured, { mode: 0o600 });
            chmodSync(unreadable, 0o000);
            return unreadable;
          },
          /private regular file/,
          true,
        ],
      ] as const
    ).filter(([, , , posixRequired]) => posixOnly || !posixRequired),
  )('rejects an unsafe %s', (_name, fileFactory, expectedError) => {
    expect(() =>
      loadPlaneServicePrincipalRegistry({
        PLANE_SERVICE_PRINCIPALS_FILE: fileFactory(),
      }),
    ).toThrow(expectedError);
  });

  (posixOnly ? it : it.skip)(
    'rejects a registry file readable by group or other users',
    () => {
      chmodSync(registryFile, 0o644);

      expect(() =>
        loadPlaneServicePrincipalRegistry({
          PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
        }),
      ).toThrow(/private/);
    },
  );
});

// Production tenancy model (plan W0-3). A principal's organizations come from
// exactly the sources below, and the production loader refuses only the legacy
// "any organization, unchecked" form:
//
//   orgIds                 a fixed list (including a pseudo-organization such as
//                          `global` that no directory holds). No lookup.
//   tenancy: 'directory'   any organization org-core's directory holds. Auth
//                          Core verifies that at mint; this module only reports
//                          that it must (`orgBinding: 'directory'`).
//   allowAnyOrg: true      development and tests only (refused in production).
describe('plane service-principal organization binding', () => {
  const directoryRegistry = (overrides: Record<string, unknown> = {}): string =>
    JSON.stringify({
      'embedding-engine': {
        credential: 'test-only-embedding-key',
        audiences: ['inference-core'],
        orgIds: [],
        tenancy: 'directory',
        scopes: ['inference:invoke'],
        ...overrides,
      },
    });

  const request = (orgId: string) => ({
    serviceId: 'embedding-engine',
    credential: 'test-only-embedding-key',
    audience: 'inference-core',
    orgId,
    requestedScopes: ['inference:invoke'],
    reason: 'embed organization documents',
  });

  it('accepts a directory-bound principal for any organization and says the directory must vouch for it', () => {
    for (const orgId of ['org-a', 'org-b']) {
      expect(
        authorizePlaneServicePrincipal(directoryRegistry(), request(orgId)),
      ).toMatchObject({ orgId, orgBinding: 'directory' });
    }
  });

  it('binds a fixed organization without a lookup even when the row is also directory-bound', () => {
    // `global` is a pseudo-organization: no directory holds it, so it can only
    // ever be a fixed entry.
    const registry = directoryRegistry({ orgIds: ['global'] });
    expect(
      authorizePlaneServicePrincipal(registry, request('global')),
    ).toMatchObject({ orgId: 'global', orgBinding: 'fixed' });
    expect(
      authorizePlaneServicePrincipal(registry, request('org-a')),
    ).toMatchObject({ orgId: 'org-a', orgBinding: 'directory' });
  });

  it('keeps a fixed-list principal fixed: an organization outside the list is refused', () => {
    const registry = directoryRegistry({
      tenancy: undefined,
      orgIds: ['org-a'],
    });
    expect(
      authorizePlaneServicePrincipal(registry, request('org-a')),
    ).toMatchObject({ orgBinding: 'fixed' });
    expect(() =>
      authorizePlaneServicePrincipal(registry, request('org-b')),
    ).toThrow(/not authorized/i);
  });

  it('reports the legacy allowAnyOrg form as unchecked so audit can count it', () => {
    const registry = directoryRegistry({
      tenancy: undefined,
      allowAnyOrg: true,
    });
    expect(
      authorizePlaneServicePrincipal(registry, request('org-a')),
    ).toMatchObject({ orgBinding: 'any' });
  });

  it('holds a row that carries both the directory and the unchecked form to the directory, never to the unchecked form', () => {
    // Only a development or test registry can carry both (the production loader
    // refuses allowAnyOrg outright). The stricter binding must win, or a leftover
    // allowAnyOrg beside tenancy would silently switch the lookup off.
    const registry = directoryRegistry({ allowAnyOrg: true });
    expect(
      authorizePlaneServicePrincipal(registry, request('org-a')),
    ).toMatchObject({ orgId: 'org-a', orgBinding: 'directory' });
  });

  it('refuses a principal that names no organization source at all', () => {
    const registry = directoryRegistry({ tenancy: undefined });
    expect(() =>
      authorizePlaneServicePrincipal(registry, request('org-a')),
    ).toThrow(/invalid principal policy/i);
  });

  it.each(['any', 'global', '', 'Directory', true, null, ['directory']])(
    'rejects a tenancy value other than "directory", even beside a valid fixed list: %p',
    (tenancy) => {
      // The fixed list is a valid organization source on its own, so the only
      // thing wrong with this row is the tenancy value.
      expect(() =>
        authorizePlaneServicePrincipal(
          directoryRegistry({ tenancy, orgIds: ['org-a'] }),
          request('org-a'),
        ),
      ).toThrow(/invalid principal policy/i);
    },
  );

  it('lets a directory-bound principal use token exchange for the organization its verified subject proves', () => {
    const registry = directoryRegistry({
      audiences: ['ingestion'],
      scopes: ['integration:read'],
      tokenExchange: { ingestion: ['session-core'] },
    });
    expect(
      authorizePlaneServiceTokenExchange(registry, {
        serviceId: 'embedding-engine',
        credential: 'test-only-embedding-key',
        audience: 'ingestion',
        orgId: 'org-a',
        requestedScopes: ['integration:read'],
        reason: 'acting for a verified user',
        subjectAudience: 'session-core',
      }),
    ).toMatchObject({ orgId: 'org-a' });
  });

  describe('production loading', () => {
    let directory: string;
    let registryFile: string;

    beforeEach(() => {
      directory = mkdtempSync(join(tmpdir(), 'plane-service-principals-'));
      registryFile = join(directory, 'registry.json');
    });

    afterEach(() => {
      rmSync(directory, { recursive: true, force: true });
    });

    const load = (raw: string, runtime: string | undefined = 'production') => {
      writeFileSync(registryFile, raw, { mode: 0o600 });
      return loadPlaneServicePrincipalRegistry({
        NODE_ENV: runtime,
        PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
      });
    };

    it.each(['production', 'staging', undefined])(
      'accepts a directory-bound and a fixed-list registry in a %s file',
      (runtime) => {
        const raw = JSON.stringify({
          ...JSON.parse(directoryRegistry()),
          'corpus-seeder': {
            credential: 'test-only-corpus-seeder-key',
            audiences: ['data-plane'],
            orgIds: ['org-corpus-baseline'],
            allowAnyOrg: false,
            scopes: ['data:read'],
          },
        });
        expect(load(raw, runtime)).toBe(raw);
      },
    );

    it('still refuses the legacy form, and a row that carries both forms', () => {
      expect(() =>
        load(directoryRegistry({ tenancy: undefined, allowAnyOrg: true })),
      ).toThrow(/fixed organization allowlists/);
      // A directory-bound row must not smuggle the unchecked form in beside it.
      expect(() => load(directoryRegistry({ allowAnyOrg: true }))).toThrow(
        /fixed organization allowlists/,
      );
    });

    it('accepts the same directory-bound registry in development, where nothing changes', () => {
      const raw = directoryRegistry();
      expect(load(raw, 'development')).toBe(raw);
      expect(load(raw, 'test')).toBe(raw);
    });
  });
});
