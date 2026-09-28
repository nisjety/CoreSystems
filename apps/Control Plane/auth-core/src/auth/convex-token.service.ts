import { Injectable, Logger } from '@nestjs/common';
import {
  createHash,
  createPrivateKey,
  createPublicKey,
  createSign,
  createVerify,
  generateKeyPairSync,
  type KeyObject,
} from 'crypto';
import { readFileSync } from 'fs';
import type { InteractiveRetentionPosture } from './interactive-retention-policy';

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
  principalType?: 'user' | 'service';
  serviceId?: string;
  reason?: string;
  retentionPosture?: ServiceRetentionPosture | InteractiveRetentionPosture;
};

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
 * Audiences a signed-in USER may mint a token for through the interactive
 * session route.
 *
 * `control-policy` is excluded because it is a machine-to-machine decision
 * surface. `orchestrator-core` is excluded for a sharper reason: its
 * StartWorkflow lets a NON-service caller start any run-scoped workflow
 * (`authorizePolicy`'s `PolicyRunScoped` returns early for `!caller.Service`).
 * People already start runs through model-gateway, so an interactive
 * orchestrator-core token would add a way to drive the durable tier directly
 * with no product need for it. Service principals are unaffected — they mint
 * through the internal-token route, which is not gated on this type.
 */
export type InteractivePlaneAudience = Exclude<
  PlaneAudience,
  'control-policy' | 'orchestrator-core'
>;

type ServiceRetentionPosture = {
  zdr: boolean;
  authority: 'service-principal-policy';
};

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
 */
function issuedTokenZdr(
  principalType: 'user' | 'service',
  claims: RetentionAwareClaims,
): boolean {
  if (principalType === 'user') {
    return claims.retentionPosture?.authority ===
      'interactive-org-retention-policy'
      ? claims.retentionPosture.zdr
      : false;
  }
  return claims.retentionPosture?.authority === 'service-principal-policy'
    ? claims.retentionPosture.zdr
    : true;
}

type PlaneJwtClaims = {
  userId: string;
  orgId: string;
  email?: string;
  scopes?: readonly string[];
  principalType?: 'user' | 'service';
  serviceId?: string;
  reason?: string;
  retentionPosture?: ServiceRetentionPosture | InteractiveRetentionPosture;
  /**
   * Delegated-identity token exchange (auth-core §1, `verifyPlaneSubjectToken`
   * / `/api/:audience/token-exchange`). Set only on the exchanged service
   * token so the downstream consumer (e.g. integration-corev2's
   * `VerifyToken`) can read a real, cryptographically-bound acting user
   * alongside the service identity. `principal_type` stays `"service"` —
   * this is an additive claim, not a different token shape.
   */
  actingUserId?: string;
  /**
   * Lets one call site mint a token shorter-lived than the audience's
   * configured default (never longer — any value `>=` the audience default
   * is ignored). Used by the token-exchange endpoint to mint a 90s token
   * distinct from `/internal-token`'s normal TTL for the same audience.
   */
  ttlSecondsOverride?: number;
};

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

  /**
   * Single-use marker for `verifyPlaneSubjectToken` (delegated-identity
   * token exchange, §1/T-2). Keyed by a SHA-256 digest of the raw subject
   * token string (subject tokens mint with no `jti` today, so the full
   * token is the only value guaranteed unique per mint — its `iat` makes
   * two independently-minted tokens for the same claims collide only if
   * minted within the same wall-clock second, which `nbf`/`exp` bounds
   * already require to also match, i.e. never in practice for two distinct
   * mints). In-memory and process-local by design: it only has to survive
   * the token's own short TTL, not a restart — mirrors the pattern
   * `conversation-ingest-rs`'s `ReplayCache` already uses elsewhere in this
   * codebase. Value is the token's own `exp` in epoch milliseconds, used to
   * prune stale entries so the map never grows unbounded.
   */
  private readonly subjectTokenExchangeSeen = new Map<string, number>();

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
    const zdr = issuedTokenZdr(principalType, claims);
    const now = Math.floor(Date.now() / 1000);
    const payload = {
      iss: this.modelPlaneIssuer,
      aud: this.modelPlaneAudience,
      sub: claims.userId,
      iat: now,
      nbf: now - 5,
      exp: now + this.modelPlaneTtlSeconds,
      // The gateway's `Claims` struct (auth.rs) reads these snake_case fields
      // as required top-level claims.
      org_id: claims.orgId,
      ...(principalType === 'user' ? { user_id: claims.userId } : {}),
      principal_type: principalType,
      ...(principalType === 'service' ? { service_id: claims.userId } : {}),
      ...(claims.reason ? { reason: claims.reason } : {}),
      // Optional extras — accepted by the gateway but not required.
      ...(claims.email ? { email: claims.email } : {}),
      ...(claims.scopes && claims.scopes.length > 0
        ? { scopes: claims.scopes }
        : {}),
      // Interactive posture comes from Auth Core's exact organization policy;
      // service posture comes only from the bounded service-principal registry.
      zdr,
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
    const expiresAt = new Date(
      (now + this.modelPlaneTtlSeconds) * 1000,
    ).toISOString();

    return {
      token: `${signingInput}.${signature}`,
      expiresAt,
      expiresInSeconds: this.modelPlaneTtlSeconds,
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
    if (
      principalType === 'service' &&
      (!claims.serviceId || claims.userId !== `service:${claims.serviceId}`)
    ) {
      throw new Error('Service token identity is ambiguous');
    }
    if (principalType === 'user' && claims.serviceId) {
      throw new Error('User token cannot carry a service identity');
    }
    if (claims.actingUserId !== undefined && principalType !== 'service') {
      throw new Error(
        'Acting-user identity can only be carried on a service token',
      );
    }
    const zdr = issuedTokenZdr(principalType, claims);
    // Never lengthens a token beyond the audience's configured default —
    // only a strictly shorter override is honored, so a caller-supplied
    // value can only narrow the exposure window, never widen it.
    const ttlSeconds =
      claims.ttlSecondsOverride !== undefined &&
      Number.isFinite(claims.ttlSecondsOverride) &&
      claims.ttlSecondsOverride > 0 &&
      claims.ttlSecondsOverride < config.ttlSeconds
        ? claims.ttlSecondsOverride
        : config.ttlSeconds;
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
      ...(claims.reason ? { reason: claims.reason } : {}),
      ...(claims.email ? { email: claims.email } : {}),
      ...(claims.scopes && claims.scopes.length > 0
        ? { scopes: claims.scopes }
        : {}),
      // Interactive posture comes from Auth Core's exact organization policy;
      // service posture comes only from the bounded service-principal registry.
      zdr,
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

  private pruneSubjectTokenExchangeSeen(now: number): void {
    for (const [key, expiresAtMs] of this.subjectTokenExchangeSeen) {
      if (expiresAtMs <= now) {
        this.subjectTokenExchangeSeen.delete(key);
      }
    }
  }

  /**
   * Delegated-identity token exchange, subject-token half (auth-core §1).
   * Verifies a `principal_type=="user"` token THIS authority already issued
   * (same signature/kid check as `verifyPlaneServiceToken`), requires it be
   * *currently* valid (`nbf <= now < exp`, not just well-formed), requires
   * its `aud` be one of `allowedAudiences`, and enforces single-use: a
   * second call with the identical raw token — even a legitimately valid
   * one — is rejected for as long as the token itself remains unexpired
   * (closes T-2/A-1: the exchanged token this subject token is about to
   * mint is a complete standalone bearer, so limiting the subject token to
   * one exchange bounds the blast radius of a captured copy to a single
   * grant instead of unlimited re-mints within the token's window).
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

      const seenKey = createHash('sha256').update(token).digest('hex');
      this.pruneSubjectTokenExchangeSeen(Date.now());
      if (this.subjectTokenExchangeSeen.has(seenKey)) {
        throw new PlaneTokenVerificationError(
          'Plane subject token has already been exchanged',
        );
      }
      this.subjectTokenExchangeSeen.set(
        seenKey,
        (payload.exp as number) * 1000,
      );

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

  isInteractivePlaneAudience(value: string): value is InteractivePlaneAudience {
    return this.isKnownPlaneAudience(value) && value !== 'control-policy';
  }

  getJwks() {
    return {
      planeTokenIssuer: this.planeIssuer,
      keys: [this.publicJwk],
    };
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
