import { Injectable, Logger } from '@nestjs/common';
import {
  createPrivateKey,
  createPublicKey,
  createSign,
  createVerify,
  generateKeyPairSync,
  type KeyObject,
} from 'crypto';
import { readFileSync } from 'fs';
import type { FreshExecutionRetentionProof, InteractiveRetentionPosture } from './interactive-retention-policy';
// Type only on purpose: retention-decision.signer.ts imports this module for its
// Nest provider, so a value import here would be a load-order cycle.
import type { VerifiedRetentionDecision } from './retention-decision.signer';
import {
  assertWebPolicyClaimSize,
  BASELINE_WEB_POLICY_CLAIM,
  isWebPolicyAudience,
  type WebPolicyClaim,
} from './web-policy';

type ConvexJwtClaims = {
  externalAuthId: string;
  email: string;
  name?: string | null;
  activeOrgId?: string;
  activeOrgRole?: string;
};

type ConvexTokenBundle = {
  token: string;
  expiresAt: string;
  expiresInSeconds: number;
  issuer: string;
  audience: string;
};

/**
 * U2-5 follow-up: claims required by the Model Plane gateway's `auth::require_auth`
 * middleware (`apps/Model Plane/rust/services/model-gateway/src/auth.rs`).
 *
 * The gateway expects `org_id` and `user_id` (snake_case top-level claims) plus
 * standard `iss`/`exp`/`aud`. We mint a separate token for this audience rather
 * than reusing the Convex token because:
 *   1. Audience separation — a token leaked to Convex shouldn't unlock the gateway.
 *   2. The gateway needs `org_id` not `activeOrgId` (different claim shape).
 *   3. Decoupled TTL (the gateway uses 5-30 minute tokens; Convex uses 5 minutes).
 *
 * Same RS256 keypair / same JWKS — the gateway's
 * `AUTH_CORE_JWKS_URL` points at the existing `/api/convex-auth/jwks` endpoint.
 */
type ModelPlaneJwtClaims = {
  userId: string;
  orgId: string;
  email?: string;
  scopes?: readonly string[];
  principalType?: TokenPrincipalType;
  serviceId?: string;
  reason?: string;
  retentionPosture?: ServiceRetentionPosture | InteractiveRetentionPosture;
  /**
   * A verified, session-bound per-connection ZDR waiver decision (ZDR-Z-14b).
   * Accepted only for a `user` token; see {@link resolveWaiverMint}.
   */
  retentionWaiver?: VerifiedRetentionDecision;
  // A fresh password re-check from POST /api/v2/auth/step-up (WS13-S5).
  stepUp?: { at: number; method: 'password' };
  /**
   * The four fields below are read only for an `api_key` principal (EP-12 S2,
   * minted solely by `POST /api/:audience/api-key-exchange`): any other
   * principal that carries `keyId` or `actingUserId` is refused, and
   * `ttlSecondsOverride` and `tokenId` are ignored on it.
   */
  keyId?: string;
  actingUserId?: string;
  ttlSecondsOverride?: number;
  tokenId?: string;
};

/**
 * `api_key` (EP-12 S2, INNH-10 CP-4): the principal is an API key, `sub` is
 * `apikey:<key_id>`, and the key's owner rides along as `acting_user_id` for
 * attribution only. Only the API-key exchange controller passes it.
 */
type TokenPrincipalType = 'user' | 'service' | 'api_key';

/**
 * Hard ceiling on an `api_key` token's lifetime, whatever the audience's
 * configured default (INNH-10 OD-9). The exchange route mints one per call, so
 * a revoked key stops authenticating within this bound.
 */
export const API_KEY_TOKEN_MAX_TTL_SECONDS = 120;

type ApiKeyIdentityInput = {
  userId: string;
  email?: string;
  serviceId?: string;
  keyId?: string;
  actingUserId?: string;
  tokenId?: string;
};

/**
 * The identity claims of an `api_key` token, or a throw when the shape is not
 * exactly the one model-gateway's `is_well_formed_api_key` and Quarry's
 * normalizer accept: a bounded printable key id, `sub == "apikey:" + key_id`,
 * an unpadded owner that is not itself a key, a fresh `jti`, and no service or
 * person attribute (`service_id`, `email`) beside them.
 */
function apiKeyIdentityClaims(claims: ApiKeyIdentityInput): {
  key_id: string;
  acting_user_id: string;
  jti: string;
} {
  const keyId = claims.keyId ?? '';
  const owner = claims.actingUserId ?? '';
  if (
    !/^[\x21-\x7e]{1,128}$/.test(keyId) ||
    claims.userId !== `apikey:${keyId}` ||
    claims.serviceId !== undefined ||
    claims.email !== undefined ||
    !owner ||
    owner.length > 256 ||
    owner !== owner.trim() ||
    owner.startsWith('apikey:') ||
    !claims.tokenId
  ) {
    throw new Error('API-key token identity is ambiguous');
  }
  return { key_id: keyId, acting_user_id: owner, jti: claims.tokenId };
}

/**
 * An `api_key` token's lifetime: the audience default, a strictly shorter
 * override, and never more than `API_KEY_TOKEN_MAX_TTL_SECONDS`.
 */
function apiKeyTtlSeconds(
  audienceTtlSeconds: number,
  override: number | undefined,
): number {
  const requested =
    override !== undefined &&
    Number.isFinite(override) &&
    override > 0 &&
    override < audienceTtlSeconds
      ? override
      : audienceTtlSeconds;
  return Math.min(requested, API_KEY_TOKEN_MAX_TTL_SECONDS);
}

type ModelPlaneTokenBundle = {
  token: string;
  expiresAt: string;
  expiresInSeconds: number;
  issuer: string;
  audience: string;
};

/**
 * Phase A · A1.1 — non-Model-Plane audience set.
 *
 * The other planes get RS256 JWTs from the same keypair (and therefore
 * the same JWKS at `/api/convex-auth/jwks`), differing only in the `aud`
 * claim. Each plane's middleware verifies `aud` against its own expected
 * value, so a token leaked to one plane never authenticates the others.
 *
 * Token TTL per audience is tunable via env vars:
 *   PLANE_TOKEN_TTL_DATA_PLANE_SECONDS         (default 300s)
 *   PLANE_TOKEN_TTL_QUARRY_SECONDS             (default 300s)
 *   PLANE_TOKEN_TTL_INGESTION_SECONDS          (default 300s)
 *   PLANE_TOKEN_TTL_CONTROL_PLANE_SECONDS      (default 300s)
 *   PLANE_TOKEN_TTL_CONTROL_POLICY_SECONDS     (default 300s, service-only)
 *   PLANE_TOKEN_TTL_APPLICATION_PLANE_SECONDS  (default 300s)
 *   PLANE_TOKEN_TTL_SESSION_CORE_SECONDS       (default 300s)
 *   PLANE_TOKEN_TTL_INFERENCE_CORE_SECONDS     (default 300s)
 *   PLANE_TOKEN_TTL_EXECUTION_CORE_SECONDS     (default 300s)
 *   PLANE_TOKEN_TTL_COST_CORE_SECONDS          (default 300s)
 *   PLANE_TOKEN_TTL_CAPABILITY_CORE_SECONDS    (default 300s)
 *   PLANE_TOKEN_TTL_LETTA_BRIDGE_SECONDS       (default 300s)
 *   PLANE_TOKEN_TTL_BROWSER_BROKER_SECONDS     (default 300s)
 *   PLANE_TOKEN_TTL_SANDBOX_MANAGER_SECONDS    (default 300s)
 *   PLANE_TOKEN_TTL_BRIDGE_CORE_SECONDS        (default 300s)
 *
 * Audience names match the path slug (and the verevon `PlaneAudience`
 * union) for symmetry with the per-audience endpoints.
 */
export type PlaneAudience =
  | 'data-plane'
  | 'quarry'
  | 'ingestion'
  | 'control-plane'
  | 'control-policy'
  | 'application-plane'
  | 'session-core'
  | 'inference-core'
  | 'execution-core'
  | 'cost-core'
  | 'capability-core'
  | 'letta-bridge'
  | 'browser-broker'
  | 'sandbox-manager'
  | 'bridge-core'
  | 'orchestrator-core';

