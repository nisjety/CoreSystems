import { Injectable, Logger } from '@nestjs/common';
import {
  createPrivateKey,
  createPublicKey,
  createSign,
  generateKeyPairSync,
  type KeyObject,
} from 'crypto';

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
 *   PLANE_TOKEN_TTL_APPLICATION_PLANE_SECONDS  (default 300s)
 *
 * Audience names match the path slug (and the velion `PlaneAudience`
 * union) for symmetry with the per-audience endpoints.
 */
export type PlaneAudience =
  | 'data-plane'
  | 'quarry'
  | 'ingestion'
  | 'control-plane'
  | 'application-plane';

type PlaneJwtClaims = {
  userId: string;
  orgId: string;
  email?: string;
  scopes?: readonly string[];
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
  private readonly modelPlaneAudience =
    process.env.MODEL_PLANE_AUTH_AUDIENCE || 'model-gateway';
  private readonly modelPlaneTtlSeconds = Math.max(
    60,
    parseInt(
      process.env.MODEL_PLANE_AUTH_TOKEN_TTL_SECONDS || '900',
      10,
    ) || 900,
  );

  // Phase A · A1.1 — non-Model-Plane audience config. Default TTL of 5 min
  // matches the Convex token; planes that need longer can override per env.
  private readonly planeIssuer =
    process.env.PLANE_TOKEN_ISSUER ||
    process.env.MODEL_PLANE_AUTH_ISSUER ||
    process.env.CONVEX_AUTH_ISSUER ||
    `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/convex-auth`;

  private readonly planeAudiences: Record<PlaneAudience, PlaneAudienceConfig> = {
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
    'application-plane': {
      audience:
        process.env.APPLICATION_PLANE_AUTH_AUDIENCE || 'application-plane',
      ttlSeconds: parsePositiveTtl(
        'PLANE_TOKEN_TTL_APPLICATION_PLANE_SECONDS',
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
      user_id: claims.userId,
      // Optional extras — accepted by the gateway but not required.
      ...(claims.email ? { email: claims.email } : {}),
      ...(claims.scopes && claims.scopes.length > 0
        ? { scopes: claims.scopes }
        : {}),
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
    const now = Math.floor(Date.now() / 1000);
    const payload = {
      iss: this.planeIssuer,
      aud: config.audience,
      sub: claims.userId,
      iat: now,
      nbf: now - 5,
      exp: now + config.ttlSeconds,
      org_id: claims.orgId,
      user_id: claims.userId,
      ...(claims.email ? { email: claims.email } : {}),
      ...(claims.scopes && claims.scopes.length > 0
        ? { scopes: claims.scopes }
        : {}),
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
      (now + config.ttlSeconds) * 1000,
    ).toISOString();

    return {
      token: `${signingInput}.${signature}`,
      expiresAt,
      expiresInSeconds: config.ttlSeconds,
      issuer: this.planeIssuer,
      audience: config.audience,
    };
  }

  /**
   * Phase A · A1.1 — guard exposed to controllers so they can validate
   * the audience slug from the request path against the configured set
   * before touching the keypair. Returns `null` if the audience is
   * unknown, allowing the controller to surface a clean 404/400.
   */
  isKnownPlaneAudience(value: string): value is PlaneAudience {
    return Object.prototype.hasOwnProperty.call(this.planeAudiences, value);
  }

  getJwks() {
    return {
      keys: [this.publicJwk],
    };
  }

  private loadKeyPair() {
    const privatePem = process.env.CONVEX_AUTH_PRIVATE_KEY_PEM;
    const publicPem = process.env.CONVEX_AUTH_PUBLIC_KEY_PEM;

    if (privatePem && publicPem) {
      return {
        privateKey: createPrivateKey(privatePem),
        publicKey: createPublicKey(publicPem),
      };
    }

    const message =
      'CONVEX_AUTH_PRIVATE_KEY_PEM / CONVEX_AUTH_PUBLIC_KEY_PEM not set; generating ephemeral RSA keypair for local development.';
    if (process.env.NODE_ENV === 'production') {
      this.logger.warn(message);
    } else {
      this.logger.log(message);
    }

    const generated = generateKeyPairSync('rsa', {
      modulusLength: 2048,
    });

    return {
      privateKey: generated.privateKey,
      publicKey: generated.publicKey,
    };
  }

  private encodeSegment(value: unknown) {
    return Buffer.from(JSON.stringify(value)).toString('base64url');
  }
}
