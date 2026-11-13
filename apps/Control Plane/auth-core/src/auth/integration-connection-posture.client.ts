import { Injectable, Logger } from '@nestjs/common';
import { ConvexTokenService } from './convex-token.service';
import type { InteractiveRetentionPosture } from './interactive-retention-policy';

/**
 * E2 (docs/CROSS_PLANE_CONTRACT_MATRIX.md, Recorded Exceptions): Auth Core reads
 * ONE model connection's ZDR posture from integration-corev2 as the SESSION
 * USER. The call carries a user-bound `ingestion`-audience plane token minted
 * from the session's user and org, never the internal API key, so the owner
 * check ("only the member who connected this subscription") stays in the
 * service that owns the record. Auth Core never opens integration-corev2's
 * database and never decides ownership itself.
 *
 * What this client does not do, on purpose:
 *  - It sends no `X-Internal-API-Key` and has no internal-key path.
 *  - It does not retry: one attempt, a 2 second budget.
 *  - It does not cache: every decision reads fresh, so a revocation is seen at
 *    once.
 *  - It stores nothing. The answer carries ids, closed codes and a digest only;
 *    the digest is compared by the caller as an opaque string.
 *
 * Anything that is not a well-typed 200 or a recognised owner refusal is
 * `unavailable`. An untyped answer must never read as `acknowledged`.
 */

const DEFAULT_INTEGRATION_CORE_URL = 'http://integration-api:3026';

/** Total budget for the one read. */
export const CONNECTION_POSTURE_TIMEOUT_MS = 2_000;

/** What the connection's provider can honour for zero data retention. */
export type ConnectionZdrCapability =
  | 'supported'
  | 'not_supported'
  | 'cannot_verify';

/** Where the owner's acknowledgement of the current assessment stands. */
export type ConnectionAckState =
  | 'unassessed'
  | 'not_required'
  | 'required'
  | 'acknowledged';

const CAPABILITIES: ReadonlySet<string> = new Set([
  'supported',
  'not_supported',
  'cannot_verify',
]);

const ACK_STATES: ReadonlySet<string> = new Set([
  'unassessed',
  'not_required',
  'required',
  'acknowledged',
]);

/** integration-corev2 error codes that mean "this caller does not own it". */
const NOT_OWNER_FORBIDDEN_CODES: ReadonlySet<string> = new Set([
  'not_connection_owner',
  'forbidden',
]);
const NOT_OWNER_NOT_FOUND_CODES: ReadonlySet<string> = new Set([
  'connection_not_found',
  'not_a_model_connection',
]);

/** The posture integration-corev2 holds for one connection. */
export type ConnectionPosture = Readonly<{
  kind: 'posture';
  providerKey: string;
  capability: ConnectionZdrCapability;
  /** True when the capability contract offers a waiver route for this connection. */
  waiverRoute: boolean;
  ackState: ConnectionAckState;
  /** Closed reason code the acknowledgement is not in force; empty when it is. */
  ackReason: string;
  /** Id of the acknowledgement in force; empty when there is none. */
  ackId: string;
  /** Opaque assessment digest; compared as a string, never interpreted. */
  digest: string;
  textVersion: string;
}>;

/**
 * Result of one posture read. `not_owner` covers every refusal that says the
 * session user may not act on this connection (not the owner, other org, gone,
 * or not a model connection). `unavailable` is everything else.
 */
export type ConnectionPostureResult =
  | ConnectionPosture
  | Readonly<{ kind: 'not_owner' }>
  | Readonly<{ kind: 'unavailable' }>;

/** Input of one posture read. */
export type ReadConnectionPostureInput = Readonly<{
  /** The session user. The owner check is made against this id. */
  userId: string;
  /** The session user's active organization. */
  orgId: string;
  connectionId: string;
  /** The org posture the caller already resolved; stamped on the token. */
  retentionPosture: InteractiveRetentionPosture;
}>;

const UNAVAILABLE: ConnectionPostureResult = Object.freeze({
  kind: 'unavailable',
});
const NOT_OWNER: ConnectionPostureResult = Object.freeze({
  kind: 'not_owner',
});

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Type-checks the 200 body (`{ data: {...} }`). Every field must be present and
 * of the right type; `capability` and `ackState` must be members of their
 * closed sets. Returns null otherwise.
 */
