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
  /** Secure-MVP fallback until Control owns an authoritative org ZDR policy. */
  zdr: true;
  email?: string;
  scopes?: readonly string[];
  principalType?: 'user' | 'service';
  serviceId?: string;
  reason?: string;
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
 * Audience names match the path slug (and the velion `PlaneAudience`
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
  | 'bridge-core';

export type AuthCoreAudience = 'model-gateway' | PlaneAudience;
export type InteractivePlaneAudience = Exclude<PlaneAudience, 'control-policy'>;

type PlaneJwtClaims = {
  userId: string;
  orgId: string;
  email?: string;
  scopes?: readonly string[];
  principalType?: 'user' | 'service';
  serviceId?: string;
  reason?: string;
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
      // Control Plane has no authoritative org-level ZDR policy source yet.
      // Historically this failed closed (unconditional zdr:true) for every Model
      // token. Operator decision 2026-07-14: with no org ZDR policy and no
      // ZDR-attested provider configured, forcing ZDR blocked ALL chat
      // (zdr_inference_failed). Default is now relaxed but env-gated — set
      // AUTH_CORE_DEFAULT_MODEL_ZDR=true to restore the fail-closed posture.
      // Still not copied from caller input, so a client can never downgrade it.
      zdr: process.env.AUTH_CORE_DEFAULT_MODEL_ZDR === 'true',
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
    const now = Math.floor(Date.now() / 1000);
    const payload = {
      iss: this.planeIssuer,
      aud: config.audience,
      sub: claims.userId,
      iat: now,
      nbf: now - 5,
      exp: now + config.ttlSeconds,
      org_id: claims.orgId,
      ...(principalType === 'user' ? { user_id: claims.userId } : {}),
      principal_type: principalType,
      ...(principalType === 'service' ? { service_id: claims.userId } : {}),
      ...(claims.reason ? { reason: claims.reason } : {}),
      ...(claims.email ? { email: claims.email } : {}),
      ...(claims.scopes && claims.scopes.length > 0
        ? { scopes: claims.scopes }
        : {}),
      // Control Plane does not yet have an authoritative per-org retention
      // policy. The secure-MVP posture is therefore issuer-selected ZDR for
      // every delegated audience. Caller input is deliberately ignored so a
      // request cannot weaken the policy between services.
      zdr: true,
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
    const expiresAt = new Date((now + config.ttlSeconds) * 1000).toISOString();

    return {
      token: `${signingInput}.${signature}`,
      expiresAt,
      expiresInSeconds: config.ttlSeconds,
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
      return {
        privateKey: createPrivateKey(privatePem),
        publicKey: createPublicKey(publicPem),
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
   * var is unset/empty or the file cannot be read, so the caller falls through
   * to the inline-PEM / ephemeral branches without throwing on a misconfigured
   * mount.
   */
  private readKeyFile(path: string | undefined): string | undefined {
    const trimmed = (path ?? '').trim();
    if (!trimmed) {
      return undefined;
    }
    try {
      const pem = readFileSync(trimmed, 'utf8').trim();
      return pem.length > 0 ? pem : undefined;
    } catch (error) {
      this.logger.warn(
        `Failed to read key file at ${trimmed}: ${error instanceof Error ? error.message : String(error)}`,
      );
      return undefined;
    }
  }

  private encodeSegment(value: unknown) {
    return Buffer.from(JSON.stringify(value)).toString('base64url');
  }
}
