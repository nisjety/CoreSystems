import { Logger, type Provider } from '@nestjs/common';
import {
  createPrivateKey,
  createPublicKey,
  createSign,
  createVerify,
  randomUUID,
  type KeyObject,
} from 'crypto';
import { readFileSync } from 'fs';
import { ConvexTokenService } from './convex-token.service';
import {
  createVerifiedRetentionDecision,
  RetentionDecisionInvalidError,
  VerifiedRetentionDecision,
} from './retention-decision.verified';

// The verified type and the invalid-decision error live in the dependency-free
// retention-decision.verified module, so the token issuer can check them at
// runtime. They are re-exported here for existing importers.
export { RetentionDecisionInvalidError, VerifiedRetentionDecision };

/**
 * Dedicated retention-decision signer (ZDR.md section 5.3, decision D-ZDR-01).
 *
 * Auth Core is the only issuer of a signed "waived" retention decision: a
 * 60 second RS256 JWS that a plane can check before it persists content for a
 * connection whose owner waived Zero Data Retention. The signing key is a
 * dedicated RSA keypair loaded from mounted files by key id. It is never the
 * token signing key and is never published in the JWKS, so a leaked token key
 * cannot mint a waiver. When the key is absent or unsafe the factory returns
 * the unavailable signer and every waiver fails closed.
 *
 * This module only signs and verifies what the caller (the decision service)
 * decides. It stores nothing.
 */

/** Nest provider token for the signer chosen once at boot. */
export const RETENTION_DECISION_SIGNER = Symbol('RETENTION_DECISION_SIGNER');

/** Issuer claim of every retention decision. */
export const RETENTION_DECISION_ISSUER = 'auth-core';
/** Audience claim of every retention decision. */
export const RETENTION_DECISION_AUDIENCE = 'auth-core:retention-decision';
/** JWS `typ` header of every retention decision. */
export const RETENTION_DECISION_TYP = 'retention-decision+jwt';
/** Waiver contract version carried in the `wv` claim. */
export const WAIVER_CONTRACT_VERSION = 1;
/** Lifetime of a signed decision in seconds. */
export const RETENTION_DECISION_TTL_SECONDS = 60;

const CLOCK_SKEW_SECONDS = 5;
const MAX_FIELD_LENGTH = 128;
const MIN_RSA_MODULUS_BITS = 2048;

/** Facts a signed waiver decision attests to. All fields are required. */
export type RetentionDecisionClaims = {
  /** Better Auth user id (becomes `sub`). */
  userId: string;
  /** Organization the connection belongs to (becomes `org_id`). */
  orgId: string;
  /** Better Auth session id (becomes `sid`). */
  sessionId: string;
  /** Connection the waiver applies to. */
  connection: string;
  /** Provider the connection belongs to. */
  provider: string;
  /** Id of the owner's acknowledgement of the waiver. */
  ackId: string;
  /** Digest binding the decision to the acknowledged text. */
  digest: string;
};

/** Result of signing a decision. */
export type SignedRetentionDecision = {
  /** Compact JWS. Never log it. */
  token: string;
  /** ISO timestamp at which the token stops verifying. */
  expiresAt: string;
  /** Lifetime in seconds. */
  expiresInSeconds: number;
  /** Unique token id. */
  jti: string;
  /** Key id that signed the token. */
  keyId: string;
};

/** Raised when no decision signing key is available (fail closed). */
export class RetentionDecisionUnavailableError extends Error {
  constructor() {
    super('Retention decision signer is unavailable');
    this.name = 'RetentionDecisionUnavailableError';
  }
}

/** The one interface callers depend on; the key can move to an HSM later. */
export interface RetentionDecisionSigner {
  /** True when a decision signing key is loaded. */
  available(): boolean;
  /** Sign a waived decision. Throws RetentionDecisionUnavailableError when unavailable. */
  sign(claims: RetentionDecisionClaims): SignedRetentionDecision;
  /** Verify a token. Throws RetentionDecisionUnavailableError or RetentionDecisionInvalidError. */
  verify(token: string): VerifiedRetentionDecision;
}

// eslint-disable-next-line no-control-regex
const CONTROL_CHARACTERS = /[\u0000-\u001f\u007f-\u009f]/;

function isCleanField(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= MAX_FIELD_LENGTH &&
    value.trim() === value &&
    !CONTROL_CHARACTERS.test(value)
  );
}

function isEpochSeconds(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value > 0;
}

function encodeSegment(value: unknown): string {
  return Buffer.from(JSON.stringify(value)).toString('base64url');
}

function decodeObject(segment: string): Record<string, unknown> {
  const decoded: unknown = JSON.parse(
    Buffer.from(segment, 'base64url').toString('utf8'),
  );
  if (!decoded || typeof decoded !== 'object' || Array.isArray(decoded)) {
    throw new RetentionDecisionInvalidError();
  }
  return decoded as Record<string, unknown>;
}

