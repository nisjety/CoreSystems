/**
 * INNH-10 CP-2 and CP-3: browser-agent API key metadata and the durable
 * `api_key_revoked` event.
 *
 * A browser-agent key is a Better Auth `user-keys` key (OD-4) that carries the
 * `browser:agent` permission (OD-5) and the metadata below. The metadata is the
 * key's CEILING: the api-key exchange (CP-1) and the browser broker (MP-2) only
 * ever narrow it, never widen it.
 *
 * Metadata fields (stored in the Better Auth `apikey.metadata` JSON):
 *   metadata_version          1
 *   org_id                    the org the key is bound to (required)
 *   key_hosts                 host ceiling, broker-canonical, 1..32, no wildcards
 *   budget_ceiling            { micro_usd, max_steps, max_runtime_s } per run
 *   zdr                       key-level ZDR flag (raises the org posture, never lowers)
 *   browser_agent_granted_by  user id of the org admin/owner who granted browser:agent
 *
 * Everything in this file is pure (no DB, no NATS) so the validation matrix and
 * the event shape are unit-testable; `src/nats/api-key-revocation.publisher.ts`
 * wires it to Postgres and the durable audit stream.
 */
import { isIP } from 'node:net';
import { domainToASCII } from 'node:url';

export const BROWSER_AGENT_PERMISSION = 'browser:agent';
export const API_KEY_AGENT_METADATA_VERSION = 1;
export const API_KEY_AGENT_MAX_HOSTS = 32;
export const API_KEY_AGENT_MAX_EXPIRY_SECONDS = 90 * 24 * 60 * 60;
export const API_KEY_AGENT_MIN_EXPIRY_SECONDS = 24 * 60 * 60;
export const API_KEY_USER_CONFIG_ID = 'user-keys';

export const API_KEY_REVOKED_EVENT = 'api_key_revoked';
export const API_KEY_REVOKED_SUBJECT = `verevon.audit.v2.control.auth-core.${API_KEY_REVOKED_EVENT}`;

export interface ApiKeyAgentBudgetCeiling {
  micro_usd: number;
  max_steps: number;
  max_runtime_s: number;
}

/** OD-10 defaults: 1 USD, 50 steps, 600 s per run. */
export const DEFAULT_API_KEY_BUDGET_CEILING: Readonly<ApiKeyAgentBudgetCeiling> =
  Object.freeze({
    micro_usd: 1_000_000,
    max_steps: 50,
    max_runtime_s: 600,
  });

/**
 * Absolute bounds a key creator may set. max_runtime_s can never exceed the
 * key-grant TTL maximum (OD-9: 900 s). The org-plan cap is not modelled yet.
 */
export const API_KEY_BUDGET_CEILING_LIMITS: Readonly<ApiKeyAgentBudgetCeiling> =
  Object.freeze({
    micro_usd: 10_000_000,
    max_steps: 500,
    max_runtime_s: 900,
  });

/** Metadata fields only auth-core itself may write (never a client request). */
export const RESERVED_API_KEY_AGENT_METADATA_FIELDS = Object.freeze([
  'metadata_version',
  'org_id',
  'key_hosts',
  'budget_ceiling',
  'zdr',
  'browser_agent_granted_by',
] as const);

export type ApiKeyRevocationReason =
  | 'deleted'
  | 'disabled'
  | 'expired'
  | 'owner_removed';

export interface ApiKeyAgentMetadata {
  metadata_version: typeof API_KEY_AGENT_METADATA_VERSION;
  org_id: string;
  key_hosts: string[];
  budget_ceiling: ApiKeyAgentBudgetCeiling;
  zdr: boolean;
  browser_agent_granted_by: string;
}

export type ApiKeyAgentRefusalReason =
  | 'api_key_agent_exchange_disabled'
  | 'api_key_org_required'
  | 'api_key_browser_agent_forbidden'
  | 'api_key_hosts_required'
  | 'api_key_hosts_too_many'
  | 'api_key_host_wildcard_refused'
  | 'api_key_host_invalid'
  | 'api_key_budget_invalid'
  | 'api_key_expiry_too_short'
  | 'api_key_expiry_too_long'
  | 'api_key_org_immutable'
  | 'api_key_browser_agent_requires_metadata'
  | 'api_key_reserved_metadata';

