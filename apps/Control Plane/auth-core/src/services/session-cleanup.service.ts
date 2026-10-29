import { Injectable, Logger } from '@nestjs/common';
import { Cron, CronExpression } from '@nestjs/schedule';
import { redisSecondaryStorage } from '../db/redis';

@Injectable()
export class SessionCleanupService {
  private readonly logger = new Logger(SessionCleanupService.name);

  // Run cleanup every 15 minutes
  @Cron(CronExpression.EVERY_30_MINUTES)
  async handleCleanup() {
    this.logger.log('🧹 Starting session and state cleanup...');

    try {
      await redisSecondaryStorage.cleanupExpiredStates();
      this.logger.log('✅ Session cleanup completed successfully');
    } catch (error) {
      this.logger.error('❌ Session cleanup failed:', error);
    }
  }

  // Manual cleanup method for testing
  async manualCleanup(): Promise<void> {
    this.logger.log('🧹 Manual cleanup initiated...');
    await redisSecondaryStorage.cleanupExpiredStates();
    this.logger.log('✅ Manual cleanup completed');
  }
}
