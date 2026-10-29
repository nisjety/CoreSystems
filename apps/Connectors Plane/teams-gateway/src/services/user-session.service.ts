/**
 * User Session Management Service
 * 
 * Manages user authentication state across services using NATS events
 * Provides centralized session tracking and user context
 */

import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { UserState } from '../entities/user-state.entity';
import { User } from '../entities/user.entity';

export interface UserSessionContext {
  userId: string;
  email: string;
  displayName?: string;
  tenantId?: string;
  sessionId: string;
  isAuthenticated: boolean;
  provider?: string;
  roles?: string[];
  permissions?: string[];
  lastActivity: Date;
  expiresAt?: Date;
  metadata?: Record<string, any>;
}

export interface SessionMetrics {
  totalSessions: number;
  activeSessions: number;
  averageSessionDuration: number;
  totalUsers: number;
  activeUsers: number;
}

@Injectable()
export class UserSessionService {
  private readonly logger = new Logger(UserSessionService.name);
  private userSessions = new Map<string, UserSessionContext>(); // sessionId -> context
  private userToSession = new Map<string, string>(); // userId -> sessionId
  private sessionActivity = new Map<string, Date>(); // sessionId -> lastActivity

  constructor(
    @InjectRepository(UserState)
    private userStateRepository: Repository<UserState>,
    @InjectRepository(User)
    private userRepository: Repository<User>,
    private configService: ConfigService,
  ) {
    // Clean up expired sessions every 5 minutes
    setInterval(() => this.cleanupExpiredSessions(), 5 * 60 * 1000);
  }

  /**
   * Create or update user session from auth event
   */
  async createSession(
    userId: string,
    sessionId: string,
    userInfo: {
      email: string;
      displayName?: string;
      tenantId?: string;
      provider?: string;
      expiresAt?: Date;
      metadata?: Record<string, any>;
    }
  ): Promise<UserSessionContext> {
    
    const context: UserSessionContext = {
      userId,
      email: userInfo.email,
      displayName: userInfo.displayName,
      tenantId: userInfo.tenantId,
      sessionId,
      isAuthenticated: true,
      provider: userInfo.provider,
      lastActivity: new Date(),
      expiresAt: userInfo.expiresAt,
      metadata: userInfo.metadata,
    };

    // Store session context
    this.userSessions.set(sessionId, context);
    this.userToSession.set(userId, sessionId);
    this.sessionActivity.set(sessionId, new Date());

    // Update user state in database
    await this.updateUserState(userId, sessionId);

    this.logger.log(`Session created: ${sessionId} for user ${userId} (${userInfo.email})`);
    
    return context;
  }

  /**
   * End user session
   */
  async endSession(sessionId: string, reason: 'logout' | 'expired' | 'revoked' = 'logout'): Promise<void> {
    const context = this.userSessions.get(sessionId);
    if (!context) {
      this.logger.warn(`Attempt to end non-existent session: ${sessionId}`);
      return;
    }

    // Remove from maps
    this.userSessions.delete(sessionId);
    this.userToSession.delete(context.userId);
    this.sessionActivity.delete(sessionId);

    // Clear active session in database
    await this.clearActiveSession(context.userId);

    this.logger.log(`Session ended: ${sessionId} for user ${context.userId} (reason: ${reason})`);
  }

  /**
   * Get session by session ID
   */
  getSession(sessionId: string): UserSessionContext | null {
    const context = this.userSessions.get(sessionId);
    if (!context) {
      return null;
    }

    // Check if session has expired
    if (context.expiresAt && context.expiresAt < new Date()) {
      this.endSession(sessionId, 'expired');
      return null;
    }

    return context;
  }

  /**
   * Get session by user ID
   */
  getSessionByUserId(userId: string): UserSessionContext | null {
    const sessionId = this.userToSession.get(userId);
    if (!sessionId) {
      return null;
    }

    return this.getSession(sessionId);
  }

  /**
   * Update session activity
   */
  updateActivity(sessionId: string): void {
    const context = this.userSessions.get(sessionId);
    if (context) {
      context.lastActivity = new Date();
      this.sessionActivity.set(sessionId, new Date());
    }
  }

  /**
   * Check if user is authenticated
   */
  isAuthenticated(userId: string): boolean {
    const context = this.getSessionByUserId(userId);
    return context?.isAuthenticated ?? false;
  }

