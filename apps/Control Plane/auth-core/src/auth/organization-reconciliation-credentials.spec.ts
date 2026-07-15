import { validateOrganizationReconciliationCredentials } from './control-service-credentials';

describe('organization reconciliation credential startup policy', () => {
  const originalEnvironment = { ...process.env };

  afterEach(() => {
    process.env = { ...originalEnvironment };
  });

  it('accepts distinct non-placeholder Org and Billing credentials', () => {
    process.env.ORG_CORE_SERVICE_TOKEN =
      '  org-generated-secret-value-at-least-32-bytes  ';
    process.env.BILLING_CORE_SERVICE_TOKEN =
      'billing-generated-secret-value-at-least-32-bytes';

    expect(() => validateOrganizationReconciliationCredentials()).not.toThrow();
  });

  it.each([
    ['test', 'test-generated-secret-value-at-least-32-bytes'],
    ['placeholder', 'placeholder-generated-secret-at-least-32-bytes'],
    ['change-me', 'change-me-generated-secret-at-least-32-bytes'],
    ['replace-with', 'replace-with-generated-secret-at-least-32-bytes'],
  ])('rejects the %s token family before startup', (_family, unsafeToken) => {
    process.env.ORG_CORE_SERVICE_TOKEN = unsafeToken;
    process.env.BILLING_CORE_SERVICE_TOKEN =
      'billing-generated-secret-value-at-least-32-bytes';

    expect(() => validateOrganizationReconciliationCredentials()).toThrow(
      'ORG_CORE_SERVICE_TOKEN',
    );
  });

  it('rejects a missing Org token and a short Billing token', () => {
    delete process.env.ORG_CORE_SERVICE_TOKEN;
    process.env.BILLING_CORE_SERVICE_TOKEN =
      'billing-generated-secret-value-at-least-32-bytes';
    expect(() => validateOrganizationReconciliationCredentials()).toThrow(
      'ORG_CORE_SERVICE_TOKEN',
    );

    process.env.ORG_CORE_SERVICE_TOKEN =
      'org-generated-secret-value-at-least-32-bytes';
    process.env.BILLING_CORE_SERVICE_TOKEN = 'short';
    expect(() => validateOrganizationReconciliationCredentials()).toThrow(
      'BILLING_CORE_SERVICE_TOKEN',
    );
  });

  it('rejects credential reuse across audiences and legacy keys', () => {
    const scoped = 'generated-secret-value-at-least-32-bytes';
    process.env.ORG_CORE_SERVICE_TOKEN = scoped;
    process.env.BILLING_CORE_SERVICE_TOKEN = scoped;
    expect(() => validateOrganizationReconciliationCredentials()).toThrow(
      /distinct|reuse/i,
    );

    process.env.BILLING_CORE_SERVICE_TOKEN =
      'billing-generated-secret-value-at-least-32-bytes';
    process.env.INTERNAL_API_KEY = scoped;
    expect(() => validateOrganizationReconciliationCredentials()).toThrow(
      /distinct|reuse/i,
    );

    delete process.env.INTERNAL_API_KEY;
    process.env.INTERNAL_SERVICE_SECRET =
      'billing-generated-secret-value-at-least-32-bytes';
    expect(() => validateOrganizationReconciliationCredentials()).toThrow(
      /distinct|reuse/i,
    );
  });
});