function spkiOf(key: KeyObject): Buffer {
  return key.export({ type: 'spki', format: 'der' }) as Buffer;
}

function isStrongRsa(key: KeyObject): boolean {
  const bits = key.asymmetricKeyDetails?.modulusLength;
  return (
    key.asymmetricKeyType === 'rsa' &&
    typeof bits === 'number' &&
    bits >= MIN_RSA_MODULUS_BITS
  );
}

/** Signer used whenever no safe decision key is configured: fails closed. */
export class UnavailableRetentionDecisionSigner
  implements RetentionDecisionSigner
{
  available(): boolean {
    return false;
  }

  sign(_claims: RetentionDecisionClaims): SignedRetentionDecision {
    throw new RetentionDecisionUnavailableError();
  }

  verify(_token: string): VerifiedRetentionDecision {
    throw new RetentionDecisionUnavailableError();
  }
}

type FileKeySignerKeys = {
  keyId: string;
  privateKey: KeyObject;
  publicKey: KeyObject;
  /** Verify-only keys by key id (rotation overlap). */
  previous: ReadonlyMap<string, KeyObject>;
};

/** RS256 signer over a dedicated keypair loaded from mounted key files. */
export class FileKeyRetentionDecisionSigner implements RetentionDecisionSigner {
  private readonly verifyKeys: ReadonlyMap<string, KeyObject>;

  constructor(private readonly keys: FileKeySignerKeys) {
    const verifyKeys = new Map<string, KeyObject>(keys.previous);
    verifyKeys.set(keys.keyId, keys.publicKey);
    this.verifyKeys = verifyKeys;
  }

  available(): boolean {
    return true;
  }

  sign(claims: RetentionDecisionClaims): SignedRetentionDecision {
    const fields = [
      claims?.userId,
      claims?.orgId,
      claims?.sessionId,
      claims?.connection,
      claims?.provider,
      claims?.ackId,
      claims?.digest,
    ];
    if (!fields.every(isCleanField)) {
      throw new RetentionDecisionInvalidError(
        'Retention decision claims are not valid',
      );
    }
    const now = Math.floor(Date.now() / 1000);
    const jti = randomUUID();
    const payload = {
      iss: RETENTION_DECISION_ISSUER,
      aud: RETENTION_DECISION_AUDIENCE,
      sub: claims.userId,
      org_id: claims.orgId,
      sid: claims.sessionId,
      connection: claims.connection,
      provider: claims.provider,
      ack_id: claims.ackId,
      digest: claims.digest,
      wv: WAIVER_CONTRACT_VERSION,
      decision: 'waived',
      jti,
      iat: now,
      nbf: now - CLOCK_SKEW_SECONDS,
      exp: now + RETENTION_DECISION_TTL_SECONDS,
    };
    const signingInput = `${encodeSegment({
      alg: 'RS256',
      typ: RETENTION_DECISION_TYP,
      kid: this.keys.keyId,
    })}.${encodeSegment(payload)}`;
    const signer = createSign('RSA-SHA256');
    signer.update(signingInput);
    signer.end();
    const signature = signer.sign(this.keys.privateKey).toString('base64url');
    return {
      token: `${signingInput}.${signature}`,
      expiresAt: new Date(payload.exp * 1000).toISOString(),
      expiresInSeconds: RETENTION_DECISION_TTL_SECONDS,
      jti,
      keyId: this.keys.keyId,
    };
  }

  verify(token: string): VerifiedRetentionDecision {
    try {
      return this.verifyOrThrow(token);
    } catch (error) {
      if (error instanceof RetentionDecisionInvalidError) {
        throw error;
      }
      // Malformed JSON, bad base64 or a crypto error: never leak the cause.
      throw new RetentionDecisionInvalidError();
    }
  }