export type AuthCoreAudience = 'model-gateway' | PlaneAudience;

/**
 * Audiences that ONLY a service principal may mint a token for. The
 * interactive session route (`GET /api/:audience/token`) must never issue a
 * user-bound JWT for one of these; services are unaffected because they mint
 * through the internal-token route, which is not gated on this list.
 *
 * `control-policy` is service-only because it is a machine-to-machine
 * decision surface. `orchestrator-core` is service-only for a sharper reason:
 * its StartWorkflow lets a NON-service caller start any run-scoped workflow
 * (`authorizePolicy`'s `PolicyRunScoped` returns early for `!caller.Service`).
 * People already start runs through model-gateway, so an interactive
 * orchestrator-core token would add a way to drive the durable tier directly
 * with no product need for it.
 *
 * This tuple is the single source of truth for the exclusion: the
 * `InteractivePlaneAudience` type is derived from it and the runtime guard
 * `isInteractivePlaneAudience` checks membership in it, so the compile-time
 * contract and the HTTP gate cannot drift apart again. The `satisfies` clause
 * rejects a slug that is not a `PlaneAudience`, which a bare `Exclude<>`
 * would silently ignore.
 */
export const SERVICE_ONLY_PLANE_AUDIENCES = [
  'control-policy',
  'orchestrator-core',
] as const satisfies readonly PlaneAudience[];

export type ServiceOnlyPlaneAudience =
  (typeof SERVICE_ONLY_PLANE_AUDIENCES)[number];

const serviceOnlyPlaneAudienceSet: ReadonlySet<string> = new Set(
  SERVICE_ONLY_PLANE_AUDIENCES,
);

/**
 * Audiences a signed-in USER may mint a token for through the interactive
 * session route: every `PlaneAudience` except `SERVICE_ONLY_PLANE_AUDIENCES`.
 */
export type InteractivePlaneAudience = Exclude<
  PlaneAudience,
  ServiceOnlyPlaneAudience
>;

export type ServiceRetentionPosture = {
  zdr: boolean;
  authority: 'service-principal-policy';
  /**
   * Set (by `resolveTenantServiceRetentionPosture` only) when the org's own ZDR
   * setting, not the registry, decided `zdr`. The signer then caps the token's
   * lifetime at `TENANT_PRINCIPAL_TOKEN_TTL_SECONDS`, so the cap holds for every
   * caller instead of depending on each one passing a TTL.
   */
  source?: 'tenant-policy';
};

/**
 * Longest a token minted for a `tenant`-retention service principal may live.
 * Its `zdr` claim is a snapshot of the org's setting, and a consumer that caches
 * the bearer keeps acting on that snapshot until it expires; 60 seconds (the
 * floor `parsePositiveTtl` already enforces for every audience) bounds how long
 * after an org turns ZDR on a worker can still hold a token that says it has
 * not. Together with the resolver's 45 second cache that is the whole staleness
 * window in normal operation.
 */
export const TENANT_PRINCIPAL_TOKEN_TTL_SECONDS = 60;

type RetentionAwareClaims = {
  orgId: string;
  retentionPosture?: ServiceRetentionPosture | InteractiveRetentionPosture;
};

/**
 * Retention posture is issuer-owned computed state. It is resolved once by
 * the caller — asynchronously, via `resolveInteractiveRetentionPosture`
 * (interactive users) or the bounded service-principal registry (service
 * principals) — and handed in as a claim; this function never performs I/O
 * itself so token minting stays synchronous.
 *
 * Interactive (`user`) tokens trust ONLY a claim stamped with the
 * `interactive-org-retention-policy` authority. Anything else — including no
 * claim at all — fails closed to `zdr: false` (normal retention is Auth
 * Core's default for every org; ZDR is an opt-in, paid, plan-gated add-on).
 *
 * Service tokens are unrelated to this task and keep their prior behavior:
 * they trust only a claim stamped `service-principal-policy`, defaulting to
 * `zdr: true` otherwise. No caller-selected field (request body, header) can
 * ever downgrade either posture.
 *
 * `api_key` tokens carry the org's posture (already raised by the key's own
 * flag in the exchange controller) under the organization-policy authority;
 * without it they fail closed to `zdr: true`, never to a persistent token.
 */
function issuedTokenZdr(
  principalType: TokenPrincipalType,
  claims: RetentionAwareClaims,
): boolean {
  if (principalType === 'user') {
    return claims.retentionPosture?.authority ===
      'interactive-org-retention-policy'
      ? claims.retentionPosture.zdr
      : false;
  }
  if (principalType === 'api_key') {
    return claims.retentionPosture?.authority ===
      'interactive-org-retention-policy'
      ? claims.retentionPosture.zdr
      : true;
  }
  return claims.retentionPosture?.authority === 'service-principal-policy'
    ? claims.retentionPosture.zdr
    : true;
}

/**
 * The lifetime cap a service token carries because its posture came from the
 * org, or `undefined` when it has none. Read only for service tokens: a user
 * token's posture has its own authority and never takes this path.
 */
function tenantTtlCapSeconds(
  principalType: TokenPrincipalType,
  claims: RetentionAwareClaims,
): number | undefined {
  return principalType === 'service' &&
    claims.retentionPosture?.authority === 'service-principal-policy' &&
    claims.retentionPosture.source === 'tenant-policy'
    ? TENANT_PRINCIPAL_TOKEN_TTL_SECONDS
    : undefined;
}

/**
 * Audiences whose user token is never lowered by a waiver (ZDR.md 5.6, D-ZDR-11):
 * a waived turn never gets a browser or sandbox bearer. Single source of truth
 * for the exclusion, like `SERVICE_ONLY_PLANE_AUDIENCES`.
 */
export const WAIVER_EXCLUDED_AUDIENCES = [
  'browser-broker',
  'sandbox-manager',
] as const satisfies readonly PlaneAudience[];

const waiverExcludedAudienceSet: ReadonlySet<string> = new Set(
  WAIVER_EXCLUDED_AUDIENCES,
);

/**
 * True for every interactive user audience of the model-plane route and the
 * plane route (`model-gateway` and every `InteractivePlaneAudience`) except
 * `WAIVER_EXCLUDED_AUDIENCES`. The callers have already validated the slug.
 */
export function isWaiverAudience(audience: AuthCoreAudience): boolean {
  return (
    !waiverExcludedAudienceSet.has(audience) &&
    !serviceOnlyPlaneAudienceSet.has(audience)
  );
}

/** Longest a token that carries a `zdr_waiver` claim may live, in seconds. */
export const WAIVER_TOKEN_MAX_TTL_SECONDS = 300;

/**
 * Waiver contract version in the `zdr_waiver.version` claim. Equal to
 * `WAIVER_CONTRACT_VERSION` in retention-decision.signer.ts (not imported, to
 * keep that module a type-only import here); the enforcers refuse any other.
 */
const WAIVER_CLAIM_VERSION = 1;

/** The `zdr_waiver` claim as the enforcers read it (version, provider, connection, ack_id). */
export type ZdrWaiverClaim = {
  version: number;
  provider: string;
  connection: string;
  ack_id: string;
  digest: string;
  decision_jti: string;
};

type WaiverMint = {
  zdrWaiver: ZdrWaiverClaim;
  ttlCapSeconds: number;
};

/**
 * The `zdr_waiver` claim and lifetime cap of a token, or `undefined` when the
 * mint carries no waiver. A waiver is never silently dropped: it is accepted
 * only for a `user` token on a waiver audience, whose org posture is ZDR, and
 * whose decision names exactly this user and org; anything else throws and
 * nothing is signed. The same decision gives the same claim on every audience.
 */
