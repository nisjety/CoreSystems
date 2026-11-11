import { Inject, Injectable, Logger, type Provider } from '@nestjs/common';

import zdrRefusalsFixture from './fixtures/zdr-refusals.json';
import {
  IntegrationConnectionPostureClient,
  type ConnectionPosture,
  type ConnectionPostureResult,
} from './integration-connection-posture.client';
import {
  resolveInteractiveRetentionPosture,
  type InteractiveRetentionPosture,
} from './interactive-retention-policy';
import {
  RETENTION_DECISION_SIGNER,
  type RetentionDecisionSigner,
  type SignedRetentionDecision,
} from './retention-decision.signer';

/**
 * Per-connection ZDR waiver decision (ZDR.md sections 5.2, 5.3 and 5.7;
 * docs/core-research/auth-core.md, "per-connection ZDR waiver").
 *
 * Auth Core is the only issuer of a signed "waived" retention decision. The BFF
 * asks; it cannot assert. This service evaluates the posture matrix rule
 * (apps/Model Plane/api/contracts/zdr-posture-matrix.json, `rule`) in the
 * order the matrix lists it, with the literal predicates written below, in one
 * place:
 *
 *   1. O, the org posture, comes from `resolveInteractiveRetentionPosture`.
 *      Unknown (org-core unreadable, nothing remembered) refuses
 *      `zdr_decision_unavailable`; nothing else is read.
 *   2. A connection is named and (O or explicit_zdr): read it as the session
 *      user. Unavailable, or an `unassessed` capability record, refuses
 *      `zdr_decision_unavailable` (retryable); another member's or another
 *      org's connection refuses `zdr_connection_not_acknowledged`; a request
 *      provider other than the connection's refuses `zdr_waiver_route_mismatch`.
 *   3. explicit_zdr and the capability is not `supported` (not_supported and
 *      cannot_verify both count as not supported): refuse
 *      `zdr_explicit_request_not_supported_by_connection`, before the kill
 *      switch. A waiver covers the org default only.
 *   4. Not O: `persistent`, or `zdr` (reason explicit_zdr) when explicit.
 *   5. O and no connection: `zdr`.
 *   6. O and a supported connection: `zdr` stays on.
 *   7. O and an unsupported connection: the kill switch off, a connection with
 *      no waiver route, or a provider outside WAIVER_PROVIDERS refuses
 *      `zdr_connection_not_supported`; no acknowledgement in force refuses
 *      `zdr_connection_not_acknowledged`; otherwise `waived`, signed for one
 *      user, one connection and one session for 60 s.
 *
 * The decision binds sub, org, sid, connection, provider, ack_id and digest
 * from the session and the integration-corev2 answer, never from the request
 * body. Space ZDR is not an input in v1 (matrix row
 * d_space_zdr_not_applied_org_off). Every doubt refuses with
 * `zdr_decision_unavailable`. This service stores nothing and logs ids and
 * closed codes only.
 */

/** Nest provider token for the ZDR_WAIVER_ENABLED kill switch value. */
export const ZDR_WAIVER_ENABLED_TOKEN = Symbol('ZDR_WAIVER_ENABLED_TOKEN');

/** Providers that have a waiver route in v1 (matrix `policy.waiver_providers`). */
export const WAIVER_PROVIDERS: readonly string[] = [
  'openai-codex-subscription',
];

/**
 * Nest provider for the kill switch. The strict literal 'true' turns it on;
 * anything else, including an unset variable, is off. Read once at boot, so a
 * later change to the environment is not seen. Tests override this provider.
 */
export const zdrWaiverEnabledProvider: Provider = {
  provide: ZDR_WAIVER_ENABLED_TOKEN,
  useFactory: (): boolean => process.env.ZDR_WAIVER_ENABLED === 'true',
};

