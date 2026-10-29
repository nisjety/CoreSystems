import { Injectable, Logger } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository, In } from 'typeorm';
import { UserSession, SessionStatus } from '../entities/user-session.entity';
import { SessionMessage, MessageRole } from '../entities/session-message.entity';
import { AiTitleGeneratorService, TitleGenerationRequest } from './ai-title-generator.service';

export interface CreateSessionDto {
  userId: string;
  title?: string;
}

export interface SessionSummary {
  id: string;
  title: string;
  status: SessionStatus;
  isPinned: boolean;
  messageCount: number;
  lastMessageAt: Date;
  createdAt: Date;
}

@Injectable()
export class SessionService {
  private readonly logger = new Logger(SessionService.name);

  constructor(
    @InjectRepository(UserSession)
    private sessionRepository: Repository<UserSession>,
    @InjectRepository(SessionMessage)
    private messageRepository: Repository<SessionMessage>,
    private aiTitleGeneratorService: AiTitleGeneratorService,
  ) {}

  /**
   * Create a new session for a user
   */
  async createSession(dto: CreateSessionDto): Promise<UserSession> {
    const session = this.sessionRepository.create({
      userId: dto.userId,
      title: dto.title || 'Ny samtale',
      status: SessionStatus.ACTIVE,
      messageCount: 0
    });

    const saved = await this.sessionRepository.save(session);
    this.logger.log(`Created new session ${saved.id} for user ${dto.userId}: "${saved.title}"`);
    return saved;
  }

  /**
   * Get user's sessions with optional filtering
   */
  async getUserSessions(
    userId: string, 
    status?: SessionStatus, 
    limit = 10
  ): Promise<SessionSummary[]> {
    const query = this.sessionRepository
      .createQueryBuilder('session')
      .leftJoin('session.messages', 'message')
      .addSelect('MAX(message.createdAt)', 'lastMessageAt')
      .where('session.userId = :userId', { userId })
      .groupBy('session.id')
      .orderBy('session.isPinned', 'DESC')
      .addOrderBy('session.updatedAt', 'DESC')
      .limit(limit);

    if (status) {
      query.andWhere('session.status = :status', { status });
    }

    const sessions = await query.getRawAndEntities();
    
    return sessions.entities.map((session, index) => ({
      id: session.id,
      title: session.title,
      status: session.status,
      isPinned: session.isPinned,
      messageCount: session.messageCount,
      lastMessageAt: sessions.raw[index]?.lastMessageAt || session.createdAt,
      createdAt: session.createdAt
    }));
  }

  /**
   * Get active sessions for a user
   */
  async getActiveSessions(userId: string, limit = 10): Promise<SessionSummary[]> {
    return this.getUserSessions(userId, SessionStatus.ACTIVE, limit);
  }

  /**
   * Get session by ID
   */
  async getSession(sessionId: string): Promise<UserSession | null> {
    return this.sessionRepository.findOne({
      where: { id: sessionId }
    });
  }

  /**
   * Get session with messages
   */
  async getSessionWithMessages(sessionId: string, limit = 50): Promise<UserSession | null> {
    return this.sessionRepository.findOne({
      where: { id: sessionId },
      relations: ['messages'],
      order: {
        messages: {
          createdAt: 'ASC'
        }
      }
    });
  }

  /**
   * Add message to session
   */
  async addMessage(
    sessionId: string, 
    role: MessageRole, 
    content: string, 
    metadata?: Record<string, any>
  ): Promise<SessionMessage> {
    const message = this.messageRepository.create({
      sessionId,
      role,
      content,
      metadata
    });

    const saved = await this.messageRepository.save(message);

    // Update session message count and timestamp
    await this.sessionRepository.update(sessionId, {
      messageCount: () => 'message_count + 1',
      updatedAt: new Date()
    });

    this.logger.debug(`Added ${role} message to session ${sessionId}`);

    // Try to auto-generate title after first few messages
    this.tryAutoGenerateTitle(sessionId).catch(error => {
      this.logger.warn(`Auto title generation failed for session ${sessionId}: ${error.message}`);
    });

    return saved;
  }

  /**
   * Get recent messages from session for context
   */
  async getRecentMessages(sessionId: string, limit = 20): Promise<SessionMessage[]> {
    return this.messageRepository.find({
      where: { sessionId },
      order: { createdAt: 'DESC' },
      take: limit
    });
  }

  /**
   * Update session title
   */
  async updateTitle(sessionId: string, title: string): Promise<boolean> {
    const result = await this.sessionRepository.update(sessionId, { 
      title,
      updatedAt: new Date()
    });
    
    if (result.affected > 0) {
      this.logger.log(`Updated session ${sessionId} title to: "${title}"`);
      return true;
    }
    return false;
  }

  /**
   * Archive a session
   */
  async archiveSession(sessionId: string): Promise<boolean> {
    const result = await this.sessionRepository.update(sessionId, { 
      status: SessionStatus.ARCHIVED,
      updatedAt: new Date()
    });

    if (result.affected > 0) {
      this.logger.log(`Archived session ${sessionId}`);
      return true;
    }
    return false;
  }

  /**
   * Restore archived session
   */
  async restoreSession(sessionId: string): Promise<boolean> {
    const result = await this.sessionRepository.update(sessionId, { 
      status: SessionStatus.ACTIVE,
      updatedAt: new Date()
    });

    if (result.affected > 0) {
      this.logger.log(`Restored session ${sessionId}`);
      return true;
    }
    return false;
  }