function resolveWaiverMint(
  audience: AuthCoreAudience,
  principalType: TokenPrincipalType,
  claims: RetentionAwareClaims & {
    userId: string;
    retentionWaiver?: VerifiedRetentionDecision;
  },
): WaiverMint | undefined {
  const decision = claims.retentionWaiver;
  if (decision === undefined) return undefined;
  if (principalType !== 'user' || !isWaiverAudience(audience)) {
    throw new Error(
      `A retention waiver cannot be carried on a ${principalType} token for ${audience}`,
    );
  }
  if (
    claims.retentionPosture?.authority !== 'interactive-org-retention-policy' ||
    claims.retentionPosture.zdr !== true
  ) {
    throw new Error('A retention waiver needs a ZDR organization posture');
  }
  if (
    decision.decision !== 'waived' ||
    decision.orgId !== claims.orgId ||
    decision.userId !== claims.userId ||
    decision.expiresAt <= Math.floor(Date.now() / 1000)
  ) {
    throw new Error('Retention waiver does not match the token subject');
  }
  return {
    zdrWaiver: {
      version: WAIVER_CLAIM_VERSION,
      provider: decision.provider,
      connection: decision.connection,
      ack_id: decision.ackId,
      digest: decision.digest,
      decision_jti: decision.jti,
    },
    ttlCapSeconds: WAIVER_TOKEN_MAX_TTL_SECONDS,
  };
}

type PlaneJwtClaims = {
  userId: string;
  orgId: string;
  email?: string;
  scopes?: readonly string[];
  principalType?: TokenPrincipalType;
  serviceId?: string;
  reason?: string;
  retentionPosture?: ServiceRetentionPosture | InteractiveRetentionPosture;
  /**
   * A verified, session-bound per-connection ZDR waiver decision (ZDR-Z-14b).
   * Accepted only for a `user` token on a waiver audience; see
   * {@link resolveWaiverMint}.
   */
  retentionWaiver?: VerifiedRetentionDecision;
  executionRetentionProof?: FreshExecutionRetentionProof;
  /**
   * Delegated-identity token exchange (auth-core §1, `verifyPlaneSubjectToken`
   * / `/api/:audience/token-exchange`). Set only on the exchanged service
   * token so the downstream consumer (e.g. integration-corev2's
   * `VerifyToken`) can read a real, cryptographically-bound acting user
   * alongside the service identity. `principal_type` stays `"service"` —
   * this is an additive claim, not a different token shape.
   *
   * Also required on an `api_key` token, where it names the key's owner
   * (attribution only; the principal is the key).
   */
  actingUserId?: string;
  /**
   * The Better Auth key id of an `api_key` principal (EP-12 S2), emitted as
   * `key_id`. Refused on every other principal type.
   */
  keyId?: string;
  /**
   * Lets one call site mint a token shorter-lived than the audience's
   * configured default (never longer — any value `>=` the audience default
   * is ignored). Used by the token-exchange endpoint to mint a 90s token
   * distinct from `/internal-token`'s normal TTL for the same audience.
   */
  ttlSecondsOverride?: number;
  /**
   * Unique token id, emitted as the `jti` claim. Set ONLY by the token-exchange
   * endpoint, whose token is a complete standalone bearer that its resource
   * server (integration-corev2) consumes exactly once: the `jti` is what lets
   * that server refuse a captured copy (T-2/A-1). Ordinary plane tokens carry
   * none, so nothing that verifies them changes.
   */
  tokenId?: string;
  /**
   * The org web policy (WS7.I), resolved by the caller through
   * `webPolicyClaimFor` and signed as the `web_policy` claim beside `zdr`.
   * Accepted only for `quarry` and `browser-broker`. A USER token for those
   * audiences always carries the claim (`{"v":1,"rev":"0"}` when none is
   * given), so an absent claim identifies an old issuer; a SERVICE token
   * carries it only when the caller passes one (a `tenant` registry row).
   * Never touches `zdr`.
   */
  webPolicy?: WebPolicyClaim;
};

/**
 * Every `principal_type` this authority signs on a plane token (INNH-10 CP-4).
 * `user` and `service` mint through `issuePlaneToken`. `api_key` is minted only
 * for the api-key exchange (`POST /api/:audience/api-key-exchange`): for the
 * browser audiences (`quarry`, `browser-broker`) ONLY through
 * `issueApiKeyAgentToken`, which `issuePlaneToken` refuses them for; for the
 * Agent API audiences (EP-12 S2) through `issuePlaneToken` /
 * `issueModelPlaneToken` with a validated key identity. No session,
 * internal-token or token-exchange path passes it.
 */
export type PlanePrincipalType = 'user' | 'service' | 'api_key';

/** The only audiences an `api_key` token is ever minted for (INNH-10). */
export const API_KEY_AGENT_AUDIENCES = [
  'quarry',
  'browser-broker',
] as const satisfies readonly PlaneAudience[];

export type ApiKeyAgentAudience = (typeof API_KEY_AGENT_AUDIENCES)[number];

const apiKeyAgentAudienceSet: ReadonlySet<string> = new Set(
  API_KEY_AGENT_AUDIENCES,
);

export function isApiKeyAgentAudience(
  value: string,
): value is ApiKeyAgentAudience {
  return apiKeyAgentAudienceSet.has(value);
}

/** OD-9: an exchanged api-key token lives at most 120 seconds. */
export const API_KEY_AGENT_TOKEN_TTL_SECONDS = API_KEY_TOKEN_MAX_TTL_SECONDS;

/** At most this many hosts in a `key_hosts` claim (the CP-2 ceiling bound). */
const API_KEY_AGENT_MAX_KEY_HOSTS = 32;

/** Bounded printable key id (the design's `key_id` rule, at most 128 bytes). */
const API_KEY_ID_PATTERN = /^[\x21-\x7e]{1,128}$/;

/**
 * Claims of an `api_key` agent token (INNH-10 design section 3). The caller
 * (the exchange) has already verified the key with Better Auth, its org
 * binding, its permission and its owner's membership.
 */
export type ApiKeyAgentJwtClaims = {
  /** Better Auth key id; `sub` is `apikey:<keyId>`. */
  keyId: string;
  /** The key's owning user, carried as `acting_user_id` (never `user_id`). */
  actingUserId: string;
  orgId: string;
  /** Non-empty; already narrowed to what the key holds. */
  scopes: readonly string[];
  /**
   * The ORG's standing posture, from `resolveInteractiveRetentionPosture`.
   * Required: an api_key token never signs without the org's own answer.
   */
  retentionPosture: InteractiveRetentionPosture;
  /** The key's own ZDR flag: raises the org posture, never lowers it. */
  keyZdr: boolean;
  /** As for a user token: absent means the baseline `{"v":1,"rev":"0"}`. */
  webPolicy?: WebPolicyClaim;
  /** The key's host ceiling; required on `browser-broker`, refused on `quarry`. */
  keyHosts?: readonly string[];
  /** Fresh `jti`, required on every api_key token. */
  tokenId: string;
  /** Only ever shortens the 120 s cap. */
  ttlSecondsOverride?: number;
};

/**
 * The `web_policy` claim a token carries, or `undefined`. See
 * `PlaneJwtClaims.webPolicy`; the 2 KiB cap fails closed here too, so no path
 * can sign an oversized claim.
 */
function issuedWebPolicy(
  audience: PlaneAudience,
  principalType: TokenPrincipalType,
  webPolicy: WebPolicyClaim | undefined,
): WebPolicyClaim | undefined {
  if (!isWebPolicyAudience(audience)) {
    if (webPolicy !== undefined) {
      throw new Error(
        `A web_policy claim cannot be carried on a ${audience} token`,
      );
    }
    return undefined;
  }
  const claim =
    principalType === 'user'
      ? (webPolicy ?? BASELINE_WEB_POLICY_CLAIM)
      : webPolicy;
  if (claim !== undefined) assertWebPolicyClaimSize(claim);
  return claim;
}

/**
 * Optional lifetime cap for a token that carries a `web_policy` claim
 * (`WEB_POLICY_TOKEN_TTL_CAP_SECONDS`). Unset by default, so the audience TTL
 * stands; a value below the 60 s floor every audience has is raised to it.
 */
function webPolicyTtlCapSeconds(): number | undefined {
  const raw = parseInt(process.env.WEB_POLICY_TOKEN_TTL_CAP_SECONDS ?? '', 10);
  if (!Number.isFinite(raw) || raw <= 0) return undefined;
  return Math.max(60, raw);
}

type PlaneTokenBundle = {
  token: string;
  expiresAt: string;
  expiresInSeconds: number;
  issuer: string;
  audience: string;
};