  private verifyOrThrow(token: string): VerifiedRetentionDecision {
    if (typeof token !== 'string') {
      throw new RetentionDecisionInvalidError();
    }
    const segments = token.split('.');
    if (
      segments.length !== 3 ||
      segments.some((segment) => !segment || !/^[A-Za-z0-9_-]+$/.test(segment))
    ) {
      throw new RetentionDecisionInvalidError();
    }
    const [encodedHeader, encodedPayload, encodedSignature] = segments;
    const header = decodeObject(encodedHeader);
    const payload = decodeObject(encodedPayload);

    const kid = header.kid;
    const headerKeys = Object.keys(header);
    if (
      header.alg !== 'RS256' ||
      header.typ !== RETENTION_DECISION_TYP ||
      !headerKeys.every(
        (key) => key === 'alg' || key === 'typ' || key === 'kid',
      ) ||
      typeof kid !== 'string' ||
      !this.verifyKeys.has(kid)
    ) {
      throw new RetentionDecisionInvalidError();
    }
    const verifier = createVerify('RSA-SHA256');
    verifier.update(`${encodedHeader}.${encodedPayload}`);
    verifier.end();
    if (
      !verifier.verify(
        this.verifyKeys.get(kid) as KeyObject,
        Buffer.from(encodedSignature, 'base64url'),
      )
    ) {
      throw new RetentionDecisionInvalidError();
    }

    const now = Math.floor(Date.now() / 1000);
    const { iat, nbf, exp } = payload;
    if (
      payload.iss !== RETENTION_DECISION_ISSUER ||
      payload.aud !== RETENTION_DECISION_AUDIENCE ||
      payload.decision !== 'waived' ||
      payload.wv !== WAIVER_CONTRACT_VERSION ||
      !isEpochSeconds(iat) ||
      !isEpochSeconds(nbf) ||
      !isEpochSeconds(exp) ||
      exp <= now ||
      nbf > now + CLOCK_SKEW_SECONDS ||
      iat > now + CLOCK_SKEW_SECONDS ||
      exp <= iat ||
      exp - iat > RETENTION_DECISION_TTL_SECONDS
    ) {
      throw new RetentionDecisionInvalidError();
    }
    const { sub, org_id, sid, connection, provider, ack_id, digest, jti } =
      payload;
    if (
      !isCleanField(sub) ||
      !isCleanField(org_id) ||
      !isCleanField(sid) ||
      !isCleanField(connection) ||
      !isCleanField(provider) ||
      !isCleanField(ack_id) ||
      !isCleanField(digest) ||
      !isCleanField(jti)
    ) {
      throw new RetentionDecisionInvalidError();
    }
    return createVerifiedRetentionDecision({
      userId: sub,
      orgId: org_id,
      sessionId: sid,
      connection,
      provider,
      ackId: ack_id,
      digest,
      jti,
      keyId: kid,
      issuedAt: iat,
      expiresAt: exp,
    });
  }
}

/** Environment variable names read by the factory. */
export type RetentionDecisionSignerEnv = Partial<
  Record<
    | 'ZDR_DECISION_KEY_ID'
    | 'ZDR_DECISION_PRIVATE_KEY_FILE'
    | 'ZDR_DECISION_PUBLIC_KEY_FILE'
    | 'ZDR_DECISION_PREVIOUS_KEY_ID'
    | 'ZDR_DECISION_PREVIOUS_PUBLIC_KEY_FILE',
    string | undefined
  >
>;

/** Names the variable that made the signer unavailable. Never carries key data. */
class DecisionKeyConfigError extends Error {
  constructor(
    readonly variable: string,
    reason: string,
  ) {
    super(`${variable}: ${reason}`);
  }
}

function readDecisionKeyFile(
  variable: string,
  path: string | undefined,
): string {
  const trimmed = (path ?? '').trim();
  if (!trimmed) {
    throw new DecisionKeyConfigError(variable, 'is not set');
  }
  let pem: string;
  try {
    pem = readFileSync(trimmed, 'utf8').trim();
  } catch {
    throw new DecisionKeyConfigError(variable, 'file is not readable');
  }
  if (!pem) {
    throw new DecisionKeyConfigError(variable, 'file is empty');
  }
  return pem;
}

function readKeyId(variable: string, value: string | undefined): string {
  const trimmed = (value ?? '').trim();
  if (!trimmed) {
    throw new DecisionKeyConfigError(variable, 'is not set');
  }
  if (!isCleanField(trimmed)) {
    throw new DecisionKeyConfigError(variable, 'is not a valid key id');
  }
  return trimmed;
}

function loadPublicKey(variable: string, pem: string): KeyObject {
  let key: KeyObject;
  try {
    key = createPublicKey(pem);
  } catch {
    throw new DecisionKeyConfigError(variable, 'is not a parsable public key');
  }
  if (!isStrongRsa(key)) {
    throw new DecisionKeyConfigError(
      variable,
      'must be an RSA key of at least 2048 bits',
    );
  }
  return key;
}

function loadPrivateKey(variable: string, pem: string): KeyObject {
  let key: KeyObject;
  try {
    key = createPrivateKey(pem);
  } catch {
    throw new DecisionKeyConfigError(variable, 'is not a parsable private key');
  }
  if (!isStrongRsa(key)) {
    throw new DecisionKeyConfigError(
      variable,
      'must be an RSA key of at least 2048 bits',
    );
  }
  return key;
}

