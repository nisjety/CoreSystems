import { Injectable, Logger } from '@nestjs/common';
import { Cron, CronExpression } from '@nestjs/schedule';

import { flushInvitationAcceptanceRepairs } from '../auth/invitation-acceptance-repair';

@Injectable()
export class InvitationAcceptanceRepairService {
  private readonly logger = new Logger(InvitationAcceptanceRepairService.name);

  @Cron(CronExpression.EVERY_MINUTE)
  async reconcileAcceptedInvitations(): Promise<void> {
    try {
      const summary = await flushInvitationAcceptanceRepairs();
      if (summary.claimed > 0) {
        this.logger.log({
          event: 'invitation_acceptance_repair_sweep',
          ...summary,
        });
      }
      if (summary.deadLettered > 0) {
        this.logger.warn({
          event: 'invitation_acceptance_repair_dead_letter',
          deadLettered: summary.deadLettered,
          action:
            'inspect invitation_acceptance_repair without exposing invitation identifiers',
        });
      }
    } catch (error) {
      this.logger.error('Invitation acceptance repair sweep failed', error);
    }
  }
}