  /**
   * Get user context for Teams user
   */
  async getUserContextFromTeamsId(teamsUserId: string, teamsEmail: string): Promise<UserSessionContext | null> {
    try {
      // First try to find existing session by email
      const existingSession = Array.from(this.userSessions.values())
        .find(session => session.email.toLowerCase() === teamsEmail.toLowerCase());

      if (existingSession) {
        this.updateActivity(existingSession.sessionId);
        return existingSession;
      }

      // Try to find user in database
      let user = await this.userRepository.findOne({
        where: { email: teamsEmail.toLowerCase() }
      });

      if (!user) {
        // Create new user record
        user = this.userRepository.create({
          email: teamsEmail.toLowerCase(),
          teams_user_id: teamsUserId,
          display_name: teamsEmail.split('@')[0], // fallback display name
          tenant_id: '', // Will be updated when available
          is_active: true,
        });
        user = await this.userRepository.save(user);
        this.logger.log(`Created new user record for Teams user: ${teamsEmail}`);
      }

      // Create session for Teams user
      const sessionId = `teams-${teamsUserId}-${Date.now()}`;
      const context = await this.createSession(user.id, sessionId, {
        email: user.email,
        displayName: user.display_name,
        tenantId: user.tenant_id,
        provider: 'teams',
        metadata: { teamsUserId }
      });

      return context;

    } catch (error) {
      this.logger.error(`Error getting user context for Teams user ${teamsEmail}`, error);
      return null;
    }
  }

  /**
   * Get all active sessions
   */
  getActiveSessions(): UserSessionContext[] {
    return Array.from(this.userSessions.values());
  }

  /**
   * Get session metrics
   */
  getSessionMetrics(): SessionMetrics {
    const activeSessions = this.getActiveSessions();
    const uniqueUsers = new Set(activeSessions.map(s => s.userId));

    // Calculate average session duration (simplified)
    const totalDuration = activeSessions.reduce((total, session) => {
      const duration = Date.now() - session.lastActivity.getTime();
      return total + duration;
    }, 0);

    const averageSessionDuration = activeSessions.length > 0 
      ? totalDuration / activeSessions.length 
      : 0;

    return {
      totalSessions: activeSessions.length, // This is simplified - in production, track historical data
      activeSessions: activeSessions.length,
      averageSessionDuration,
      totalUsers: uniqueUsers.size, // This is simplified - in production, query database
      activeUsers: uniqueUsers.size,
    };
  }

  /**
   * Clean up expired sessions
   */
  private cleanupExpiredSessions(): void {
    const now = new Date();
    const expiredSessions: string[] = [];

    for (const [sessionId, context] of this.userSessions.entries()) {
      // Check explicit expiration
      if (context.expiresAt && context.expiresAt < now) {
        expiredSessions.push(sessionId);
        continue;
      }

      // Check inactivity timeout (default: 24 hours)
      const inactivityTimeout = 24 * 60 * 60 * 1000; // 24 hours
      const lastActivity = this.sessionActivity.get(sessionId);
      if (lastActivity && (now.getTime() - lastActivity.getTime()) > inactivityTimeout) {
        expiredSessions.push(sessionId);
      }
    }

    // Clean up expired sessions
    for (const sessionId of expiredSessions) {
      this.endSession(sessionId, 'expired');
    }

    if (expiredSessions.length > 0) {
      this.logger.log(`Cleaned up ${expiredSessions.length} expired sessions`);
    }
  }

  /**
   * Update user state in database
   */
  private async updateUserState(userId: string, sessionId: string): Promise<void> {
    try {
      let userState = await this.userStateRepository.findOne({
        where: { userId }
      });

      if (!userState) {
        userState = this.userStateRepository.create({
          userId,
          activeSessionId: sessionId,
          autoNameEnabled: true, // default
        });
      } else {
        userState.activeSessionId = sessionId;
      }

      await this.userStateRepository.save(userState);
    } catch (error) {
      this.logger.error(`Error updating user state for ${userId}`, error);
    }
  }

  /**
   * Clear active session in database
   */
  private async clearActiveSession(userId: string): Promise<void> {
    try {
      await this.userStateRepository.update(
        { userId },
        { activeSessionId: null }
      );
    } catch (error) {
      this.logger.error(`Error clearing active session for ${userId}`, error);
    }
  }
}