/** Refusal codes this endpoint can answer (a subset of the Z-05a catalogue). */
export type RetentionDecisionRefusalCode =
  | 'zdr_connection_not_acknowledged'
  | 'zdr_connection_not_supported'
  | 'zdr_explicit_request_not_supported_by_connection'
  | 'zdr_decision_unavailable'
  | 'zdr_waiver_route_mismatch'
  | 'zdr_posture_mismatch';

const REFUSAL_CODES: readonly RetentionDecisionRefusalCode[] = [
  'zdr_connection_not_acknowledged',
  'zdr_connection_not_supported',
  'zdr_explicit_request_not_supported_by_connection',
  'zdr_decision_unavailable',
  'zdr_waiver_route_mismatch',
  'zdr_posture_mismatch',
];

/** What the client is told about one refusal, read from the vendored catalogue. */
export type RetentionDecisionRefusal = Readonly<{
  code: RetentionDecisionRefusalCode;
  message: string;
  retryable: boolean;
  action: string;
}>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Builds the refusal table from the vendored catalogue
 * (`fixtures/zdr-refusals.json`, a byte copy of
 * apps/Model Plane/api/contracts/zdr-refusals.json). A code missing from the
 * catalogue, or an entry of the wrong shape, throws: the service must not boot
 * with a refusal it cannot describe.
 */
export function loadRetentionDecisionRefusals(
  catalogue: unknown,
): ReadonlyMap<RetentionDecisionRefusalCode, RetentionDecisionRefusal> {
  const codes =
    isRecord(catalogue) && Array.isArray(catalogue.codes)
      ? (catalogue.codes as unknown[])
      : [];
  const table = new Map<
    RetentionDecisionRefusalCode,
    RetentionDecisionRefusal
  >();
  for (const code of REFUSAL_CODES) {
    const entry = codes.find((item) => isRecord(item) && item.code === code);
    if (
      !isRecord(entry) ||
      typeof entry.message_en !== 'string' ||
      entry.message_en.length === 0 ||
      typeof entry.retryable !== 'boolean' ||
      typeof entry.action !== 'string' ||
      entry.action.length === 0
    ) {
      throw new Error(`zdr-refusals catalogue has no usable entry for ${code}`);
    }
    table.set(code, {
      code,
      message: entry.message_en,
      retryable: entry.retryable,
      action: entry.action,
    });
  }
  return table;
}

const REFUSALS = loadRetentionDecisionRefusals(zdrRefusalsFixture);

/** The catalogue entry for a refusal code. */
export function retentionDecisionRefusal(
  code: RetentionDecisionRefusalCode,
): RetentionDecisionRefusal {
  return REFUSALS.get(code) as RetentionDecisionRefusal;
}

/**
 * HTTP status of a refusal: 503 for `zdr_decision_unavailable`, 412 for every
 * other code (the model-gateway http_status rule).
 */
export function retentionDecisionRefusalStatus(
  code: RetentionDecisionRefusalCode,
): number {
  return code === 'zdr_decision_unavailable' ? 503 : 412;
}

/** The three outcomes of a decision that is not a refusal. */
export type RetentionDecisionKind = 'zdr' | 'persistent' | 'waived';

/** Reason codes of a decision (matrix `vocabulary.reason_codes`). */
export type RetentionDecisionReason =
  | 'org_zdr_off'
  | 'org_zdr'
  | 'explicit_zdr'
  | 'connection_supports_zdr'
  | 'connection_not_zdr_capable';

/** Retention class of a decision (matrix `vocabulary.retention_classes`). */
export type RetentionDecisionClass = 'zdr' | 'persistent' | 'zdr_waived';

/** What the caller asked, plus the facts of the session it came from. */
export type RetentionDecisionInput = Readonly<{
  /** Better Auth user id of the session. */
  userId: string;
  /** Active organization of the session. */
  orgId: string;
  /** Better Auth session id. */
  sessionId: string;
  /** Connection the turn names, or null. */
  connectionId: string | null;
  /** Provider the request names, or null. */
  provider: string | null;
  /** The turn asked for ZDR itself (Temporary chat, x-zdr, body zdr). */
  explicitZdr: boolean;
}>;