function parsePosture(
  body: unknown,
  connectionId: string,
): ConnectionPosture | null {
  if (!isRecord(body) || !isRecord(body.data)) return null;
  const data = body.data;
  const {
    providerKey,
    capability,
    waiverRoute,
    ackState,
    ackReason,
    ackId,
    digest,
    textVersion,
  } = data;
  // An answer about some other connection must never be read as this one's.
  if (data.connectionId !== undefined && data.connectionId !== connectionId) {
    return null;
  }
  if (
    typeof providerKey !== 'string' ||
    typeof capability !== 'string' ||
    !CAPABILITIES.has(capability) ||
    typeof waiverRoute !== 'boolean' ||
    typeof ackState !== 'string' ||
    !ACK_STATES.has(ackState) ||
    typeof ackReason !== 'string' ||
    typeof ackId !== 'string' ||
    typeof digest !== 'string' ||
    typeof textVersion !== 'string'
  ) {
    return null;
  }
  return {
    kind: 'posture',
    providerKey,
    capability: capability as ConnectionZdrCapability,
    waiverRoute,
    ackState: ackState as ConnectionAckState,
    ackReason,
    ackId,
    digest,
    textVersion,
  };
}

/** The `error.code` of an integration-corev2 error envelope, or null. */
function errorCodeOf(body: unknown): string | null {
  if (!isRecord(body) || !isRecord(body.error)) return null;
  return typeof body.error.code === 'string' ? body.error.code : null;
}

@Injectable()
export class IntegrationConnectionPostureClient {
  private readonly logger = new Logger(IntegrationConnectionPostureClient.name);
  private readonly baseUrl: string;

  constructor(private readonly tokens: ConvexTokenService) {
    // Read once; a later change to the environment is not seen.
    this.baseUrl = (
      process.env.INTEGRATION_CORE_URL?.trim() || DEFAULT_INTEGRATION_CORE_URL
    ).replace(/\/+$/, '');
  }

  /**
   * Reads the posture of one connection as the session user. Never throws:
   * every failure is `{ kind: 'unavailable' }`, so a caller that fails closed
   * on anything but an `acknowledged` posture needs no try/catch.
   */
  async readConnectionPosture(
    input: ReadConnectionPostureInput,
  ): Promise<ConnectionPostureResult> {
    const { userId, orgId, connectionId, retentionPosture } = input;
    if (!userId || !orgId || !connectionId) return UNAVAILABLE;

    let bearer: string;
    try {
      // E2: a user-bound, ingestion-audience token for the session user and org.
      bearer = this.tokens.issuePlaneToken('ingestion', {
        userId,
        orgId,
        principalType: 'user',
        retentionPosture,
      }).token;
    } catch {
      this.logger.warn(
        `posture read could not mint a user token for connection ${connectionId}`,
      );
      return UNAVAILABLE;
    }

    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    // A fetch that ignores its signal must still end at the budget.
    const timedOut = new Promise<never>((_resolve, reject) => {
      timer = setTimeout(() => {
        controller.abort();
        reject(new Error('timeout'));
      }, CONNECTION_POSTURE_TIMEOUT_MS);
    });
    try {
      const url = `${this.baseUrl}/api/v1/model-connections/${encodeURIComponent(connectionId)}/zdr-posture`;
      const response = await Promise.race([
        fetch(url, {
          method: 'GET',
          headers: {
            Accept: 'application/json',
            Authorization: `Bearer ${bearer}`,
          },
          signal: controller.signal,
        }),
        timedOut,
      ]);
      return await this.mapResponse(response, connectionId, timedOut);
    } catch {
      this.logger.warn(
        `posture read failed (timeout or transport) for connection ${connectionId}`,
      );
      return UNAVAILABLE;
    } finally {
      if (timer !== undefined) clearTimeout(timer);
      // The race loser must not surface as an unhandled rejection.
      timedOut.catch(() => undefined);
    }
  }

  private async mapResponse(
    response: Response,
    connectionId: string,
    timedOut: Promise<never>,
  ): Promise<ConnectionPostureResult> {
    const status = response.status;
    if (status !== 200 && status !== 403 && status !== 404) {
      this.logger.warn(
        `posture read refused: HTTP ${status} for connection ${connectionId}`,
      );
      return UNAVAILABLE;
    }
    let body: unknown;
    try {
      body = await Promise.race([response.json(), timedOut]);
    } catch {
      this.logger.warn(
        `posture read returned an unreadable body: HTTP ${status} for connection ${connectionId}`,
      );
      return UNAVAILABLE;
    }
    if (status === 200) {
      const posture = parsePosture(body, connectionId);
      if (!posture) {
        this.logger.warn(
          `posture read returned an untyped answer: HTTP ${status} for connection ${connectionId}`,
        );
        return UNAVAILABLE;
      }
      return posture;
    }
    const code = errorCodeOf(body);
    if (
      code !== null &&
      (status === 403
        ? NOT_OWNER_FORBIDDEN_CODES.has(code)
        : NOT_OWNER_NOT_FOUND_CODES.has(code))
    ) {
      return NOT_OWNER;
    }
    // A bare 404 means the routes are switched off; any other body is unknown.
    this.logger.warn(
      `posture read refused: HTTP ${status} for connection ${connectionId}`,
    );
    return UNAVAILABLE;
  }
}