/**
 * Choose the signer once at boot. Returns the unavailable signer, with an error
 * log that names the variable and never the key, when any file variable is
 * unset or unreadable, when the key is weaker than RSA 2048, when the pair does
 * not match, or when the public key equals the token signing key. There is no
 * ephemeral key in any environment.
 *
 * @param tokenSigningPublicKeySpki SPKI DER of the token signing public key
 * @param env environment to read (defaults to process.env)
 * @param logger sink for the error log
 */
export function createRetentionDecisionSigner(
  tokenSigningPublicKeySpki: Buffer,
  env: RetentionDecisionSignerEnv = process.env as RetentionDecisionSignerEnv,
  logger: Pick<Logger, 'error' | 'log'> = new Logger('RetentionDecisionSigner'),
): RetentionDecisionSigner {
  try {
    const keyId = readKeyId('ZDR_DECISION_KEY_ID', env.ZDR_DECISION_KEY_ID);
    const privatePem = readDecisionKeyFile(
      'ZDR_DECISION_PRIVATE_KEY_FILE',
      env.ZDR_DECISION_PRIVATE_KEY_FILE,
    );
    const publicPem = readDecisionKeyFile(
      'ZDR_DECISION_PUBLIC_KEY_FILE',
      env.ZDR_DECISION_PUBLIC_KEY_FILE,
    );
    const privateKey = loadPrivateKey(
      'ZDR_DECISION_PRIVATE_KEY_FILE',
      privatePem,
    );
    const publicKey = loadPublicKey('ZDR_DECISION_PUBLIC_KEY_FILE', publicPem);
    const publicSpki = spkiOf(publicKey);
    if (!spkiOf(createPublicKey(privateKey)).equals(publicSpki)) {
      throw new DecisionKeyConfigError(
        'ZDR_DECISION_PUBLIC_KEY_FILE',
        'does not match ZDR_DECISION_PRIVATE_KEY_FILE',
      );
    }
    if (publicSpki.equals(tokenSigningPublicKeySpki)) {
      throw new DecisionKeyConfigError(
        'ZDR_DECISION_PUBLIC_KEY_FILE',
        'must differ from the token signing key',
      );
    }

    const previous = new Map<string, KeyObject>();
    const previousKeyIdRaw = (env.ZDR_DECISION_PREVIOUS_KEY_ID ?? '').trim();
    const previousFileRaw = (
      env.ZDR_DECISION_PREVIOUS_PUBLIC_KEY_FILE ?? ''
    ).trim();
    if (previousKeyIdRaw || previousFileRaw) {
      try {
        const previousKeyId = readKeyId(
          'ZDR_DECISION_PREVIOUS_KEY_ID',
          env.ZDR_DECISION_PREVIOUS_KEY_ID,
        );
        const previousKey = loadPublicKey(
          'ZDR_DECISION_PREVIOUS_PUBLIC_KEY_FILE',
          readDecisionKeyFile(
            'ZDR_DECISION_PREVIOUS_PUBLIC_KEY_FILE',
            env.ZDR_DECISION_PREVIOUS_PUBLIC_KEY_FILE,
          ),
        );
        if (previousKeyId === keyId) {
          throw new DecisionKeyConfigError(
            'ZDR_DECISION_PREVIOUS_KEY_ID',
            'must differ from ZDR_DECISION_KEY_ID',
          );
        }
        if (spkiOf(previousKey).equals(tokenSigningPublicKeySpki)) {
          throw new DecisionKeyConfigError(
            'ZDR_DECISION_PREVIOUS_PUBLIC_KEY_FILE',
            'must differ from the token signing key',
          );
        }
        previous.set(previousKeyId, previousKey);
      } catch (error) {
        // A bad previous key only removes the rotation overlap; the current
        // key stays usable and tokens from the previous key fail verification.
        if (!(error instanceof DecisionKeyConfigError)) {
          throw error;
        }
        logger.error(
          `Retention decision previous key ignored: ${error.message}`,
        );
      }
    }

    logger.log('Retention decision signer is available');
    return new FileKeyRetentionDecisionSigner({
      keyId,
      privateKey,
      publicKey,
      previous,
    });
  } catch (error) {
    const detail =
      error instanceof DecisionKeyConfigError
        ? error.message
        : 'decision key configuration could not be loaded';
    logger.error(
      `Retention decision signer is unavailable, waivers fail closed: ${detail}`,
    );
    return new UnavailableRetentionDecisionSigner();
  }
}

/**
 * Nest provider that picks the signer once at boot. Registered in the module
 * by the decision unit (ZDR-Z-14), not here.
 */
export const retentionDecisionSignerProvider: Provider = {
  provide: RETENTION_DECISION_SIGNER,
  inject: [ConvexTokenService],
  useFactory: (convexTokens: ConvexTokenService): RetentionDecisionSigner =>
    createRetentionDecisionSigner(convexTokens.tokenSigningPublicKeySpki()),
};
