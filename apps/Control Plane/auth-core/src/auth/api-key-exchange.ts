/**
 * EP-12 S2 / INNH-10 CP-1: the pure policy of the API-key exchange.
 *
 * `POST /api/:audience/api-key-exchange` turns a Better Auth `user-keys` API
 * key into one short-lived, audience-bound `api_key` principal token. This
 * module decides, without I/O, whether a verified key may have that token and
 * with which claims; the controller does the reads (key verification,
 * membership, org posture), the signing and the durable audit.
 *
 * The principal is the key (`sub = apikey:<key_id>`); the person who owns it
 * is attribution only (`acting_user_id`, never `user_id`). A key token can
 * therefore never inherit a person's powers by default (INNH-10 OD-1, EP-12
 * D2).
 */
import type { CanonicalTokenContext } from './plane-token-membership';

/**
 * The closed table of audiences an API key may be exchanged into and the only
 * scopes each may carry. EP-12 adds the Agent API audiences (`agent:runs`),
 * INNH-10 the browser ones (`browser:agent`). A registry entry can only narrow
 * this (its `apiKeyExchange` list names which of these audiences a caller may
 * use); nothing widens it.
 */
export const API_KEY_EXCHANGE_AUDIENCE_SCOPES = {
  'model-gateway': ['agent:runs'],
  'session-core': ['agent:runs'],
  'inference-core': ['agent:runs'],
  quarry: ['browser:agent'],
  'browser-broker': ['browser:agent'],
} as const satisfies Readonly<Record<string, readonly string[]>>;

export type ApiKeyExchangeAudience =
  keyof typeof API_KEY_EXCHANGE_AUDIENCE_SCOPES;

export function isApiKeyExchangeAudience(
  value: string,
): value is ApiKeyExchangeAudience {
  return Object.hasOwn(API_KEY_EXCHANGE_AUDIENCE_SCOPES, value);
}

/** The only Better Auth key configuration an exchange accepts (INNH-10 OD-4). */
export const API_KEY_EXCHANGE_CONFIG_ID = 'user-keys';

/** The key id bound the downstream verifiers apply (model-gateway, broker). */
const MAX_API_KEY_ID_LENGTH = 128;
const MAX_ACTING_USER_ID_LENGTH = 256;

/**
 * Every refusal an exchange can answer, as `details.reason`. Closed so the
 * audit, the response and the tests name the same few words.
 */
export type ApiKeyExchangeDenialReason =
  | 'key_invalid'
  | 'key_disabled'
  | 'key_expired'
  | 'key_rate_limited'
  | 'org_key_refused'
  | 'org_mismatch'
  | 'owner_not_member'
  | 'owner_banned'
  | 'scope_not_granted'
  | 'audience_not_allowed';

/**
 * The fields of a verified key this policy reads, as Better Auth's
 * `verifyApiKey` returns them (the hashed key itself is never part of it).
 * `permissions` is the plugin's `{ resource: [action] }` map; `metadata`
 * carries the creation-time `org_id` binding and the optional `zdr` flag.
 */
export type VerifiedApiKey = Readonly<{
  id: string;
  configId: string;
  referenceId: string;
  enabled: boolean;
  expiresAt: Date | string | null;
  permissions?: Readonly<Record<string, readonly string[]>> | null;
  metadata?: Readonly<Record<string, unknown>> | null;
}>;

export type ApiKeyExchangeRequest = Readonly<{
  audience: string;
  orgId: string;
  scopes: readonly string[];
}>;

/** What a granted exchange mints: the identity, tenant and scope of the token. */
export type ApiKeyExchangeGrant = Readonly<{
  audience: ApiKeyExchangeAudience;
  keyId: string;
  actingUserId: string;
  orgId: string;
  scopes: readonly string[];
  /** The key's own `zdr` flag; it can only raise the org's posture. */
  keyZdr: boolean;
}>;

export type ApiKeyExchangeDecision =
  | Readonly<{ allowed: true; grant: ApiKeyExchangeGrant }>
  | Readonly<{ allowed: false; reason: ApiKeyExchangeDenialReason }>;

function deny(reason: ApiKeyExchangeDenialReason): ApiKeyExchangeDecision {
  return { allowed: false, reason };
}

/** The bound model-gateway's `valid_api_key_id` applies: printable, no space. */
function validKeyId(keyId: string): boolean {
  return (
    keyId.length > 0 &&
    keyId.length <= MAX_API_KEY_ID_LENGTH &&
    /^[\x21-\x7e]+$/.test(keyId)
  );
}

