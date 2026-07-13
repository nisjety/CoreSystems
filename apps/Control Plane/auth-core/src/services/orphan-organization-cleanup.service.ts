import { Injectable, Logger } from '@nestjs/common';
import { Cron, CronExpression } from '@nestjs/schedule';
import { sqlClient } from '../db';
import {
  flushOrganizationProjectionOutbox,
  flushOrganizationMembershipOutbox,
  flushOrganizationDeletionOutbox,
} from '../auth/organization-events.plugin';

export type OwnerlessOrganizationMode = 'off' | 'report';

export function ownerlessOrganizationMode(
  value: string | undefined,
): OwnerlessOrganizationMode {
  return value?.trim().toLowerCase() === 'report' ? 'report' : 'off';
}

@Injectable()
export class OrphanOrganizationCleanupService {
  private readonly logger = new Logger(OrphanOrganizationCleanupService.name);

  // Reconcile durable outboxes every minute. Historical ownerless rows are
  // never auto-deleted: age alone cannot prove a failed partial write. An
  // explicit report-only mode inventories candidates for a separate, reviewed
  // Auth-derived repair workflow.
  @Cron(CronExpression.EVERY_MINUTE)
  async reconcileOrganizationOutboxes(): Promise<void> {
    try {
      const published = await flushOrganizationProjectionOutbox();
      if (published > 0) {
        this.logger.log(
          `Published ${published} pending organization projection(s)`,
        );
      }
      const memberships = await flushOrganizationMembershipOutbox();
      if (memberships > 0) {
        this.logger.log(`Reconciled ${memberships} organization membership(s)`);
      }
      const deletions = await flushOrganizationDeletionOutbox();
      if (deletions > 0) {
        this.logger.log(`Reconciled ${deletions} organization deletion(s)`);
      }
      if (
        ownerlessOrganizationMode(process.env.OWNERLESS_ORGANIZATION_MODE) !==
        'report'
      ) {
        return;
      }

      const candidates = await sqlClient<
        Array<{ id: string; slug: string | null }>
      >`
        SELECT o.id, o.slug FROM organization o
        WHERE o.created_at < NOW() - INTERVAL '2 minutes'
          AND NOT EXISTS (
            SELECT 1 FROM member m
            WHERE m.organization_id = o.id
              AND 'owner' = ANY(string_to_array(m.role, ','))
          )
      `;
      if (candidates.length > 0) {
        this.logger.warn(
          `Ownerless organization report found ${candidates.length} candidate(s); no rows were changed: ${candidates
            .map((organization) => organization.id)
            .join(', ')}`,
        );
      }
    } catch (error) {
      this.logger.error('Organization reconciliation sweep failed', error);
    }
  }
}
