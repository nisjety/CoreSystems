/**
 * The verified retention decision (ZDR-Z-14s, ZDR-Z-14b).
 *
 * This module has no imports on purpose. The signer builds a
 * `VerifiedRetentionDecision` here, and the token issuer
 * (convex-token.service.ts) accepts one only when `isVerifiedRetentionDecision`
 * recognises it as the factory's own. The signer imports ConvexTokenService for
 * its Nest provider, so the type cannot live in the signer without a load-order
 * cycle. Keep this file dependency-free.
 */

/** Raised for a malformed claim set or a token that does not verify. */
export class RetentionDecisionInvalidError extends Error {
  constructor(message = 'Retention decision is not valid') {
    super(message);
    this.name = 'RetentionDecisionInvalidError';
  }
}

/** The facts the signer verified, as the constructor receives them. */
export type VerifiedFields = {
  userId: string;
  orgId: string;
  sessionId: string;
  connection: string;
  provider: string;
  ackId: string;
  digest: string;
  jti: string;
  keyId: string;
  issuedAt: number;
  expiresAt: number;
};

// Not exported: only this module can build a VerifiedRetentionDecision.
const VERIFIED_GUARD = Symbol('VerifiedRetentionDecisionGuard');

// The brand. The factory records every decision it builds here; nothing else
// can add to the set. `instanceof` is not a brand: `Object.create` of the class
// prototype passes it.
const issued = new WeakSet<object>();

/**
 * A retention decision that the signer verified. The constructor is private
 * and guarded by a module-private symbol, so no other code can construct one
 * from request data. Callers obtain it from `RetentionDecisionSigner.verify`.
 * The issuer accepts only a decision that `isVerifiedRetentionDecision` brands,
 * so a structurally equal object (a spread copy, a prototype-linked object, a
 * request body) is refused.
 */
export class VerifiedRetentionDecision {
  readonly decision = 'waived' as const;
  readonly userId: string;
  readonly orgId: string;
  readonly sessionId: string;
  readonly connection: string;
  readonly provider: string;
  readonly ackId: string;
  readonly digest: string;
  readonly jti: string;
  readonly keyId: string;
  /** Issued-at, seconds since the epoch. */
  readonly issuedAt: number;
  /** Expiry, seconds since the epoch. */
  readonly expiresAt: number;

  private constructor(guard: symbol, fields: VerifiedFields) {
    if (guard !== VERIFIED_GUARD) {
      throw new RetentionDecisionInvalidError(
        'A verified retention decision can only be created by the signer',
      );
    }
    this.userId = fields.userId;
    this.orgId = fields.orgId;
    this.sessionId = fields.sessionId;
    this.connection = fields.connection;
    this.provider = fields.provider;
    this.ackId = fields.ackId;
    this.digest = fields.digest;
    this.jti = fields.jti;
    this.keyId = fields.keyId;
    this.issuedAt = fields.issuedAt;
    this.expiresAt = fields.expiresAt;
    Object.freeze(this);
  }
}

/**
 * Builds a VerifiedRetentionDecision. The signer calls it after verifying a
 * token, and nothing else may (the signer spec pins that). It is exported only
 * so the signer can reach it across this module boundary.
 */
export function createVerifiedRetentionDecision(
  fields: VerifiedFields,
): VerifiedRetentionDecision {
  const construct = VerifiedRetentionDecision as unknown as new (
    guard: symbol,
    fields: VerifiedFields,
  ) => VerifiedRetentionDecision;
  const decision = new construct(VERIFIED_GUARD, fields);
  issued.add(decision);
  return decision;
}

/**
 * True only for a decision the factory built, which is every decision the
 * signer verifies. A copy or a prototype-linked object is false. The issuer uses
 * this check, not `instanceof`.
 */
export function isVerifiedRetentionDecision(
  value: unknown,
): value is VerifiedRetentionDecision {
  return typeof value === 'object' && value !== null && issued.has(value);
}
