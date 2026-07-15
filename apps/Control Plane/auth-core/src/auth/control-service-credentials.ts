const placeholderPrefixes = [
  'test',
  'placeholder',
  'change-me',
  'replace-with',
] as const;

export function requiredScopedServiceToken(name: string): string {
  const token = process.env[name]?.trim() || '';
  const lower = token.toLowerCase();
  if (
    token.length < 32 ||
    placeholderPrefixes.some((prefix) => lower.startsWith(prefix))
  ) {
    throw new Error(`${name} must be a non-placeholder scoped credential`);
  }
  return token;
}

export function validateOrganizationReconciliationCredentials(): void {
  const scoped = [
    [
      'ORG_CORE_SERVICE_TOKEN',
      requiredScopedServiceToken('ORG_CORE_SERVICE_TOKEN'),
    ],
    [
      'BILLING_CORE_SERVICE_TOKEN',
      requiredScopedServiceToken('BILLING_CORE_SERVICE_TOKEN'),
    ],
  ] as const;
  const seen = new Map<string, string>();
  for (const [name, raw] of [
    ['INTERNAL_API_KEY', process.env.INTERNAL_API_KEY],
    ['INTERNAL_SERVICE_SECRET', process.env.INTERNAL_SERVICE_SECRET],
  ] as const) {
    const value = raw?.trim();
    if (value) seen.set(value, name);
  }
  for (const [name, value] of scoped) {
    const reusedFrom = seen.get(value);
    if (reusedFrom) {
      throw new Error(`${name} must not reuse ${reusedFrom}`);
    }
    seen.set(value, name);
  }
}
