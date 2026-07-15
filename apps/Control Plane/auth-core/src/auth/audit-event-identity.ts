import { createHash } from 'node:crypto';

type IssuedTokenAuditPrefix = 'plane-token' | 'model-token';

interface JwtIssuedAtClaims {
  iat?: unknown;
}

/**
 * Derive audit identity from the immutable token artifact Auth Core just
 * minted. Retries publishing the same artifact therefore reuse both the
 * JetStream de-duplication key and the producer occurrence time.
 */
export function issuedTokenAuditIdentity(
  token: string,
  prefix: IssuedTokenAuditPrefix,
): { eventId: string; occurredAt: string } {
  const segments = token.split('.');
  if (segments.length !== 3 || !segments[1]) {
    throw new Error('Issued token has no stable audit identity');
  }

  let claims: JwtIssuedAtClaims;
  try {
    claims = JSON.parse(
      Buffer.from(segments[1], 'base64url').toString('utf8'),
    ) as JwtIssuedAtClaims;
  } catch {
    throw new Error('Issued token has no stable audit identity');
  }
  if (!Number.isSafeInteger(claims.iat) || Number(claims.iat) <= 0) {
    throw new Error('Issued token has no producer occurrence time');
  }

  const occurredAt = new Date(Number(claims.iat) * 1000).toISOString();
  const digest = createHash('sha256').update(token).digest('hex');
  return { eventId: `${prefix}:${digest}`, occurredAt };
}
