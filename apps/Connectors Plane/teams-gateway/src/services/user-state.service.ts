import { Injectable, Logger } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { UserState } from '../entities/user-state.entity';

export interface UserPreferences {
  autoNameEnabled: boolean;
  maxSessions?: number;
  defaultSessionTitle?: string;
}

@Injectable()
export class UserStateService {
  private readonly logger = new Logger(UserStateService.name);

  constructor(
    @InjectRepository(UserState)
    private userStateRepository: Repository<UserState>,
  ) {}

  /**
   * Get user state, create if doesn't exist
   */
  async getUserState(userId: string): Promise<UserState> {
    let userState = await this.userStateRepository.findOne({
      where: { userId }
    });

    if (!userState) {
      userState = this.userStateRepository.create({
        userId,
        autoNameEnabled: true, // Default to auto-naming enabled
      });
      
      userState = await this.userStateRepository.save(userState);
      this.logger.log(`Created user state for ${userId}`);
    }

    return userState;
  }

  /**
   * Get user's active session ID
   */
  async getActiveSessionId(userId: string): Promise<string | null> {
    const userState = await this.getUserState(userId);
    return userState.activeSessionId;
  }

  /**
   * Set user's active session
   */
  async setActiveSession(userId: string, sessionId: string | null): Promise<void> {
    await this.userStateRepository.upsert(
      {
        userId,
        activeSessionId: sessionId,
        lastSwitcherShownAt: sessionId ? new Date() : undefined
      },
      ['userId']
    );

    this.logger.log(`Set active session for ${userId}: ${sessionId || 'none'}`);
  }

  /**
   * Clear active session for user
   */
  async clearActiveSession(userId: string): Promise<void> {
    await this.setActiveSession(userId, null);
  }

  /**
   * Update when thread switcher was last shown
   */
  async updateSwitcherShown(userId: string): Promise<void> {
    await this.userStateRepository.update(
      { userId },
      { lastSwitcherShownAt: new Date() }
    );
  }

  /**
   * Check if thread switcher should be shown (not shown recently)
   */
  async shouldShowSwitcher(userId: string, cooldownMinutes = 30): Promise<boolean> {
    const userState = await this.getUserState(userId);
    
    if (!userState.lastSwitcherShownAt) {
      return true;
    }

    const cooldownMs = cooldownMinutes * 60 * 1000;
    const timeSinceLastShown = Date.now() - userState.lastSwitcherShownAt.getTime();
    
    return timeSinceLastShown > cooldownMs;
  }

  /**
   * Get user preferences
   */
  async getUserPreferences(userId: string): Promise<UserPreferences> {
    const userState = await this.getUserState(userId);
    
    return {
      autoNameEnabled: userState.autoNameEnabled,
      // Add other preferences as needed
    };
  }

  /**
   * Update user preferences
   */
  async updatePreferences(userId: string, preferences: Partial<UserPreferences>): Promise<void> {
    const updates: Partial<UserState> = {};
    
    if (preferences.autoNameEnabled !== undefined) {
      updates.autoNameEnabled = preferences.autoNameEnabled;
    }

    if (Object.keys(updates).length > 0) {
      await this.userStateRepository.update({ userId }, updates);
      this.logger.log(`Updated preferences for ${userId}:`, updates);
    }
  }

  /**
   * Enable/disable auto-naming for user
   */
  async toggleAutoNaming(userId: string, enabled: boolean): Promise<void> {
    await this.updatePreferences(userId, { autoNameEnabled: enabled });
  }

  /**
   * Check if auto-naming is enabled for user
   */
  async isAutoNamingEnabled(userId: string): Promise<boolean> {
    const userState = await this.getUserState(userId);
    return userState.autoNameEnabled;
  }

  /**
   * Track session switch activity
   */
  async trackSessionSwitch(userId: string, fromSessionId: string | null, toSessionId: string): Promise<void> {
    await this.setActiveSession(userId, toSessionId);
    
    this.logger.debug(`User ${userId} switched from session ${fromSessionId || 'none'} to ${toSessionId}`);
  }

  /**
   * Get user activity summary
   */
  async getUserActivitySummary(userId: string): Promise<{
    hasActiveSession: boolean;
    activeSessionId: string | null;
    lastSwitcherShown: Date | null;
    autoNamingEnabled: boolean;
  }> {
    const userState = await this.getUserState(userId);
    
    return {
      hasActiveSession: !!userState.activeSessionId,
      activeSessionId: userState.activeSessionId,
      lastSwitcherShown: userState.lastSwitcherShownAt,
      autoNamingEnabled: userState.autoNameEnabled
    };
  }

  /**
   * Clean up inactive user states (GDPR compliance)
   */
  async cleanupInactiveUsers(inactiveDays = 365): Promise<number> {
    const cutoffDate = new Date();
    cutoffDate.setDate(cutoffDate.getDate() - inactiveDays);

    const result = await this.userStateRepository
      .createQueryBuilder()
      .delete()
      .where('last_switcher_shown_at < :cutoffDate OR last_switcher_shown_at IS NULL', { cutoffDate })
      .andWhere('active_session_id IS NULL')
      .execute();

    const deletedCount = result.affected || 0;
    if (deletedCount > 0) {
      this.logger.log(`Cleaned up ${deletedCount} inactive user states`);
    }

    return deletedCount;
  }

  /**
   * Export user state data (GDPR compliance)
   */
  async exportUserData(userId: string): Promise<any> {
    const userState = await this.getUserState(userId);
    
    return {
      userId: userState.userId,
      activeSessionId: userState.activeSessionId,
      autoNameEnabled: userState.autoNameEnabled,
      lastSwitcherShownAt: userState.lastSwitcherShownAt,
      createdAt: userState.createdAt,
      updatedAt: userState.updatedAt
    };
  }

  /**
   * Delete all user data (GDPR right to be forgotten)
   */
  async deleteUserData(userId: string): Promise<boolean> {
    const result = await this.userStateRepository.delete({ userId });
    
    if (result.affected > 0) {
      this.logger.log(`Deleted user state for ${userId}`);
      return true;
    }
    
    return false;
  }
}