export type ApiKeyAgentResult<T> =
  | { ok: true; value: T }
  | { ok: false; reason: ApiKeyAgentRefusalReason; message: string };

function refuse<T>(
  reason: ApiKeyAgentRefusalReason,
  message: string,
): ApiKeyAgentResult<T> {
  return { ok: false, reason, message };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

/** The exchange flag of INNH-10 CP-1; browser:agent cannot be granted without it. */
export function apiKeyAgentExchangeEnabled(
  env: NodeJS.ProcessEnv = process.env,
): boolean {
  return (
    env.API_KEY_ENABLED === 'true' &&
    env.API_KEY_AGENT_EXCHANGE_ENABLED === 'true'
  );
}

// ---------------------------------------------------------------------------
// Hosts (broker-canonical, mirrors quarry-browser is_canonical_broker_domain)
// ---------------------------------------------------------------------------

function isCanonicalBrokerDomain(domain: string): boolean {
  if (
    domain.length === 0 ||
    domain.length > 253 ||
    isIP(domain) !== 0 ||
    !domain.includes('.') ||
    domain.startsWith('.') ||
    domain.endsWith('.')
  ) {
    return false;
  }
  return domain
    .split('.')
    .every(
      (label) =>
        label.length > 0 &&
        label.length <= 63 &&
        !label.startsWith('-') &&
        !label.endsWith('-') &&
        /^[a-z0-9-]+$/.test(label),
    );
}

/**
 * Normalise one host to the broker's canonical form: trimmed, lower case,
 * IDN to punycode, one trailing root dot removed. Wildcards, schemes, paths,
 * ports, credentials and IP literals are refused (OD-6: no wildcards).
 */
export function normalizeApiKeyHost(raw: string): ApiKeyAgentResult<string> {
  const trimmed = typeof raw === 'string' ? raw.trim() : '';
  if (trimmed.includes('*')) {
    return refuse(
      'api_key_host_wildcard_refused',
      'Host wildcards are not allowed in an API key host ceiling',
    );
  }
  if (trimmed.length === 0 || /[\s/:@?#\\[\]]/.test(trimmed)) {
    return refuse(
      'api_key_host_invalid',
      'Each host must be a bare domain name (no scheme, port or path)',
    );
  }
  const withoutRoot = trimmed.endsWith('.') ? trimmed.slice(0, -1) : trimmed;
  const ascii = domainToASCII(withoutRoot.toLowerCase());
  if (!ascii || !isCanonicalBrokerDomain(ascii)) {
    return refuse(
      'api_key_host_invalid',
      'Each host must be a public domain name with at least one dot',
    );
  }
  return { ok: true, value: ascii };
}

/** Canonical host ceiling: normalised, de-duplicated, sorted, 1..32 hosts. */
export function normalizeApiKeyHostCeiling(
  hosts: readonly string[] | undefined,
): ApiKeyAgentResult<string[]> {
  if (!Array.isArray(hosts) || hosts.length === 0) {
    return refuse(
      'api_key_hosts_required',
      'A browser:agent key needs a non-empty host ceiling',
    );
  }
  const canonical = new Set<string>();
  for (const host of hosts as readonly string[]) {
    const normalized = normalizeApiKeyHost(host);
    if (!normalized.ok) return normalized;
    canonical.add(normalized.value);
  }
  if (canonical.size > API_KEY_AGENT_MAX_HOSTS) {
    return refuse(
      'api_key_hosts_too_many',
      `A host ceiling holds at most ${API_KEY_AGENT_MAX_HOSTS} hosts`,
    );
  }
  // Code-unit order == the broker's BTreeSet byte order for ASCII hosts.
  return { ok: true, value: [...canonical].sort() };
}

// ---------------------------------------------------------------------------
// Budget ceiling and expiry
// ---------------------------------------------------------------------------

export interface ApiKeyAgentBudgetInput {
  microUsd?: number;
  maxSteps?: number;
  maxRuntimeSeconds?: number;
}

export function normalizeApiKeyBudgetCeiling(
  input: ApiKeyAgentBudgetInput | undefined,
): ApiKeyAgentResult<ApiKeyAgentBudgetCeiling> {
  const ceiling: ApiKeyAgentBudgetCeiling = {
    micro_usd: input?.microUsd ?? DEFAULT_API_KEY_BUDGET_CEILING.micro_usd,
    max_steps: input?.maxSteps ?? DEFAULT_API_KEY_BUDGET_CEILING.max_steps,
    max_runtime_s:
      input?.maxRuntimeSeconds ?? DEFAULT_API_KEY_BUDGET_CEILING.max_runtime_s,
  };
  for (const field of ['micro_usd', 'max_steps', 'max_runtime_s'] as const) {
    const value = ceiling[field];
    if (
      !Number.isSafeInteger(value) ||
      value <= 0 ||
      value > API_KEY_BUDGET_CEILING_LIMITS[field]
    ) {
      return refuse(
        'api_key_budget_invalid',
        `budget ceiling ${field} must be a positive integer of at most ${API_KEY_BUDGET_CEILING_LIMITS[field]}`,
      );
    }
  }
  return { ok: true, value: ceiling };
}

/**
 * Expiry in seconds from `now` (OD-15: at most 90 days). Absent means the
 * maximum: a browser:agent key never lives forever.
 */
export function apiKeyAgentExpiresIn(
  expiresAt: Date | undefined,
  now: Date,
): ApiKeyAgentResult<number> {
  if (!expiresAt) {
    return { ok: true, value: API_KEY_AGENT_MAX_EXPIRY_SECONDS };
  }
  const seconds = Math.floor((expiresAt.getTime() - now.getTime()) / 1000);
  // Better Auth's keyExpiration.minExpiresIn default is one day.
  if (!Number.isFinite(seconds) || seconds < API_KEY_AGENT_MIN_EXPIRY_SECONDS) {
    return refuse(
      'api_key_expiry_too_short',
      'A browser:agent key must expire at least one day from now',
    );
  }
  if (seconds > API_KEY_AGENT_MAX_EXPIRY_SECONDS) {
    return refuse(
      'api_key_expiry_too_long',
      'A browser:agent key expires after at most 90 days',
    );
  }
  return { ok: true, value: seconds };
}

/** Better Auth may hold several comma-separated roles in `member.role`. */
export function canGrantBrowserAgent(role: string | null | undefined): boolean {
  if (typeof role !== 'string') return false;
  return role
    .split(',')
    .map((part) => part.trim())
    .some((part) => part === 'owner' || part === 'admin');
}

// ---------------------------------------------------------------------------
// Creation / update validation (CP-2)
// ---------------------------------------------------------------------------

export interface ApiKeyAgentRequest {
  organizationId: string | undefined;
  hosts: readonly string[] | undefined;
  budget?: ApiKeyAgentBudgetInput;
  zdr?: boolean;
  expiresAt?: Date;
}

export interface ApiKeyAgentActor {
  userId: string;
  /** The actor's `member.role` in `organizationId`, or null if not a member. */
  orgRole: string | null;
}

export interface ValidatedApiKeyAgent {
  metadata: ApiKeyAgentMetadata;
  expiresIn: number;
}

export function validateApiKeyAgentRequest(
  request: ApiKeyAgentRequest,
  actor: ApiKeyAgentActor,
  options: { exchangeEnabled: boolean; now: Date },
): ApiKeyAgentResult<ValidatedApiKeyAgent> {
  if (!options.exchangeEnabled) {
    return refuse(
      'api_key_agent_exchange_disabled',
      'browser:agent keys are unavailable until the API-key exchange is enabled',
    );
  }
  const orgId = request.organizationId?.trim();
  if (!orgId) {
    return refuse(
      'api_key_org_required',
      'A browser:agent key must be bound to an organization',
    );
  }
  if (!canGrantBrowserAgent(actor.orgRole)) {
    return refuse(
      'api_key_browser_agent_forbidden',
      'Only an organization owner or admin may grant browser:agent',
    );
  }
  const hosts = normalizeApiKeyHostCeiling(request.hosts);
  if (!hosts.ok) return hosts;
  const budget = normalizeApiKeyBudgetCeiling(request.budget);
  if (!budget.ok) return budget;
  const expiresIn = apiKeyAgentExpiresIn(request.expiresAt, options.now);
  if (!expiresIn.ok) return expiresIn;
  return {
    ok: true,
    value: {
      metadata: {
        metadata_version: API_KEY_AGENT_METADATA_VERSION,
        org_id: orgId,
        key_hosts: hosts.value,
        budget_ceiling: budget.value,
        zdr: request.zdr === true,
        browser_agent_granted_by: actor.userId,
      },
      expiresIn: expiresIn.value,
    },
  };
}

/** Requested plain scopes must not smuggle browser:agent past the metadata path. */
export function plainScopesRefusal(
  scopes: readonly string[],
): ApiKeyAgentResult<never> | null {
  if (scopes.includes(BROWSER_AGENT_PERMISSION)) {
    return refuse(
      'api_key_browser_agent_requires_metadata',
      'browser:agent is granted through the browserAgent block (org, hosts, budget, expiry)',
    );
  }
  return null;
}

// ---------------------------------------------------------------------------
// Stored metadata and permissions (shared with the CP-1 exchange)
// ---------------------------------------------------------------------------

/** Better Auth stores metadata as JSON text, sometimes double-encoded. */
export function parseApiKeyJson(raw: unknown): Record<string, unknown> | null {
  let value: unknown = raw;
  for (let depth = 0; depth < 2 && typeof value === 'string'; depth += 1) {
    try {
      value = JSON.parse(value);
    } catch {
      return null;
    }
  }
  return isRecord(value) ? value : null;
}

/** Flattened permission list (same reading as orpc-router apiKeyScopesFrom). */
export function apiKeyPermissionList(raw: unknown): string[] {
  const permissions = parseApiKeyJson(raw);
  if (!permissions) return [];
  return Object.values(permissions)
    .filter(Array.isArray)
    .flatMap((scopes) => (scopes as unknown[]).map((scope) => String(scope)));
}

export function apiKeyHoldsBrowserAgent(rawPermissions: unknown): boolean {
  return apiKeyPermissionList(rawPermissions).includes(
    BROWSER_AGENT_PERMISSION,
  );
}

/** Permissions object for a browser:agent key (`api` array, as the router writes). */
export function browserAgentPermissions(scopes: readonly string[]): {
  api: string[];
} {
  const set = new Set(
    scopes.filter((scope) => scope !== BROWSER_AGENT_PERMISSION),
  );
  set.add(BROWSER_AGENT_PERMISSION);
  return { api: [...set] };
}

export function withoutBrowserAgentPermission(rawPermissions: unknown): {
  api: string[];
} {
  return {
    api: apiKeyPermissionList(rawPermissions).filter(
      (scope) => scope !== BROWSER_AGENT_PERMISSION,
    ),
  };
}

/** Metadata with every auth-core-only agent field removed. */
export function stripAgentMetadata(raw: unknown): Record<string, unknown> {
  const record = { ...(parseApiKeyJson(raw) ?? {}) };
  for (const field of RESERVED_API_KEY_AGENT_METADATA_FIELDS) {
    delete record[field];
  }
  return record;
}

/**
 * Strict re-read of stored agent metadata. Returns null unless every field is
 * still within the CP-2 bounds, so a hand-edited row fails closed.
 */
export function readApiKeyAgentMetadata(
  raw: unknown,
): ApiKeyAgentMetadata | null {
  const metadata = parseApiKeyJson(raw);
  if (!metadata) return null;
  if (metadata.metadata_version !== API_KEY_AGENT_METADATA_VERSION) return null;
  if (typeof metadata.org_id !== 'string' || metadata.org_id.trim() === '') {
    return null;
  }
  if (
    typeof metadata.browser_agent_granted_by !== 'string' ||
    metadata.browser_agent_granted_by === ''
  ) {
    return null;
  }
  const rawHosts: unknown = metadata.key_hosts;
  if (
    !Array.isArray(rawHosts) ||
    !rawHosts.every((host) => typeof host === 'string')
  ) {
    return null;
  }
  const storedHosts = rawHosts as string[];
  const hosts = normalizeApiKeyHostCeiling(storedHosts);
  if (
    !hosts.ok ||
    hosts.value.length !== storedHosts.length ||
    hosts.value.some((host, index) => host !== storedHosts[index])
  ) {
    return null;
  }
  const rawBudget = metadata.budget_ceiling;
  if (
    !isRecord(rawBudget) ||
    typeof rawBudget.micro_usd !== 'number' ||
    typeof rawBudget.max_steps !== 'number' ||
    typeof rawBudget.max_runtime_s !== 'number'
  ) {
    return null;
  }
  const budget = normalizeApiKeyBudgetCeiling({
    microUsd: rawBudget.micro_usd,
    maxSteps: rawBudget.max_steps,
    maxRuntimeSeconds: rawBudget.max_runtime_s,
  });
  if (!budget.ok) return null;
  if (typeof metadata.zdr !== 'boolean') return null;
  return {
    metadata_version: API_KEY_AGENT_METADATA_VERSION,
    org_id: metadata.org_id,
    key_hosts: hosts.value,
    budget_ceiling: budget.value,
    zdr: metadata.zdr,
    browser_agent_granted_by: metadata.browser_agent_granted_by,
  };
}

/** True when a client-supplied metadata object writes an auth-core-only field. */
export function touchesReservedAgentMetadata(metadata: unknown): boolean {
  const record = parseApiKeyJson(metadata);
  if (!record) return false;
  return RESERVED_API_KEY_AGENT_METADATA_FIELDS.some((field) =>
    Object.prototype.hasOwnProperty.call(record, field),
  );
}

// ---------------------------------------------------------------------------
// Revocation event (CP-3)
// ---------------------------------------------------------------------------

/** The subset of a Better Auth `apikey` row the revocation path needs. */
export interface ApiKeyRevocationRow {
  id: string;
  configId: string | null;
  referenceId: string | null;
  enabled: boolean | null;
  expiresAt: Date | string | null;
  updatedAt: Date | string | null;
  permissions: unknown;
  metadata: unknown;
}

export interface ApiKeyRevocationTarget {
  keyId: string;
  orgId: string;
  ownerUserId: string | null;
}

/**
 * Only browser:agent keys (or keys that still carry agent metadata) are
 * org-bound, so only they produce the event; audit-core requires org_id.
 */
export function apiKeyRevocationTarget(
  row: ApiKeyRevocationRow | null | undefined,
): ApiKeyRevocationTarget | null {
  if (!row || typeof row.id !== 'string' || row.id === '') return null;
  const metadata = parseApiKeyJson(row.metadata);
  const orgId =
    metadata && typeof metadata.org_id === 'string'
      ? metadata.org_id.trim()
      : '';
  if (!orgId) return null;
  if (
    !apiKeyHoldsBrowserAgent(row.permissions) &&
    metadata?.metadata_version !== API_KEY_AGENT_METADATA_VERSION
  ) {
    return null;
  }
  return {
    keyId: row.id,
    orgId,
    ownerUserId: typeof row.referenceId === 'string' ? row.referenceId : null,
  };
}

function eventIdPart(value: string): string {
  return value.replace(/[^A-Za-z0-9._-]/g, '_');
}

function epochMs(
  value: Date | string | number | null | undefined,
): number | null {
  if (value === null || value === undefined) return null;
  const ms =
    value instanceof Date ? value.getTime() : new Date(value).getTime();
  return Number.isFinite(ms) ? ms : null;
}

export interface ApiKeyRevokedEventInput {
  target: ApiKeyRevocationTarget;
  reason: ApiKeyRevocationReason;
  occurredAt: Date;
  /** The user who caused it (delete/disable); absent for expiry and sweeps. */
  actorUserId?: string;
  /**
   * Distinguishes repeatable revocations of one key (disable, re-enable,
   * disable): the row's updatedAt for disable, expiresAt for expiry.
   */
  discriminator?: Date | string | number | null;
}

/**
 * Durable audit payload for `verevon.audit.v2.control.auth-core.api_key_revoked`.
 * The consumer contract (broker MP-2) is `details.{key_id, org_id, reason}`.
 * event_id is stable per revocation so a retried publish de-duplicates
 * (JetStream msgID) and matches DirectNatsService AUDIT_EVENT_ID.
 */
export function buildApiKeyRevokedEvent(
  input: ApiKeyRevokedEventInput,
): Record<string, unknown> {
  const discriminator = epochMs(input.discriminator);
  const eventId = [
    'api_key',
    eventIdPart(input.target.keyId),
    'revoked',
    input.reason,
    ...(discriminator === null ? [] : [String(discriminator)]),
  ]
    .join(':')
    .slice(0, 128);
  return {
    occurred_at: new Date(input.occurredAt.getTime()).toISOString(),
    event_id: eventId,
    org_id: input.target.orgId,
    ...(input.actorUserId ? { user_id: input.actorUserId } : {}),
    actor_role: input.actorUserId ? 'user' : 'system',
    plane: 'control',
    producer: 'auth-core',
    event: API_KEY_REVOKED_EVENT,
    resource_id: input.target.keyId,
    outcome: 'ok',
    details: {
      key_id: input.target.keyId,
      org_id: input.target.orgId,
      reason: input.reason,
    },
  };
}

export type DurableAuditPublish = (
  subject: string,
  payload: Record<string, unknown>,
) => Promise<unknown>;

/** Publish and wait for the PubAck; throws so callers can fail closed. */
export async function publishApiKeyRevoked(
  publish: DurableAuditPublish,
  input: ApiKeyRevokedEventInput,
): Promise<Record<string, unknown>> {
  const payload = buildApiKeyRevokedEvent(input);
  await publish(API_KEY_REVOKED_SUBJECT, payload);
  return payload;
}

/** The revocation a key update causes, if any (disable or browser:agent removed). */
export function revocationForUpdate(
  row: ApiKeyRevocationRow,
  body: { enabled?: unknown; permissions?: unknown },
): ApiKeyRevocationReason | null {
  if (body.enabled === false && row.enabled !== false) return 'disabled';
  if (
    body.permissions !== undefined &&
    apiKeyHoldsBrowserAgent(row.permissions) &&
    !apiKeyHoldsBrowserAgent(body.permissions)
  ) {
    return 'disabled';
  }
  return null;
}

// ---------------------------------------------------------------------------
// Expiry and owner-removal sweep (CP-3)
// ---------------------------------------------------------------------------

export interface ApiKeyRevocationSweepDeps {
  /** Enabled user-keys rows that may be browser:agent keys. */
  listCandidates: () => Promise<ApiKeyRevocationRow[]>;
  isActiveMember: (userId: string, orgId: string) => Promise<boolean>;
  publish: DurableAuditPublish;
  /** Disable the key after its event is acknowledged (stops re-detection). */
  disable: (keyId: string, now: Date) => Promise<void>;
  now: () => Date;
}

export interface ApiKeyRevocationSweepResult {
  revoked: Array<{ keyId: string; reason: ApiKeyRevocationReason }>;
  failed: Array<{ keyId: string; reason: ApiKeyRevocationReason }>;
}

/**
 * Expiry and owner removal have no Better Auth hook we can rely on (the
 * membership mutation short-circuits in organization-events.plugin, and expiry
 * is time), so a periodic sweep detects them. A key is disabled only after its
 * event has a PubAck: a failed publish is retried on the next sweep.
 */
export async function sweepApiKeyRevocations(
  deps: ApiKeyRevocationSweepDeps,
): Promise<ApiKeyRevocationSweepResult> {
  const result: ApiKeyRevocationSweepResult = { revoked: [], failed: [] };
  const now = deps.now();
  const rows = await deps.listCandidates();
  for (const row of rows) {
    if (row.enabled === false) continue;
    const target = apiKeyRevocationTarget(row);
    if (!target) continue;
    const expiresAtMs = epochMs(row.expiresAt);
    let reason: ApiKeyRevocationReason | null = null;
    let discriminator: number | null = null;
    try {
      if (expiresAtMs !== null && expiresAtMs <= now.getTime()) {
        reason = 'expired';
        discriminator = expiresAtMs;
      } else if (
        !target.ownerUserId ||
        !(await deps.isActiveMember(target.ownerUserId, target.orgId))
      ) {
        reason = 'owner_removed';
        discriminator = epochMs(row.updatedAt);
      }
      if (!reason) continue;
      await publishApiKeyRevoked(deps.publish, {
        target,
        reason,
        occurredAt: now,
        discriminator,
      });
      await deps.disable(target.keyId, now);
      result.revoked.push({ keyId: target.keyId, reason });
    } catch {
      result.failed.push({ keyId: target.keyId, reason: reason ?? 'owner_removed' });
    }
  }
  return result;
}
