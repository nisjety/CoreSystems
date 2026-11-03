import {
  Injectable,
  Logger,
  OnModuleDestroy,
  OnModuleInit,
} from '@nestjs/common';
import { and, eq, ilike } from 'drizzle-orm';
import { db } from '../db';
import * as schema from '../db/schema';
import { DirectNatsService } from './direct-nats.service';
import {
  API_KEY_USER_CONFIG_ID,
  apiKeyAgentExchangeEnabled,
  sweepApiKeyRevocations,
  type ApiKeyRevocationRow,
} from '../auth/api-key-metadata';

/**
 * INNH-10 CP-3 wiring: Postgres reads for the revocation path and the periodic
 * expiry / owner-removal sweep that publishes
 * `verevon.audit.v2.control.auth-core.api_key_revoked` on the LOCAL
 * control-plane JetStream (same transport as every other auth-core audit
 * event). Delete and disable are published fail-closed from the Better Auth
 * before-hook in auth.ts; this file covers what has no hook.
 */

const DEFAULT_SWEEP_INTERVAL_MS = 60_000;
const MIN_SWEEP_INTERVAL_MS = 5_000;

function sweepIntervalMs(env: NodeJS.ProcessEnv = process.env): number {
  const parsed = Number.parseInt(env.API_KEY_REVOCATION_SWEEP_MS ?? '', 10);
  if (!Number.isFinite(parsed) || parsed <= 0) return DEFAULT_SWEEP_INTERVAL_MS;
  return Math.max(MIN_SWEEP_INTERVAL_MS, parsed);
}

const revocationColumns = {
  id: schema.apikey.id,
  configId: schema.apikey.configId,
  referenceId: schema.apikey.referenceId,
  enabled: schema.apikey.enabled,
  expiresAt: schema.apikey.expiresAt,
  updatedAt: schema.apikey.updatedAt,
  permissions: schema.apikey.permissions,
  metadata: schema.apikey.metadata,
};

/** One key row by id, or null (database storage, the plugin default). */
export async function loadApiKeyRevocationRow(
  keyId: string,
): Promise<ApiKeyRevocationRow | null> {
  if (typeof keyId !== 'string' || keyId.length === 0) return null;
  const [row] = await db
    .select(revocationColumns)
    .from(schema.apikey)
    .where(eq(schema.apikey.id, keyId))
    .limit(1);
  return row ?? null;
}

/** The caller's `member.role` in an org, or null when not a member. */
export async function loadOrganizationRole(
  userId: string,
  organizationId: string,
): Promise<string | null> {
  const [row] = await db
    .select({ role: schema.member.role })
    .from(schema.member)
    .where(
      and(
        eq(schema.member.userId, userId),
        eq(schema.member.organizationId, organizationId),
      ),
    )
    .limit(1);
  return row?.role ?? null;
}

async function listAgentKeyCandidates(): Promise<ApiKeyRevocationRow[]> {
  // Coarse SQL filter; apiKeyRevocationTarget() makes the exact decision.
  return db
    .select(revocationColumns)
    .from(schema.apikey)
    .where(
      and(
        eq(schema.apikey.configId, API_KEY_USER_CONFIG_ID),
        eq(schema.apikey.enabled, true),
        ilike(schema.apikey.metadata, '%key_hosts%'),
      ),
    );
}

async function disableApiKey(keyId: string, now: Date): Promise<void> {
  // Direct write on purpose: the Better Auth update hook would publish a
  // second ('disabled') event for the same revocation.
  await db
    .update(schema.apikey)
    .set({ enabled: false, updatedAt: now })
    .where(eq(schema.apikey.id, keyId));
}

@Injectable()
export class ApiKeyRevocationSweeper implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(ApiKeyRevocationSweeper.name);
  private timer: NodeJS.Timeout | null = null;
  private running = false;

  constructor(private readonly directNats: DirectNatsService) {}

  onModuleInit(): void {
    // Inert unless browser:agent keys can exist (API_KEY_ENABLED and the
    // CP-1 exchange flag); no flag of its own.
    if (!apiKeyAgentExchangeEnabled()) return;
    const interval = sweepIntervalMs();
    this.timer = setInterval(() => {
      void this.sweepOnce();
    }, interval);
    this.timer.unref?.();
  }

  onModuleDestroy(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  async sweepOnce(): Promise<void> {
    if (this.running) return;
    this.running = true;
    try {
      const result = await sweepApiKeyRevocations({
        listCandidates: listAgentKeyCandidates,
        isActiveMember: async (userId, orgId) =>
          (await loadOrganizationRole(userId, orgId)) !== null,
        publish: (subject, payload) =>
          this.directNats.publishAuditDurable(subject, payload),
        disable: disableApiKey,
        now: () => new Date(),
      });
      if (result.revoked.length > 0) {
        this.logger.log(
          `api_key_revoked published for ${result.revoked.length} key(s)`,
        );
      }
      if (result.failed.length > 0) {
        this.logger.error(
          `api_key_revoked publish failed for ${result.failed.length} key(s); retrying next sweep`,
        );
      }
    } catch (error) {
      this.logger.error(
        `api key revocation sweep failed: ${error instanceof Error ? error.message : 'unknown error'}`,
      );
    } finally {
      this.running = false;
    }
  }
}