  /**
   * Delete a session and all its messages
   */
  async deleteSession(sessionId: string): Promise<boolean> {
    const result = await this.sessionRepository.delete(sessionId);
    
    if (result.affected > 0) {
      this.logger.log(`Deleted session ${sessionId}`);
      return true;
    }
    return false;
  }

  /**
   * Pin/unpin a session
   */
  async togglePin(sessionId: string, isPinned: boolean): Promise<boolean> {
    const result = await this.sessionRepository.update(sessionId, { 
      isPinned,
      updatedAt: new Date()
    });

    if (result.affected > 0) {
      this.logger.log(`${isPinned ? 'Pinned' : 'Unpinned'} session ${sessionId}`);
      return true;
    }
    return false;
  }

  /**
   * Auto-generate session title based on first message
   */
  async autoGenerateTitle(sessionId: string): Promise<string | null> {
    const messages = await this.messageRepository.find({
      where: { sessionId, role: MessageRole.USER },
      order: { createdAt: 'ASC' },
      take: 1
    });

    if (messages.length === 0) return null;

    const firstMessage = messages[0].content;
    
    // Simple title generation based on keywords
    let title = 'Ny samtale';
    
    // Extract key phrases (simple implementation)
    const keywords = firstMessage.toLowerCase().match(/\b\w{3,}\b/g) || [];
    if (keywords.length > 0) {
      // Take first few meaningful words
      const meaningfulWords = keywords
        .filter(word => !['the', 'and', 'that', 'with', 'have', 'this', 'will', 'you', 'from', 'they', 'know', 'want', 'been', 'good', 'much', 'some', 'time', 'very', 'when', 'come', 'here', 'just', 'like', 'long', 'make', 'many', 'over', 'such', 'take', 'than', 'them', 'well', 'were'].includes(word))
        .slice(0, 3);
      
      if (meaningfulWords.length > 0) {
        title = meaningfulWords.map(w => w.charAt(0).toUpperCase() + w.slice(1)).join(' ');
      }
    }

    // Update the session with auto-generated title
    await this.updateTitle(sessionId, title);
    return title;
  }

  /**
   * Try to auto-generate title using AI after sufficient messages
   */
  private async tryAutoGenerateTitle(sessionId: string): Promise<void> {
    const session = await this.getSession(sessionId);
    if (!session) return;

    // Only generate title if:
    // 1. Session still has default title
    // 2. Has enough messages for context (3+ messages)
    const isDefaultTitle = session.title === 'Ny samtale' || session.title.startsWith('Samtale ');
    if (!isDefaultTitle || session.messageCount < 3) {
      return;
    }

    try {
      // Get recent messages for title generation
      const messages = await this.getRecentMessages(sessionId, 10);
      if (messages.length < 2) return;

      // Reverse to get chronological order
      messages.reverse();

      const titleRequest: TitleGenerationRequest = {
        messages,
        userId: session.userId,
        sessionId: session.id
      };

      const titleResponse = await this.aiTitleGeneratorService.generateTitle(titleRequest);
      
      if (titleResponse.title && titleResponse.title !== session.title) {
        await this.updateTitle(sessionId, titleResponse.title);
        this.logger.log(`AI generated title for session ${sessionId}: "${titleResponse.title}" (confidence: ${titleResponse.confidence})`);
      }

    } catch (error) {
      this.logger.warn(`Failed to generate AI title for session ${sessionId}: ${error.message}`);
      // Fallback to simple keyword-based title
      await this.autoGenerateTitle(sessionId);
    }
  }

  /**
   * Manually trigger AI title generation for a session
   */
  async generateAiTitle(sessionId: string): Promise<string | null> {
    const session = await this.getSessionWithMessages(sessionId, 20);
    if (!session || !session.messages || session.messages.length === 0) {
      return null;
    }

    try {
      const titleRequest: TitleGenerationRequest = {
        messages: session.messages,
        userId: session.userId,
        sessionId: session.id
      };

      const titleResponse = await this.aiTitleGeneratorService.generateTitle(titleRequest);
      
      if (titleResponse.title) {
        await this.updateTitle(sessionId, titleResponse.title);
        this.logger.log(`Manually generated AI title for session ${sessionId}: "${titleResponse.title}"`);
        return titleResponse.title;
      }

    } catch (error) {
      this.logger.error(`Manual AI title generation failed for session ${sessionId}: ${error.message}`);
    }

    return null;
  }

  /**
   * Get sessions that need cleanup (older than retention period)
   */
  async getSessionsForCleanup(retentionDays = 180): Promise<UserSession[]> {
    const cutoffDate = new Date();
    cutoffDate.setDate(cutoffDate.getDate() - retentionDays);

    return this.sessionRepository.find({
      where: {
        updatedAt: cutoffDate,
        status: SessionStatus.ARCHIVED
      }
    });
  }

  /**
   * Get total message count for a user across all sessions
   */
  async getUserMessageCount(userId: string): Promise<number> {
    const result = await this.sessionRepository
      .createQueryBuilder('session')
      .select('SUM(session.messageCount)', 'total')
      .where('session.userId = :userId', { userId })
      .getRawOne();

    return parseInt(result?.total || '0');
  }

  /**
   * Export session data for GDPR compliance
   */
  async exportSessionData(sessionId: string): Promise<any> {
    const session = await this.getSessionWithMessages(sessionId);
    if (!session) return null;

    return {
      session: {
        id: session.id,
        title: session.title,
        status: session.status,
        createdAt: session.createdAt,
        updatedAt: session.updatedAt
      },
      messages: session.messages.map(msg => ({
        role: msg.role,
        content: msg.content,
        createdAt: msg.createdAt,
        metadata: msg.metadata
      }))
    };
  }
}