/** Content-free facts the audit event records for one decision. */
export type RetentionDecisionAuditFacts = Readonly<{
  /** Provider of the connection read, else the request provider, else ''. */
  provider: string;
  /** Connection id, or ''. */
  connectionId: string;
  explicitZdr: boolean;
  /** Value of the kill switch at boot. */
  waiverEnabled: boolean;
  /** Acknowledgement state read from integration-corev2, or ''. */
  ackState: string;
  /** Closed reason the acknowledgement is not in force, or ''. */
  ackReason: string;
  /** Id of the acknowledgement in force, or ''. */
  ackId: string;
}>;

/** A decision that is not a refusal. */
export type RetentionDecisionDecided = Readonly<{
  kind: 'decided';
  decision: RetentionDecisionKind;
  reason: RetentionDecisionReason;
  orgZdr: boolean;
  effectiveZdr: boolean;
  retentionClass: RetentionDecisionClass;
  /** Present only for `waived`. */
  signed?: SignedRetentionDecision;
  facts: RetentionDecisionAuditFacts;
}>;

/** A refusal. `orgZdr` is null when the org posture was not known. */
export type RetentionDecisionRefused = Readonly<{
  kind: 'refused';
  code: RetentionDecisionRefusalCode;
  orgZdr: boolean | null;
  facts: RetentionDecisionAuditFacts;
}>;

/** Result of one decision. */
export type RetentionDecisionOutcome =
  | RetentionDecisionDecided
  | RetentionDecisionRefused;

@Injectable()
export class RetentionDecisionService {
  private readonly logger = new Logger(RetentionDecisionService.name);

  constructor(
    @Inject(RETENTION_DECISION_SIGNER)
    private readonly signer: RetentionDecisionSigner,
    private readonly connections: IntegrationConnectionPostureClient,
    @Inject(ZDR_WAIVER_ENABLED_TOKEN)
    private readonly waiverEnabled: boolean,
  ) {
    // Once at boot: the operator sees both halves of "can a waiver be minted".
    this.logger.log(
      `ZDR waiver kill switch is ${this.waiverEnabled ? 'ON' : 'off'}; decision signer is ${this.signer.available() ? 'available' : 'unavailable'}`,
    );
  }