interface PlaneAudienceConfig {
  audience: string;
  ttlSeconds: number;
}

export type VerifiedPlaneServicePrincipal = {
  subject: string;
  serviceId: string;
  orgId: string;
  scopes: readonly string[];
  reason: string;
};

/**
 * Result of `verifyPlaneSubjectToken` — the delegated-identity token-exchange
 * subject check (auth-core §1). `audience` is the subject token's own raw
 * `aud` claim (one of `allowedAudiences`, matched during verification) so a
 * caller can gate further authorization (e.g. a registry's `tokenExchange`
 * map) on exactly which audience the subject token was issued for.
 */
export type VerifiedPlaneSubjectToken = {
  orgId: string;
  userId: string;
  zdr: boolean;
  audience: string;
};

export class PlaneTokenVerificationError extends Error {}

/**
 * Parse a TTL env var with a sane floor (60s) and fallback default. Used
 * by both the Convex- and the per-plane audience configs so a misconfigured
 * env var never produces a sub-minute token (which would make every
 * cross-plane fetch round-trip auth-core).
 */
function parsePositiveTtl(envName: string, fallback: number): number {
  const raw = parseInt(process.env[envName] ?? '', 10);
  if (!Number.isFinite(raw) || raw <= 0) {
    return fallback;
  }
  return Math.max(60, raw);
}

function exactAudience(envName: string, expected: string): string {
  const configured = (process.env[envName] ?? '').trim();
  if (configured && configured !== expected) {
    throw new Error(`${envName} must be ${expected}`);
  }
  return expected;
}

@Injectable()
export class ConvexTokenService {
  private readonly logger = new Logger(ConvexTokenService.name);
  private readonly issuer =
    process.env.CONVEX_AUTH_ISSUER ||
    `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/convex-auth`;
  private readonly audience =
    process.env.CONVEX_AUTH_AUDIENCE || 'coresystem-convex';
  private readonly kid = process.env.CONVEX_AUTH_KID || 'convex-auth-rs256';
  private readonly ttlSeconds = Math.max(
    60,
    parseInt(process.env.CONVEX_AUTH_TOKEN_TTL_SECONDS || '300', 10) || 300,
  );

  // U2-5: Model Plane gateway token settings. Reuses the same RS256 keypair
  // (so a single JWKS endpoint serves both) but with audience separation so
  // a Convex token can never authenticate against the gateway and vice versa.
  private readonly modelPlaneIssuer =
    process.env.MODEL_PLANE_AUTH_ISSUER ||
    process.env.CONVEX_AUTH_ISSUER ||
    `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/convex-auth`;
  private readonly modelPlaneAudience = exactAudience(
    'MODEL_PLANE_AUTH_AUDIENCE',
    'model-gateway',
  );
  private readonly modelPlaneTtlSeconds = Math.max(
    60,
    parseInt(process.env.MODEL_PLANE_AUTH_TOKEN_TTL_SECONDS || '900', 10) ||
      900,
  );

  // Phase A · A1.1 — non-Model-Plane audience config. Default TTL of 5 min
  // matches the Convex token; planes that need longer can override per env.
  private readonly planeIssuer =
    process.env.PLANE_TOKEN_ISSUER ||
    process.env.MODEL_PLANE_AUTH_ISSUER ||
    process.env.CONVEX_AUTH_ISSUER ||
    `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/convex-auth`;