function validOwner(userId: string): boolean {
  return (
    userId.length > 0 &&
    userId.length <= MAX_ACTING_USER_ID_LENGTH &&
    userId === userId.trim() &&
    !userId.startsWith('apikey:')
  );
}

function keyExpired(expiresAt: Date | string | null, now: number): boolean {
  if (expiresAt === null) return false;
  const at = new Date(expiresAt).getTime();
  // An expiry that cannot be read is not "never expires".
  return !Number.isFinite(at) || at <= now;
}

/**
 * A scope `resource:action` is granted when the key's permission map lists
 * `action` under `resource`, the plugin's own permission shape. Nothing else
 * (a wildcard, a bare resource) grants it.
 */
function keyGrantsScope(
  permissions: VerifiedApiKey['permissions'],
  scope: string,
): boolean {
  const separator = scope.indexOf(':');
  if (!permissions || separator <= 0 || separator === scope.length - 1) {
    return false;
  }
  const actions = permissions[scope.slice(0, separator)];
  return (
    Array.isArray(actions) && actions.includes(scope.slice(separator + 1))
  );
}

/**
 * Decide one exchange. `membership` is the key owner's canonical membership
 * in `request.orgId` (null when there is none) and `ownerBanned` whether that
 * owner's account is currently banned, both read by the caller. The
 * checks run from the key outwards so the reason names the first thing that
 * is wrong; every path that is not the last one refuses.
 */
export function decideApiKeyExchange(
  key: VerifiedApiKey,
  membership: CanonicalTokenContext | null,
  request: ApiKeyExchangeRequest,
  now: number = Date.now(),
  ownerBanned = false,
): ApiKeyExchangeDecision {
  const audience = request.audience;
  if (!isApiKeyExchangeAudience(audience)) {
    return deny('audience_not_allowed');
  }
  if (!validKeyId(key.id)) return deny('key_invalid');
  if (key.enabled !== true) return deny('key_disabled');
  if (keyExpired(key.expiresAt, now)) return deny('key_expired');
  // An org key names no person to attribute the work to (INNH-10 OD-4).
  if (key.configId !== API_KEY_EXCHANGE_CONFIG_ID) {
    return deny('org_key_refused');
  }
  if (!validOwner(key.referenceId)) return deny('key_invalid');

  const orgId = request.orgId.trim();
  const boundOrg = key.metadata?.org_id;
  if (!orgId || typeof boundOrg !== 'string' || boundOrg !== orgId) {
    return deny('org_mismatch');
  }
  if (!membership || membership.orgId !== orgId) {
    return deny('owner_not_member');
  }
  // A banned owner keeps a member row; the key stops minting tokens while the
  // ban stands (the key itself is left alone, a ban can expire).
  if (ownerBanned) return deny('owner_banned');

  const scopes = [...new Set(request.scopes.map((scope) => scope.trim()))];
  const audienceScopes: readonly string[] =
    API_KEY_EXCHANGE_AUDIENCE_SCOPES[audience];
  if (
    scopes.length === 0 ||
    scopes.some(
      (scope) =>
        !audienceScopes.includes(scope) ||
        !keyGrantsScope(key.permissions, scope),
    )
  ) {
    return deny('scope_not_granted');
  }

  return {
    allowed: true,
    grant: {
      audience,
      keyId: key.id,
      actingUserId: key.referenceId,
      orgId,
      scopes,
      keyZdr: key.metadata?.zdr === true,
    },
  };
}

/**
 * The token's `zdr`: the org's standing posture OR the key's flag. A key can
 * raise retention to zero, never lower an org that is ZDR.
 */
export function apiKeyTokenZdr(orgZdr: boolean, keyZdr: boolean): boolean {
  return orgZdr || keyZdr;
}

/**
 * Map Better Auth's `verifyApiKey` refusal code to a denial reason. Only the
 * codes the plugin documents for a key it found are named; anything else,
 * including an unknown key, is `key_invalid` so a caller cannot probe which
 * keys exist.
 */
export function apiKeyVerificationDenial(
  code: string | undefined,
): ApiKeyExchangeDenialReason {
  switch (code) {
    case 'KEY_DISABLED':
      return 'key_disabled';
    case 'KEY_EXPIRED':
      return 'key_expired';
    case 'RATE_LIMITED':
    case 'USAGE_EXCEEDED':
      return 'key_rate_limited';
    default:
      return 'key_invalid';
  }
}