  /** Evaluates the matrix rule for one turn. Never throws. */
  async decide(
    input: RetentionDecisionInput,
  ): Promise<RetentionDecisionOutcome> {
    const baseFacts: RetentionDecisionAuditFacts = {
      provider: input.provider ?? '',
      connectionId: input.connectionId ?? '',
      explicitZdr: input.explicitZdr,
      waiverEnabled: this.waiverEnabled,
      ackState: '',
      ackReason: '',
      ackId: '',
    };

    // 1. The org posture O. Unknown refuses; nothing else is read.
    let posture: InteractiveRetentionPosture;
    try {
      posture = await resolveInteractiveRetentionPosture(input.orgId);
    } catch {
      return this.refused('zdr_decision_unavailable', null, baseFacts);
    }
    const orgZdr = posture.zdr;

    // 2. A connection is named and (O or explicit_zdr): read it as the user.
    let connection: ConnectionPosture | null = null;
    let facts = baseFacts;
    if (input.connectionId !== null && (orgZdr || input.explicitZdr)) {
      let read: ConnectionPostureResult;
      try {
        read = await this.connections.readConnectionPosture({
          userId: input.userId,
          orgId: input.orgId,
          connectionId: input.connectionId,
          retentionPosture: posture,
        });
      } catch {
        return this.refused('zdr_decision_unavailable', orgZdr, baseFacts);
      }
      if (read.kind === 'unavailable') {
        return this.refused('zdr_decision_unavailable', orgZdr, baseFacts);
      }
      if (read.kind === 'not_owner') {
        // Only the owner can acknowledge, so no acknowledgement can exist for
        // anyone else.
        return this.refused(
          'zdr_connection_not_acknowledged',
          orgZdr,
          baseFacts,
        );
      }
      connection = read;
      facts = {
        ...baseFacts,
        provider: connection.providerKey,
        ackState: connection.ackState,
        ackReason: connection.ackReason,
        ackId: connection.ackId,
      };
      if (connection.ackState === 'unassessed') {
        // No capability record yet (the start-up reconcile is pending): retry,
        // never "not supported".
        return this.refused('zdr_decision_unavailable', orgZdr, facts);
      }
      if (
        input.provider !== null &&
        input.provider !== connection.providerKey
      ) {
        return this.refused('zdr_waiver_route_mismatch', orgZdr, facts);
      }
    }

    // A connection counts as supported only when its capability is exactly
    // 'supported'. not_supported and cannot_verify are both unsupported.
    const supported =
      connection !== null && connection.capability === 'supported';

    // 3. An explicit ZDR request is never covered by a waiver, and this comes
    //    before the kill switch.
    if (input.explicitZdr && connection !== null && !supported) {
      return this.refused(
        'zdr_explicit_request_not_supported_by_connection',
        orgZdr,
        facts,
      );
    }

    // The reason of a zdr decision: a supported connection, else the explicit
    // request, else the org default.
    const zdrReason: RetentionDecisionReason = supported
      ? 'connection_supports_zdr'
      : input.explicitZdr
        ? 'explicit_zdr'
        : 'org_zdr';

    // 4. Not O: persistent; the request flag raises the posture, never lowers.
    if (!orgZdr) {
      return input.explicitZdr
        ? this.decided('zdr', zdrReason, orgZdr, facts)
        : this.decided('persistent', 'org_zdr_off', orgZdr, facts);
    }

    // 5. O and no connection. 6. O and a supported connection.
    if (connection === null || supported) {
      return this.decided('zdr', zdrReason, orgZdr, facts);
    }

    // 7. O and an unsupported connection: a waiver is the only way to run it.
    if (
      !this.waiverEnabled ||
      !connection.waiverRoute ||
      !WAIVER_PROVIDERS.includes(connection.providerKey)
    ) {
      return this.refused('zdr_connection_not_supported', orgZdr, facts);
    }
    if (connection.ackState !== 'acknowledged') {
      return this.refused('zdr_connection_not_acknowledged', orgZdr, facts);
    }
    if (!this.signer.available()) {
      return this.refused('zdr_decision_unavailable', orgZdr, facts);
    }
    let signed: SignedRetentionDecision;
    try {
      signed = this.signer.sign({
        userId: input.userId,
        orgId: input.orgId,
        sessionId: input.sessionId,
        connection: input.connectionId as string,
        provider: connection.providerKey,
        ackId: connection.ackId,
        digest: connection.digest,
      });
    } catch {
      // An unclean claim (empty ack id or digest) or a signer fault: doubt.
      return this.refused('zdr_decision_unavailable', orgZdr, facts);
    }
    return this.decided(
      'waived',
      'connection_not_zdr_capable',
      orgZdr,
      facts,
      signed,
    );
  }

  private decided(
    decision: RetentionDecisionKind,
    reason: RetentionDecisionReason,
    orgZdr: boolean,
    facts: RetentionDecisionAuditFacts,
    signed?: SignedRetentionDecision,
  ): RetentionDecisionDecided {
    return {
      kind: 'decided',
      decision,
      reason,
      orgZdr,
      effectiveZdr: decision === 'zdr',
      retentionClass:
        decision === 'waived'
          ? 'zdr_waived'
          : decision === 'zdr'
            ? 'zdr'
            : 'persistent',
      ...(signed ? { signed } : {}),
      facts,
    };
  }

  private refused(
    code: RetentionDecisionRefusalCode,
    orgZdr: boolean | null,
    facts: RetentionDecisionAuditFacts,
  ): RetentionDecisionRefused {
    return { kind: 'refused', code, orgZdr, facts };
  }
}
