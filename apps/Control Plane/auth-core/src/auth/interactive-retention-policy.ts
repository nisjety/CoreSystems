/**
 * Control-Plane-owned retention posture for interactive user tokens.
 *
 * The absence of this policy is deliberately restrictive: every interactive
 * token remains ZDR. A persistent posture can be selected only by an exact
 * organization entry in managed Auth Core configuration; request bodies,
 * headers, frontend state, and service principals cannot influence it.
 */

export type InteractiveRetentionPosture = Readonly<{
  zdr: boolean;
  authority: 'interactive-org-retention-policy';
}>;

type OrganizationPolicy = Readonly<{
  posture: 'zdr' | 'persistent';
  policyEvidenceSha256?: string;
}>;

type InteractiveRetentionPolicy = Readonly<{
  version: 1;
  organizations: Readonly<Record<string, OrganizationPolicy>>;
}>;

const ORGANIZATION_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;
const SHA256 = /^[a-f0-9]{64}$/;
const DEFAULT_POSTURE: InteractiveRetentionPosture = Object.freeze({
  zdr: true,
  authority: 'interactive-org-retention-policy',
});
const PERSISTENT_POSTURE: InteractiveRetentionPosture = Object.freeze({
  zdr: false,
  authority: 'interactive-org-retention-policy',
});

export class InteractiveRetentionPolicyConfigurationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'InteractiveRetentionPolicyConfigurationError';
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function invalidPolicy(): never {
  throw new InteractiveRetentionPolicyConfigurationError(
    'AUTH_CORE_INTERACTIVE_RETENTION_POLICY_JSON is invalid',
  );
}

function parseOrganizationPolicy(value: unknown): OrganizationPolicy {
  if (!isRecord(value)) {
    return invalidPolicy();
  }
  const keys = Object.keys(value);
  if (
    keys.some((key) => key !== 'posture' && key !== 'policyEvidenceSha256') ||
    (value.posture !== 'zdr' && value.posture !== 'persistent')
  ) {
    return invalidPolicy();
  }
  if (value.posture === 'persistent') {
    if (
      typeof value.policyEvidenceSha256 !== 'string' ||
      !SHA256.test(value.policyEvidenceSha256)
    ) {
      return invalidPolicy();
    }
    return {
      posture: 'persistent',
      policyEvidenceSha256: value.policyEvidenceSha256,
    };
  }
  if (value.policyEvidenceSha256 !== undefined) {
    return invalidPolicy();
  }
  return { posture: 'zdr' };
}

function parsePolicy(raw: string): InteractiveRetentionPolicy {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return invalidPolicy();
  }
  if (
    !isRecord(value) ||
    value.version !== 1 ||
    !isRecord(value.organizations) ||
    Object.keys(value).some(
      (key) => key !== 'version' && key !== 'organizations',
    )
  ) {
    return invalidPolicy();
  }

  const organizations: Record<string, OrganizationPolicy> = {};
  for (const [organizationId, entry] of Object.entries(value.organizations)) {
    if (!ORGANIZATION_ID.test(organizationId)) {
      return invalidPolicy();
    }
    organizations[organizationId] = parseOrganizationPolicy(entry);
  }
  return { version: 1, organizations };
}

/**
 * Resolve the effective interactive token posture from managed Auth Core
 * configuration. Empty configuration is a deliberate all-ZDR default; a
 * non-empty malformed policy is an availability error rather than a fallback.
 */
export function resolveInteractiveRetentionPosture(
  rawPolicy: string | undefined,
  organizationId: string,
): InteractiveRetentionPosture {
  if (!ORGANIZATION_ID.test(organizationId)) {
    return invalidPolicy();
  }
  if (!rawPolicy?.trim()) {
    return DEFAULT_POSTURE;
  }
  const policy = parsePolicy(rawPolicy);
  return policy.organizations[organizationId]?.posture === 'persistent'
    ? PERSISTENT_POSTURE
    : DEFAULT_POSTURE;
}

/** Resolve the process-owned interactive posture without accepting user input. */
export function currentInteractiveRetentionPosture(
  organizationId: string,
): InteractiveRetentionPosture {
  return resolveInteractiveRetentionPosture(
    process.env.AUTH_CORE_INTERACTIVE_RETENTION_POLICY_JSON,
    organizationId,
  );
}