  private readonly planeAudiences: Record<PlaneAudience, PlaneAudienceConfig> =
    {
      'data-plane': {
        audience: process.env.DATA_PLANE_AUTH_AUDIENCE || 'data-plane',
        ttlSeconds: parsePositiveTtl('PLANE_TOKEN_TTL_DATA_PLANE_SECONDS', 300),
      },
      quarry: {
        audience: process.env.QUARRY_AUTH_AUDIENCE || 'quarry',
        ttlSeconds: parsePositiveTtl('PLANE_TOKEN_TTL_QUARRY_SECONDS', 300),
      },
      ingestion: {
        audience: process.env.INGESTION_AUTH_AUDIENCE || 'ingestion',
        ttlSeconds: parsePositiveTtl('PLANE_TOKEN_TTL_INGESTION_SECONDS', 300),
      },
      'control-plane': {
        audience: process.env.CONTROL_PLANE_AUTH_AUDIENCE || 'control-plane',
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_CONTROL_PLANE_SECONDS',
          300,
        ),
      },
      'control-policy': {
        audience:
          process.env.DATA_PLANE_POLICY_AUTH_AUDIENCE || 'control-policy',
        ttlSeconds: Math.min(
          300,
          parsePositiveTtl('PLANE_TOKEN_TTL_CONTROL_POLICY_SECONDS', 300),
        ),
      },
      'application-plane': {
        audience:
          process.env.APPLICATION_PLANE_AUTH_AUDIENCE || 'application-plane',
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_APPLICATION_PLANE_SECONDS',
          300,
        ),
      },
      'session-core': {
        audience: exactAudience('SESSION_CORE_AUTH_AUDIENCE', 'session-core'),
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_SESSION_CORE_SECONDS',
          300,
        ),
      },
      'inference-core': {
        audience: exactAudience(
          'INFERENCE_CORE_AUTH_AUDIENCE',
          'inference-core',
        ),
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_INFERENCE_CORE_SECONDS',
          300,
        ),
      },
      'execution-core': {
        audience: exactAudience(
          'EXECUTION_CORE_AUTH_AUDIENCE',
          'execution-core',
        ),
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_EXECUTION_CORE_SECONDS',
          300,
        ),
      },
      'cost-core': {
        audience: exactAudience('COST_CORE_AUTH_AUDIENCE', 'cost-core'),
        ttlSeconds: parsePositiveTtl('PLANE_TOKEN_TTL_COST_CORE_SECONDS', 300),
      },
      'capability-core': {
        audience: exactAudience(
          'CAPABILITY_CORE_AUTH_AUDIENCE',
          'capability-core',
        ),
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_CAPABILITY_CORE_SECONDS',
          300,
        ),
      },
      'letta-bridge': {
        audience: exactAudience('LETTA_BRIDGE_AUTH_AUDIENCE', 'letta-bridge'),
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_LETTA_BRIDGE_SECONDS',
          300,
        ),
      },
      'browser-broker': {
        audience: exactAudience(
          'BROWSER_BROKER_AUTH_AUDIENCE',
          'browser-broker',
        ),
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_BROWSER_BROKER_SECONDS',
          300,
        ),
      },
      'sandbox-manager': {
        audience: exactAudience(
          'SANDBOX_MANAGER_AUTH_AUDIENCE',
          'sandbox-manager',
        ),
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_SANDBOX_MANAGER_SECONDS',
          300,
        ),
      },
      'bridge-core': {
        audience: exactAudience('BRIDGE_CORE_AUTH_AUDIENCE', 'bridge-core'),
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_BRIDGE_CORE_SECONDS',
          300,
        ),
      },
      // Durable workflow tier. Its StartWorkflow accepts an Auth Core JWT and
      // verifies `aud` against ORCHESTRATOR_CORE_AUTH_AUDIENCES (default
      // `orchestrator-core`), so the slug must match exactly.
      //
      // Until this entry existed no such token could be issued at all, which is
      // why every system-initiated workflow fell back to a shared secret — and a
      // shared-secret caller carries no signed retention posture, so the three
      // workflows that persist derived content refused to start.
      'orchestrator-core': {
        audience: exactAudience(
          'ORCHESTRATOR_CORE_AUTH_AUDIENCE',
          'orchestrator-core',
        ),
        ttlSeconds: parsePositiveTtl(
          'PLANE_TOKEN_TTL_ORCHESTRATOR_CORE_SECONDS',
          300,
        ),
      },
    };

  private readonly privateKey: KeyObject;
  private readonly publicKey: KeyObject;
  private readonly publicJwk: Record<string, unknown>;

  constructor() {
    const keyPair = this.loadKeyPair();
    this.privateKey = keyPair.privateKey;
    this.publicKey = keyPair.publicKey;
    this.publicJwk = {
      ...(this.publicKey.export({ format: 'jwk' }) as Record<string, unknown>),
      use: 'sig',
      alg: 'RS256',
      kid: this.kid,
    };
  }

  issueToken(claims: ConvexJwtClaims): ConvexTokenBundle {
    const now = Math.floor(Date.now() / 1000);
    const payload = {
      iss: this.issuer,
      aud: this.audience,
      sub: claims.externalAuthId,
      iat: now,
      nbf: now - 5,
      exp: now + this.ttlSeconds,
      email: claims.email,
      name: claims.name ?? undefined,
      properties: {
        externalAuthId: claims.externalAuthId,
        email: claims.email,
        name: claims.name ?? undefined,
        activeOrgId: claims.activeOrgId ?? undefined,
        activeOrgRole: claims.activeOrgRole ?? undefined,
      },
    };

    const encodedHeader = this.encodeSegment({
      alg: 'RS256',
      typ: 'JWT',
      kid: this.kid,
    });
    const encodedPayload = this.encodeSegment(payload);
    const signingInput = `${encodedHeader}.${encodedPayload}`;
    const signer = createSign('RSA-SHA256');
    signer.update(signingInput);
    signer.end();

    const signature = signer.sign(this.privateKey).toString('base64url');
    const expiresAt = new Date((now + this.ttlSeconds) * 1000).toISOString();

    return {
      token: `${signingInput}.${signature}`,
      expiresAt,
      expiresInSeconds: this.ttlSeconds,
      issuer: this.issuer,
      audience: this.audience,
    };
  }

  /**
   * U2-5: Issue an RS256 JWT for the Model Plane gateway audience.
   *
   * Required by `auth::require_auth` in
   * `apps/Model Plane/rust/services/model-gateway/src/auth.rs` — the
   * middleware deserialises into a `Claims` struct that demands
   * `org_id` + `user_id` snake_case top-level fields plus standard
   * `iss`/`exp`/`aud`. Optional fields `nbf`, `email`, and `scopes` are
   * accepted by the gateway's lenient deserialiser.
   *
   * Shares the keypair + JWKS endpoint with `issueToken()` (Convex
   * audience). Audience separation prevents cross-surface token reuse:
   *  - Convex token (`aud=coresystem-convex`) → only valid for Convex.
   *  - Model Plane token (`aud=model-gateway`) → only valid for gateway.
   */
  issueModelPlaneToken(claims: ModelPlaneJwtClaims): ModelPlaneTokenBundle {
    const principalType = claims.principalType ?? 'user';
    if (
      principalType === 'service' &&
      (!claims.serviceId || claims.userId !== `service:${claims.serviceId}`)
    ) {
      throw new Error('Service token identity is ambiguous');
    }
    if (principalType === 'user' && claims.serviceId) {
      throw new Error('User token cannot carry a service identity');
    }
    if (
      principalType !== 'api_key' &&
      (claims.keyId !== undefined || claims.actingUserId !== undefined)
    ) {
      throw new Error('Only an API-key token can carry a key identity');
    }
    if (principalType === 'api_key' && claims.stepUp) {
      throw new Error('API-key token cannot carry a step-up');
    }
    const apiKeyIdentity =
      principalType === 'api_key' ? apiKeyIdentityClaims(claims) : undefined;
    const waiver = resolveWaiverMint('model-gateway', principalType, claims);
    // A verified waiver is the one path that signs `zdr: false` for a ZDR org.
    const zdr = waiver ? false : issuedTokenZdr(principalType, claims);
    const tenantTtlCap = tenantTtlCapSeconds(principalType, claims);
    const baseTtlSeconds =
      apiKeyIdentity !== undefined
        ? apiKeyTtlSeconds(
            this.modelPlaneTtlSeconds,
            claims.ttlSecondsOverride,
          )
        : tenantTtlCap === undefined
          ? this.modelPlaneTtlSeconds
          : Math.min(this.modelPlaneTtlSeconds, tenantTtlCap);
    const ttlSeconds = waiver
      ? Math.min(baseTtlSeconds, waiver.ttlCapSeconds)
      : baseTtlSeconds;
    const now = Math.floor(Date.now() / 1000);
    const payload = {
      iss: this.modelPlaneIssuer,
      aud: this.modelPlaneAudience,
      sub: claims.userId,
      iat: now,
      nbf: now - 5,
      exp: now + ttlSeconds,
      // The gateway's `Claims` struct (auth.rs) reads these snake_case fields
      // as required top-level claims.
      org_id: claims.orgId,
      ...(principalType === 'user' ? { user_id: claims.userId } : {}),
      principal_type: principalType,
      ...(principalType === 'service' ? { service_id: claims.userId } : {}),
      ...(apiKeyIdentity ?? {}),
      ...(claims.reason ? { reason: claims.reason } : {}),
      // Optional extras — accepted by the gateway but not required.
      ...(claims.email ? { email: claims.email } : {}),
      ...(claims.scopes && claims.scopes.length > 0
        ? { scopes: claims.scopes }
        : {}),
      ...(claims.stepUp ? { step_up: claims.stepUp } : {}),
      // Interactive posture comes from Auth Core's exact organization policy;
      // service posture comes from the bounded service-principal registry, or,
      // for a `tenant` principal, from the same organization policy resolved at
      // mint time (the caller stamps it; see `tenantTtlCapSeconds`).
      zdr,
      // ZDR-Z-14b: the org is ZDR (`org_zdr`) and one connection is waived for
      // this turn (`zdr_waiver`); both ride only with a verified decision.
      ...(waiver ? { org_zdr: true, zdr_waiver: waiver.zdrWaiver } : {}),
    };

    const encodedHeader = this.encodeSegment({
      alg: 'RS256',
      typ: 'JWT',
      kid: this.kid,
    });
    const encodedPayload = this.encodeSegment(payload);
    const signingInput = `${encodedHeader}.${encodedPayload}`;
    const signer = createSign('RSA-SHA256');
    signer.update(signingInput);
    signer.end();

    const signature = signer.sign(this.privateKey).toString('base64url');
    const expiresAt = new Date((now + ttlSeconds) * 1000).toISOString();

    return {
      token: `${signingInput}.${signature}`,
      expiresAt,
      expiresInSeconds: ttlSeconds,
      issuer: this.modelPlaneIssuer,
      audience: this.modelPlaneAudience,
    };
  }

  /**
   * Phase A · A1.1 — issue an RS256 JWT for a non-Model-Plane audience.
   *
   * Mirrors {@link issueModelPlaneToken} but with a per-audience `aud` /
   * TTL pair and a slightly different claim shape: the receiving plane
   * middlewares look up `org_id` + `user_id` snake_case (consistent with
   * the model-gateway pattern) for unambiguous tenant resolution.
   *
   * Keypair is shared across audiences so a single JWKS at
   * `/api/convex-auth/jwks` covers all of them. Tokens are mutually
   * exclusive: each plane's middleware validates `aud` against its own
   * expected value, so a quarry-scoped token cannot authenticate a
   * data-plane call (and vice versa).
   */
  issuePlaneToken(
    audience: PlaneAudience,
    claims: PlaneJwtClaims,
  ): PlaneTokenBundle {
    const config = this.planeAudiences[audience];
    if (!config) {
      throw new Error(`Unknown plane audience: ${audience}`);
    }
    const principalType = claims.principalType ?? 'user';
    // INNH-10 CP-4 / EP-12 S2: `user` and `service`, plus `api_key` for the
    // Agent API audiences only, and this runtime check holds the line against a
    // cast. A browser-audience (`quarry`, `browser-broker`) `api_key` token
    // carries `web_policy` and `key_hosts`, so it is minted by
    // `issueApiKeyAgentToken` alone.
    if (
      principalType !== 'user' &&
      principalType !== 'service' &&
      !(principalType === 'api_key' && !isApiKeyAgentAudience(audience))
    ) {
      throw new Error(
        `Principal type ${String(principalType)} cannot be minted by issuePlaneToken for ${audience}`,
      );
    }
    if (
      principalType === 'service' &&
      (!claims.serviceId || claims.userId !== `service:${claims.serviceId}`)
    ) {
      throw new Error('Service token identity is ambiguous');
    }
    if (principalType === 'user' && claims.serviceId) {
      throw new Error('User token cannot carry a service identity');
    }
    if (principalType !== 'api_key' && claims.userId.startsWith('apikey:')) {
      throw new Error('An api_key subject cannot be minted by issuePlaneToken');
    }
    if (
      claims.actingUserId !== undefined &&
      principalType !== 'service' &&
      principalType !== 'api_key'
    ) {
      throw new Error(
        'Acting-user identity can only be carried on a service token',
      );
    }
    if (claims.keyId !== undefined && principalType !== 'api_key') {
      throw new Error('Only an API-key token can carry a key identity');
    }
    const apiKeyIdentity =
      principalType === 'api_key' ? apiKeyIdentityClaims(claims) : undefined;
    const waiver = resolveWaiverMint(audience, principalType, claims);
    // A verified waiver is the one path that signs `zdr: false` for a ZDR org.
    const zdr = waiver ? false : issuedTokenZdr(principalType, claims);
    const webPolicy = issuedWebPolicy(
      audience,
      principalType,
      claims.webPolicy,
    );
    // Never lengthens a token beyond the audience's configured default —
    // only a strictly shorter override is honored, so a caller-supplied
    // value can only narrow the exposure window, never widen it.
    const requestedTtlSeconds =
      claims.ttlSecondsOverride !== undefined &&
      Number.isFinite(claims.ttlSecondsOverride) &&
      claims.ttlSecondsOverride > 0 &&
      claims.ttlSecondsOverride < config.ttlSeconds
        ? claims.ttlSecondsOverride
        : config.ttlSeconds;
    // A token whose posture is a snapshot of the org's setting (a tenant
    // principal) also never outlives the snapshot's bound.
    const tenantTtlCap = tenantTtlCapSeconds(principalType, claims);
    // An `api_key` token is capped at API_KEY_TOKEN_MAX_TTL_SECONDS; a tenant
    // principal never outlives its posture snapshot.
    const tenantCappedTtlSeconds =
      apiKeyIdentity !== undefined
        ? apiKeyTtlSeconds(config.ttlSeconds, claims.ttlSecondsOverride)
        : tenantTtlCap === undefined
          ? requestedTtlSeconds
          : Math.min(requestedTtlSeconds, tenantTtlCap);
    const webPolicyTtlCap =
      webPolicy === undefined ? undefined : webPolicyTtlCapSeconds();
    const webPolicyCappedTtlSeconds =
      webPolicyTtlCap === undefined
        ? tenantCappedTtlSeconds
        : Math.min(tenantCappedTtlSeconds, webPolicyTtlCap);
    const ttlSeconds = waiver
      ? Math.min(webPolicyCappedTtlSeconds, waiver.ttlCapSeconds)
      : webPolicyCappedTtlSeconds;
    const now = Math.floor(Date.now() / 1000);
    const payload = {
      iss: this.planeIssuer,
      aud: config.audience,
      sub: claims.userId,
      iat: now,
      nbf: now - 5,
      exp: now + ttlSeconds,
      org_id: claims.orgId,
      ...(principalType === 'user' ? { user_id: claims.userId } : {}),
      principal_type: principalType,
      ...(principalType === 'service' ? { service_id: claims.userId } : {}),
      ...(principalType === 'service' && claims.actingUserId
        ? { acting_user_id: claims.actingUserId }
        : {}),
      ...(claims.tokenId ? { jti: claims.tokenId } : {}),
      // `key_id`, `acting_user_id` and the same `jti`, validated above.
      ...(apiKeyIdentity ?? {}),
      ...(claims.reason ? { reason: claims.reason } : {}),
      ...(claims.email ? { email: claims.email } : {}),
      ...(claims.scopes && claims.scopes.length > 0
        ? { scopes: claims.scopes }
        : {}),
      // Interactive posture comes from Auth Core's exact organization policy;
      // service posture comes from the bounded service-principal registry, or,
      // for a `tenant` principal, from the same organization policy resolved at
      // mint time (the caller stamps it; see `tenantTtlCapSeconds`).
      zdr,
      // Execution's browser-attachment receipt may persist only when Control
      // actually resolved the interactive organization's standing posture.
      // Do not infer this claim from `zdr`: legacy user issuance can default
      // that value to false when no posture was provided. Service principal
      // policy and API-key posture are not proof of an interactive org choice.
      // ZDR-Z-14b: a verified waiver stamps the org as ZDR and names the one
      // waived connection; it replaces the `org_zdr: false` proof below, which
      // needs a non-ZDR org and so can never apply beside a waiver.
      ...(waiver
        ? { org_zdr: true, zdr_waiver: waiver.zdrWaiver }
        : {}),
      ...(!waiver &&
      audience === 'execution-core' &&
      principalType === 'user' &&
      claims.retentionPosture?.authority ===
        'interactive-org-retention-policy' &&
      claims.retentionPosture.zdr === false &&
      claims.executionRetentionProof?.authority === 'fresh-interactive-org-retention-policy' &&
      claims.executionRetentionProof.orgId === claims.orgId &&
      claims.executionRetentionProof.zdr === false
        ? { org_zdr: false }
        : {}),
      // WS7.I: the org web policy, beside and independent of `zdr`.
      ...(webPolicy !== undefined ? { web_policy: webPolicy } : {}),
    };

    const encodedHeader = this.encodeSegment({
      alg: 'RS256',
      typ: 'JWT',
      kid: this.kid,
    });
    const encodedPayload = this.encodeSegment(payload);
    const signingInput = `${encodedHeader}.${encodedPayload}`;
    const signer = createSign('RSA-SHA256');
    signer.update(signingInput);
    signer.end();

    const signature = signer.sign(this.privateKey).toString('base64url');
    const expiresAt = new Date((now + ttlSeconds) * 1000).toISOString();

    return {
      token: `${signingInput}.${signature}`,
      expiresAt,
      expiresInSeconds: ttlSeconds,
      issuer: this.planeIssuer,
      audience: config.audience,
    };
  }

  /**
   * INNH-10 CP-1 / CP-4: the ONLY minter of a `principal_type: "api_key"`
   * token, called only by the api-key exchange
   * (`POST /api/:audience/api-key-exchange`). The token is the design's
   * section 3 shape: `sub = apikey:<key_id>`, `key_id`, `acting_user_id` (the
   * key's owner), no `user_id`, no `service_id`, a fresh `jti`, at most 120 s,
   * `zdr` = org posture OR the key's flag (raise only), `web_policy` as for a
   * user token, and `key_hosts` on the `browser-broker` audience only.
   * Every rule fails closed by throwing: no token.
   */
  issueApiKeyAgentToken(
    audience: ApiKeyAgentAudience,
    claims: ApiKeyAgentJwtClaims,
  ): PlaneTokenBundle {
    if (!isApiKeyAgentAudience(audience)) {
      throw new Error(
        `An api_key token cannot be minted for ${String(audience)}`,
      );
    }
    const config = this.planeAudiences[audience];
    if (!config) {
      throw new Error(`Unknown plane audience: ${audience}`);
    }
    const keyId = typeof claims.keyId === 'string' ? claims.keyId : '';
    if (!API_KEY_ID_PATTERN.test(keyId)) {
      throw new Error('An api_key token needs a bounded printable key id');
    }
    const actingUserId =
      typeof claims.actingUserId === 'string' ? claims.actingUserId.trim() : '';
    if (!actingUserId || actingUserId.startsWith('apikey:')) {
      throw new Error('An api_key token needs the key owner as acting user');
    }
    const orgId = typeof claims.orgId === 'string' ? claims.orgId.trim() : '';
    if (!orgId) {
      throw new Error('An api_key token needs an organization');
    }
    const tokenId =
      typeof claims.tokenId === 'string' ? claims.tokenId.trim() : '';
    if (!tokenId) {
      throw new Error('An api_key token needs a jti');
    }
    if (
      !Array.isArray(claims.scopes) ||
      claims.scopes.length === 0 ||
      claims.scopes.some((scope) => typeof scope !== 'string' || !scope)
    ) {
      throw new Error('An api_key token needs at least one scope');
    }
    if (
      claims.retentionPosture?.authority !==
        'interactive-org-retention-policy' ||
      typeof claims.retentionPosture.zdr !== 'boolean'
    ) {
      throw new Error('An api_key token needs the organization retention posture');
    }
    if (audience === 'browser-broker') {
      if (
        !Array.isArray(claims.keyHosts) ||
        claims.keyHosts.length === 0 ||
        claims.keyHosts.length > API_KEY_AGENT_MAX_KEY_HOSTS ||
        claims.keyHosts.some((host) => typeof host !== 'string' || !host)
      ) {
        throw new Error(
          'A browser-broker api_key token needs the key host ceiling',
        );
      }
    } else if (claims.keyHosts !== undefined) {
      throw new Error('key_hosts is carried on browser-broker tokens only');
    }
    // Raise only: the key can add ZDR to a non-ZDR org, never take it away.
    const zdr = claims.retentionPosture.zdr || claims.keyZdr === true;
    const webPolicy = issuedWebPolicy(audience, 'user', claims.webPolicy);
    const override = claims.ttlSecondsOverride;
    const cappedTtlSeconds = Math.min(
      config.ttlSeconds,
      API_KEY_AGENT_TOKEN_TTL_SECONDS,
    );
    const requestedTtlSeconds =
      override !== undefined &&
      Number.isFinite(override) &&
      override >= 1 &&
      override < cappedTtlSeconds
        ? Math.floor(override)
        : cappedTtlSeconds;
    const webPolicyTtlCap = webPolicyTtlCapSeconds();
    const ttlSeconds =
      webPolicyTtlCap === undefined
        ? requestedTtlSeconds
        : Math.min(requestedTtlSeconds, webPolicyTtlCap);
    const now = Math.floor(Date.now() / 1000);
    const principalType: PlanePrincipalType = 'api_key';
    const payload = {
      iss: this.planeIssuer,
      aud: config.audience,
      sub: `apikey:${keyId}`,
      iat: now,
      nbf: now - 5,
      exp: now + ttlSeconds,
      org_id: orgId,
      principal_type: principalType,
      key_id: keyId,
      acting_user_id: actingUserId,
      jti: tokenId,
      scopes: [...claims.scopes],
      zdr,
      ...(webPolicy !== undefined ? { web_policy: webPolicy } : {}),
      ...(audience === 'browser-broker' && claims.keyHosts
        ? { key_hosts: [...claims.keyHosts] }
        : {}),
    };
    return this.signPlanePayload(payload, now, ttlSeconds, config.audience);
  }

  private signPlanePayload(
    payload: Record<string, unknown>,
    now: number,
    ttlSeconds: number,
    audience: string,
  ): PlaneTokenBundle {
    const encodedHeader = this.encodeSegment({
      alg: 'RS256',
      typ: 'JWT',
      kid: this.kid,
    });
    const encodedPayload = this.encodeSegment(payload);
    const signingInput = `${encodedHeader}.${encodedPayload}`;
    const signer = createSign('RSA-SHA256');
    signer.update(signingInput);
    signer.end();

    const signature = signer.sign(this.privateKey).toString('base64url');
    const expiresAt = new Date((now + ttlSeconds) * 1000).toISOString();

    return {
      token: `${signingInput}.${signature}`,
      expiresAt,
      expiresInSeconds: ttlSeconds,
      issuer: this.planeIssuer,
      audience,
    };
  }

  /**
   * Verify a service-principal token issued by this authority. This is used by
   * sensitive Control Plane callbacks that must not trust shared keys or
   * caller-selected identity headers. Signature, algorithm, key id, issuer,
   * audience, lifetime, and canonical service-identity claims are all required.
   */
  verifyPlaneServiceToken(
    audience: PlaneAudience,
    token: string,
  ): VerifiedPlaneServicePrincipal {
    const config = this.planeAudiences[audience];
    if (!config || !token) {
      throw new PlaneTokenVerificationError('Plane token is not verifiable');
    }

    try {
      const segments = token.split('.');
      if (
        segments.length !== 3 ||
        segments.some(
          (segment) => !segment || !/^[A-Za-z0-9_-]+$/.test(segment),
        )
      ) {
        throw new PlaneTokenVerificationError('Plane token is malformed');
      }
      const [encodedHeader, encodedPayload, encodedSignature] = segments;
      const protectedHeader = JSON.parse(
        Buffer.from(encodedHeader, 'base64url').toString('utf8'),
      ) as Record<string, unknown>;
      const payload = JSON.parse(
        Buffer.from(encodedPayload, 'base64url').toString('utf8'),
      ) as Record<string, unknown>;
      const signature = Buffer.from(encodedSignature, 'base64url');
      const verifier = createVerify('RSA-SHA256');
      verifier.update(`${encodedHeader}.${encodedPayload}`);
      verifier.end();

      if (
        protectedHeader.alg !== 'RS256' ||
        protectedHeader.typ !== 'JWT' ||
        protectedHeader.kid !== this.kid ||
        !verifier.verify(this.publicKey, signature)
      ) {
        throw new PlaneTokenVerificationError('Plane token key is not trusted');
      }

      const subject = payload.sub;
      const orgId = payload.org_id;
      const serviceId = payload.service_id;
      const scopes = payload.scopes;
      const reason = payload.reason;
      const numericDates = [payload.iat, payload.nbf, payload.exp];
      const now = Math.floor(Date.now() / 1000);
      if (
        payload.iss !== this.planeIssuer ||
        payload.aud !== config.audience ||
        payload.principal_type !== 'service' ||
        payload.user_id !== undefined ||
        typeof subject !== 'string' ||
        !subject.startsWith('service:') ||
        subject.trim() !== subject ||
        typeof serviceId !== 'string' ||
        serviceId !== subject ||
        typeof orgId !== 'string' ||
        !orgId.trim() ||
        orgId.trim() !== orgId ||
        !Array.isArray(scopes) ||
        scopes.length === 0 ||
        scopes.some(
          (scope) =>
            typeof scope !== 'string' ||
            !scope.trim() ||
            scope.trim() !== scope,
        ) ||
        typeof reason !== 'string' ||
        reason.trim() !== reason ||
        reason.length < 3 ||
        reason.length > 500 ||
        numericDates.some(
          (value) => !Number.isSafeInteger(value) || (value as number) <= 0,
        ) ||
        (payload.nbf as number) > now ||
        (payload.iat as number) > now + 5 ||
        (payload.exp as number) <= now ||
        (payload.nbf as number) > (payload.iat as number) ||
        (payload.iat as number) >= (payload.exp as number)
      ) {
        throw new PlaneTokenVerificationError(
          'Plane service principal claims are invalid',
        );
      }

      return {
        subject,
        serviceId,
        orgId,
        scopes: [...new Set(scopes as string[])],
        reason,
      };
    } catch (error) {
      if (error instanceof PlaneTokenVerificationError) {
        throw error;
      }
      throw new PlaneTokenVerificationError('Plane token verification failed');
    }
  }

  /**
   * Delegated-identity token exchange, subject-token half (auth-core §1).
   * Verifies a `principal_type=="user"` token THIS authority already issued
   * (same signature/kid check as `verifyPlaneServiceToken`), requires it be
   * *currently* valid (`nbf <= now < exp`, not just well-formed), and
   * requires its `aud` be one of `allowedAudiences`.
   *
   * Deliberately NOT single-use. execution-core holds ONE session-core bearer
   * per run and exchanges it once per tool call, so a seen-set on the subject
   * token would let a run make exactly one delegated read and then fail every
   * later one (and a refused attempt would burn the bearer for the legitimate
   * caller). Replay of the exchanged token (T-2/A-1) is closed where that
   * token is actually presented instead: each one carries its own `jti`
   * (`PlaneJwtClaims.tokenId`) and integration-corev2 consumes it exactly
   * once. The subject token stays bounded by its own short TTL and by the
   * caller also having to present the service credential.
   *
   * Returns the subject's own `{orgId, userId, zdr}` — callers must use
   * THESE values (never the registry's own defaults) for the exchanged
   * token's org/acting-user/zdr claims.
   */
  verifyPlaneSubjectToken(
    token: string,
    allowedAudiences: readonly PlaneAudience[],
  ): VerifiedPlaneSubjectToken {
    if (!token || allowedAudiences.length === 0) {
      throw new PlaneTokenVerificationError(
        'Plane subject token is not verifiable',
      );
    }
    const allowedAudienceValues = new Set(
      allowedAudiences
        .map((audience) => this.planeAudiences[audience]?.audience)
        .filter((value): value is string => typeof value === 'string'),
    );
    if (allowedAudienceValues.size === 0) {
      throw new PlaneTokenVerificationError(
        'Plane subject token audience allow-list is empty',
      );
    }

    try {
      const segments = token.split('.');
      if (
        segments.length !== 3 ||
        segments.some(
          (segment) => !segment || !/^[A-Za-z0-9_-]+$/.test(segment),
        )
      ) {
        throw new PlaneTokenVerificationError(
          'Plane subject token is malformed',
        );
      }
      const [encodedHeader, encodedPayload, encodedSignature] = segments;
      const protectedHeader = JSON.parse(
        Buffer.from(encodedHeader, 'base64url').toString('utf8'),
      ) as Record<string, unknown>;
      const payload = JSON.parse(
        Buffer.from(encodedPayload, 'base64url').toString('utf8'),
      ) as Record<string, unknown>;
      const signature = Buffer.from(encodedSignature, 'base64url');
      const verifier = createVerify('RSA-SHA256');
      verifier.update(`${encodedHeader}.${encodedPayload}`);
      verifier.end();

      if (
        protectedHeader.alg !== 'RS256' ||
        protectedHeader.typ !== 'JWT' ||
        protectedHeader.kid !== this.kid ||
        !verifier.verify(this.publicKey, signature)
      ) {
        throw new PlaneTokenVerificationError(
          'Plane subject token key is not trusted',
        );
      }

      const audience = payload.aud;
      const subject = payload.sub;
      const orgId = payload.org_id;
      const userId = payload.user_id;
      const zdr = payload.zdr;
      const numericDates = [payload.iat, payload.nbf, payload.exp];
      const now = Math.floor(Date.now() / 1000);
      if (
        payload.iss !== this.planeIssuer ||
        typeof audience !== 'string' ||
        !allowedAudienceValues.has(audience) ||
        payload.principal_type !== 'user' ||
        payload.service_id !== undefined ||
        typeof subject !== 'string' ||
        subject.startsWith('service:') ||
        typeof userId !== 'string' ||
        !userId.trim() ||
        userId !== subject ||
        typeof orgId !== 'string' ||
        !orgId.trim() ||
        orgId.trim() !== orgId ||
        typeof zdr !== 'boolean' ||
        numericDates.some(
          (value) => !Number.isSafeInteger(value) || (value as number) <= 0,
        ) ||
        (payload.nbf as number) > now ||
        (payload.iat as number) > now + 5 ||
        (payload.exp as number) <= now ||
        (payload.nbf as number) > (payload.iat as number) ||
        (payload.iat as number) >= (payload.exp as number)
      ) {
        throw new PlaneTokenVerificationError(
          'Plane subject token claims are invalid',
        );
      }

      return { orgId, userId, zdr, audience };
    } catch (error) {
      if (error instanceof PlaneTokenVerificationError) {
        throw error;
      }
      throw new PlaneTokenVerificationError(
        'Plane subject token verification failed',
      );
    }
  }

  /**
   * Phase A · A1.1 — guard exposed to controllers so they can validate
   * the audience slug from the request path against the configured set
   * before touching the keypair. Returns `null` if the audience is
   * unknown, allowing the controller to surface a clean 404/400.
   */
  isKnownPlaneAudience(value: string): value is PlaneAudience {
    return Object.hasOwn(this.planeAudiences, value);
  }

  isKnownAuthAudience(value: string): value is AuthCoreAudience {
    return value === 'model-gateway' || this.isKnownPlaneAudience(value);
  }

  /**
   * Interactive (session-cookie) gate for `GET /api/:audience/token`. Fails
   * closed for unknown slugs AND for `SERVICE_ONLY_PLANE_AUDIENCES`, so the
   * runtime answer matches the `InteractivePlaneAudience` type by construction.
   */
  isInteractivePlaneAudience(value: string): value is InteractivePlaneAudience {
    return (
      this.isKnownPlaneAudience(value) &&
      !serviceOnlyPlaneAudienceSet.has(value)
    );
  }

  getJwks() {
    return {
      planeTokenIssuer: this.planeIssuer,
      keys: [this.publicJwk],
    };
  }

  /**
   * SPKI DER of the token signing public key. Exposed only so the dedicated
   * retention-decision signer can prove its key differs from this one (a leaked
   * token key must never be able to mint a waiver). It carries no private
   * material and is not published anywhere that the JWKS does not already.
   */
  tokenSigningPublicKeySpki(): Buffer {
    return this.publicKey.export({ type: 'spki', format: 'der' }) as Buffer;
  }

  private loadKeyPair() {
    // Precedence: mounted key files (secrets-as-files; survives container
    // recreation so the JWKS at /api/convex-auth/jwks is STABLE and downstream
    // verifiers can pin the public key) → inline PEM env → ephemeral (dev only).
    // File-based keys avoid the docker `env_file` multiline-PEM limitation and
    // keep private-key material out of env/compose (and out of git).
    const privatePem =
      this.readKeyFile(process.env.CONVEX_AUTH_PRIVATE_KEY_FILE) ??
      process.env.CONVEX_AUTH_PRIVATE_KEY_PEM;
    const publicPem =
      this.readKeyFile(process.env.CONVEX_AUTH_PUBLIC_KEY_FILE) ??
      process.env.CONVEX_AUTH_PUBLIC_KEY_PEM;

    if (privatePem && publicPem) {
      const privateKey = createPrivateKey(privatePem);
      const publicKey = createPublicKey(publicPem);
      const privateModulusLength =
        privateKey.asymmetricKeyDetails?.modulusLength;
      const publicModulusLength = publicKey.asymmetricKeyDetails?.modulusLength;
      if (
        privateKey.asymmetricKeyType !== 'rsa' ||
        publicKey.asymmetricKeyType !== 'rsa' ||
        typeof privateModulusLength !== 'number' ||
        typeof publicModulusLength !== 'number' ||
        privateModulusLength < 2048 ||
        publicModulusLength < 2048
      ) {
        throw new Error(
          'Configured RS256 signing keys must be RSA keys with at least 2048 bits',
        );
      }
      const derivedPublicKey = createPublicKey(privateKey).export({
        type: 'spki',
        format: 'der',
      });
      const configuredPublicKey = publicKey.export({
        type: 'spki',
        format: 'der',
      });
      if (!derivedPublicKey.equals(configuredPublicKey)) {
        throw new Error('Configured RS256 signing keypair does not match');
      }
      return {
        privateKey,
        publicKey,
      };
    }

    if (privatePem || publicPem) {
      throw new Error(
        'A complete stable RS256 signing keypair is required; both private and public keys must be configured',
      );
    }

    if (process.env.NODE_ENV === 'production') {
      throw new Error(
        'A stable RS256 signing keypair is required in production',
      );
    }

    const message =
      'CONVEX_AUTH_PRIVATE_KEY_FILE/PEM + CONVEX_AUTH_PUBLIC_KEY_FILE/PEM not set; generating ephemeral RSA keypair for local development (JWKS rotates on every restart — set a stable key for production).';
    this.logger.log(message);

    const generated = generateKeyPairSync('rsa', {
      modulusLength: 2048,
    });

    return {
      privateKey: generated.privateKey,
      publicKey: generated.publicKey,
    };
  }

  /**
   * Read a PEM key from a mounted file path. Returns `undefined` when the env
   * var is unset/empty. An explicitly configured but unreadable mount is a
   * release error and must never fall through to another key source.
   */
  private readKeyFile(path: string | undefined): string | undefined {
    const trimmed = (path ?? '').trim();
    if (!trimmed) {
      return undefined;
    }
    try {
      const pem = readFileSync(trimmed, 'utf8').trim();
      if (!pem) {
        throw new Error('empty signing key file');
      }
      return pem;
    } catch {
      throw new Error('Configured signing key file is not readable');
    }
  }

  private encodeSegment(value: unknown) {
    return Buffer.from(JSON.stringify(value)).toString('base64url');
  }
}
