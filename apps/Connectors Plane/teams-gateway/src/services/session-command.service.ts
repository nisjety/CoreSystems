import { Injectable, Logger } from '@nestjs/common';
import { SessionService, SessionSummary } from './session.service';
import { UserStateService } from './user-state.service';
import { MessageRole } from '../entities/session-message.entity';

export interface CommandResult {
  success: boolean;
  message: string;
  data?: any;
  requiresResponse?: boolean;
}

export interface ThreadSwitcherData {
  sessions: SessionSummary[];
  activeSessionId: string | null;
  totalCount: number;
}

@Injectable()
export class SessionCommandService {
  private readonly logger = new Logger(SessionCommandService.name);

  constructor(
    private sessionService: SessionService,
    private userStateService: UserStateService,
  ) {}

  /**
   * Handle /ny command - create new session
   */
  async handleNewSessionCommand(userId: string, title?: string): Promise<CommandResult> {
    try {
      const session = await this.sessionService.createSession({
        userId,
        title: title || 'Ny samtale'
      });

      // Set as active session
      await this.userStateService.setActiveSession(userId, session.id);

      return {
        success: true,
        message: `✅ Opprettet ny samtale: "${session.title}"`,
        data: { sessionId: session.id, title: session.title },
        requiresResponse: false
      };
    } catch (error) {
      this.logger.error(`Failed to create new session for user ${userId}:`, error);
      return {
        success: false,
        message: '❌ Kunne ikke opprette ny samtale. Prøv igjen.',
        requiresResponse: false
      };
    }
  }

  /**
   * Handle /threads command - show thread switcher
   */
  async handleThreadsCommand(userId: string): Promise<CommandResult> {
    try {
      const sessions = await this.sessionService.getActiveSessions(userId, 20);
      const activeSessionId = await this.userStateService.getActiveSessionId(userId);

      // Update last switcher shown timestamp
      await this.userStateService.updateSwitcherShown(userId);

      const data: ThreadSwitcherData = {
        sessions,
        activeSessionId,
        totalCount: sessions.length
      };

      return {
        success: true,
        message: sessions.length > 0 
          ? `📋 Viser ${sessions.length} samtaler:`
          : '📭 Ingen aktive samtaler funnet.',
        data,
        requiresResponse: true // Will show Adaptive Card
      };
    } catch (error) {
      this.logger.error(`Failed to get threads for user ${userId}:`, error);
      return {
        success: false,
        message: '❌ Kunne ikke hente samtaler. Prøv igjen.',
        requiresResponse: false
      };
    }
  }

  /**
   * Handle /bytt command - switch active session
   */
  async handleSwitchSessionCommand(userId: string, sessionId: string): Promise<CommandResult> {
    try {
      const session = await this.sessionService.getSession(sessionId);
      
      if (!session) {
        return {
          success: false,
          message: '❌ Fant ikke samtalen. Kontroller ID-en.',
          requiresResponse: false
        };
      }

      if (session.userId !== userId) {
        return {
          success: false,
          message: '❌ Du har ikke tilgang til denne samtalen.',
          requiresResponse: false
        };
      }

      const currentActiveId = await this.userStateService.getActiveSessionId(userId);
      await this.userStateService.trackSessionSwitch(userId, currentActiveId, sessionId);

      return {
        success: true,
        message: `🔄 Byttet til samtale: "${session.title}"`,
        data: { 
          sessionId: session.id, 
          title: session.title,
          previousSessionId: currentActiveId 
        },
        requiresResponse: false
      };
    } catch (error) {
      this.logger.error(`Failed to switch session for user ${userId}:`, error);
      return {
        success: false,
        message: '❌ Kunne ikke bytte samtale. Prøv igjen.',
        requiresResponse: false
      };
    }
  }

  /**
   * Handle /endre command - rename session
   */
  async handleRenameSessionCommand(userId: string, sessionId: string, newTitle: string): Promise<CommandResult> {
    try {
      const session = await this.sessionService.getSession(sessionId);
      
      if (!session) {
        return {
          success: false,
          message: '❌ Fant ikke samtalen.',
          requiresResponse: false
        };
      }

      if (session.userId !== userId) {
        return {
          success: false,
          message: '❌ Du har ikke tilgang til denne samtalen.',
          requiresResponse: false
        };
      }

      const oldTitle = session.title;
      const success = await this.sessionService.updateTitle(sessionId, newTitle);

      if (success) {
        return {
          success: true,
          message: `✏️ Endret navn fra "${oldTitle}" til "${newTitle}"`,
          data: { sessionId, oldTitle, newTitle },
          requiresResponse: false
        };
      } else {
        return {
          success: false,
          message: '❌ Kunne ikke endre navn på samtalen.',
          requiresResponse: false
        };
      }
    } catch (error) {
      this.logger.error(`Failed to rename session for user ${userId}:`, error);
      return {
        success: false,
        message: '❌ Kunne ikke endre navn. Prøv igjen.',
        requiresResponse: false
      };
    }
  }

  /**
   * Handle /arkiver command - archive session
   */
  async handleArchiveSessionCommand(userId: string, sessionId: string): Promise<CommandResult> {
    try {
      const session = await this.sessionService.getSession(sessionId);
      
      if (!session) {
        return {
          success: false,
          message: '❌ Fant ikke samtalen.',
          requiresResponse: false
        };
      }

      if (session.userId !== userId) {
        return {
          success: false,
          message: '❌ Du har ikke tilgang til denne samtalen.',
          requiresResponse: false
        };
      }

      const success = await this.sessionService.archiveSession(sessionId);

      if (success) {
        // Clear active session if this was the active one
        const activeSessionId = await this.userStateService.getActiveSessionId(userId);
        if (activeSessionId === sessionId) {
          await this.userStateService.clearActiveSession(userId);
        }

        return {
          success: true,
          message: `📦 Arkiverte samtale: "${session.title}"`,
          data: { sessionId, title: session.title },
          requiresResponse: false
        };
      } else {
        return {
          success: false,
          message: '❌ Kunne ikke arkivere samtalen.',
          requiresResponse: false
        };
      }
    } catch (error) {
      this.logger.error(`Failed to archive session for user ${userId}:`, error);
      return {
        success: false,
        message: '❌ Kunne ikke arkivere samtale. Prøv igjen.',
        requiresResponse: false
      };
    }
  }

  /**
   * Handle /slett command - delete session
   */
  async handleDeleteSessionCommand(userId: string, sessionId: string): Promise<CommandResult> {
    try {
      const session = await this.sessionService.getSession(sessionId);
      
      if (!session) {
        return {
          success: false,
          message: '❌ Fant ikke samtalen.',
          requiresResponse: false
        };
      }

      if (session.userId !== userId) {
        return {
          success: false,
          message: '❌ Du har ikke tilgang til denne samtalen.',
          requiresResponse: false
        };
      }

      const title = session.title;
      const success = await this.sessionService.deleteSession(sessionId);

      if (success) {
        // Clear active session if this was the active one
        const activeSessionId = await this.userStateService.getActiveSessionId(userId);
        if (activeSessionId === sessionId) {
          await this.userStateService.clearActiveSession(userId);
        }

        return {
          success: true,
          message: `🗑️ Slettet samtale: "${title}"`,
          data: { sessionId, title },
          requiresResponse: false
        };
      } else {
        return {
          success: false,
          message: '❌ Kunne ikke slette samtalen.',
          requiresResponse: false
        };
      }
    } catch (error) {
      this.logger.error(`Failed to delete session for user ${userId}:`, error);
      return {
        success: false,
        message: '❌ Kunne ikke slette samtale. Prøv igjen.',
        requiresResponse: false
      };
    }
  }

  /**
   * Handle /ai-tittel command - generate AI title for session
   */
  async handleGenerateAiTitleCommand(userId: string, sessionId: string): Promise<CommandResult> {
    try {
      const session = await this.sessionService.getSession(sessionId);
      
      if (!session) {
        return {
          success: false,
          message: '❌ Fant ikke samtalen.',
          requiresResponse: false
        };
      }

      if (session.userId !== userId) {
        return {
          success: false,
          message: '❌ Du har ikke tilgang til denne samtalen.',
          requiresResponse: false
        };
      }

      // Generate AI title
      const newTitle = await this.sessionService.generateAiTitle(sessionId);

      if (newTitle) {
        return {
          success: true,
          message: `🤖 AI genererte ny tittel: "${newTitle}"`,
          data: { sessionId, oldTitle: session.title, newTitle },
          requiresResponse: false
        };
      } else {
        return {
          success: false,
          message: '❌ Kunne ikke generere AI-tittel. Prøv igjen senere.',
          requiresResponse: false
        };
      }
    } catch (error) {
      this.logger.error(`Failed to generate AI title for session ${sessionId}:`, error);
      return {
        success: false,
        message: '❌ Kunne ikke generere AI-tittel. Prøv igjen.',
        requiresResponse: false
      };
    }
  }

  /**
   * Handle /pin command - pin/unpin session
   */
  async handlePinSessionCommand(userId: string, sessionId: string, pin = true): Promise<CommandResult> {
    try {
      const session = await this.sessionService.getSession(sessionId);
      
      if (!session) {
        return {
          success: false,
          message: '❌ Fant ikke samtalen.',
          requiresResponse: false
        };
      }

      if (session.userId !== userId) {
        return {
          success: false,
          message: '❌ Du har ikke tilgang til denne samtalen.',
          requiresResponse: false
        };
      }

      const success = await this.sessionService.togglePin(sessionId, pin);

      if (success) {
        return {
          success: true,
          message: pin 
            ? `📌 Festet samtale: "${session.title}"`
            : `📌 Løsnet samtale: "${session.title}"`,
          data: { sessionId, title: session.title, isPinned: pin },
          requiresResponse: false
        };
      } else {
        return {
          success: false,
          message: `❌ Kunne ikke ${pin ? 'feste' : 'løsne'} samtalen.`,
          requiresResponse: false
        };
      }
    } catch (error) {
      this.logger.error(`Failed to pin session for user ${userId}:`, error);
      return {
        success: false,
        message: '❌ Kunne ikke endre pin-status. Prøv igjen.',
        requiresResponse: false
      };
    }
  }

  /**
   * Parse and route commands
   */
  async handleCommand(userId: string, message: string): Promise<CommandResult | null> {
    const trimmed = message.trim().toLowerCase();
    
    // Extract command and parameters
    const parts = trimmed.split(/\s+/);
    const command = parts[0];
    
    switch (command) {
      case '/ny':
      case '/new': {
        const title = parts.slice(1).join(' ') || undefined;
        return this.handleNewSessionCommand(userId, title);
      }
        
      case '/threads':
      case '/samtaler':
        return this.handleThreadsCommand(userId);
        
      case '/bytt':
      case '/switch':
        if (parts.length < 2) {
          return {
            success: false,
            message: '❌ Mangler samtale-ID. Bruk: /bytt <session-id>',
            requiresResponse: false
          };
        }
        return this.handleSwitchSessionCommand(userId, parts[1]);
        
      case '/endre':
      case '/rename': {
        if (parts.length < 3) {
          return {
            success: false,
            message: '❌ Mangler parametere. Bruk: /endre <session-id> <nytt navn>',
            requiresResponse: false
          };
        }
        const newTitle = parts.slice(2).join(' ');
        return this.handleRenameSessionCommand(userId, parts[1], newTitle);
      }
        
      case '/arkiver':
      case '/archive':
        if (parts.length < 2) {
          return {
            success: false,
            message: '❌ Mangler samtale-ID. Bruk: /arkiver <session-id>',
            requiresResponse: false
          };
        }
        return this.handleArchiveSessionCommand(userId, parts[1]);
        
      case '/slett':
      case '/delete':
        if (parts.length < 2) {
          return {
            success: false,
            message: '❌ Mangler samtale-ID. Bruk: /slett <session-id>',
            requiresResponse: false
          };
        }
        return this.handleDeleteSessionCommand(userId, parts[1]);
        
      case '/pin':
        if (parts.length < 2) {
          return {
            success: false,
            message: '❌ Mangler samtale-ID. Bruk: /pin <session-id>',
            requiresResponse: false
          };
        }
        return this.handlePinSessionCommand(userId, parts[1], true);
        
      case '/unpin':
        if (parts.length < 2) {
          return {
            success: false,
            message: '❌ Mangler samtale-ID. Bruk: /unpin <session-id>',
            requiresResponse: false
          };
        }
        return this.handlePinSessionCommand(userId, parts[1], false);
        
      case '/ai-tittel':
      case '/ai-title':
        if (parts.length < 2) {
          return {
            success: false,
            message: '❌ Mangler samtale-ID. Bruk: /ai-tittel <session-id>',
            requiresResponse: false
          };
        }
        return this.handleGenerateAiTitleCommand(userId, parts[1]);
        
      default:
        return null; // Not a recognized command
    }
  }

  /**
   * Add message to current session (auto-create if needed)
   */
  async addMessageToCurrentSession(
    userId: string, 
    content: string, 
    role: MessageRole = MessageRole.USER
  ): Promise<string> {
    let sessionId = await this.userStateService.getActiveSessionId(userId);
    
    if (!sessionId) {
      // Auto-create session if none exists
      const session = await this.sessionService.createSession({
        userId,
        title: 'Ny samtale'
      });
      sessionId = session.id;
      await this.userStateService.setActiveSession(userId, sessionId);
      
      // Auto-generate title if enabled
      const autoNamingEnabled = await this.userStateService.isAutoNamingEnabled(userId);
      if (autoNamingEnabled && role === MessageRole.USER) {
        setTimeout(async () => {
          await this.sessionService.autoGenerateTitle(sessionId);
        }, 1000); // Delay to allow message to be saved first
      }
    }
    
    await this.sessionService.addMessage(sessionId, role, content);
    return sessionId;
  }

  /**
   * Get help text for commands
   */
  getHelpText(): string {
    return `
📋 **Tilgjengelige kommandoer:**

**Samtalestyring:**
• \`/ny [navn]\` - Opprett ny samtale
• \`/threads\` - Vis alle samtaler
• \`/bytt <id>\` - Bytt til samtale

**Redigering:**
• \`/endre <id> <nytt navn>\` - Endre navn på samtale
• \`/ai-tittel <id>\` - Generer AI-tittel basert på innhold
• \`/pin <id>\` - Fest samtale øverst
• \`/unpin <id>\` - Løsne festet samtale

**Arkivering:**
• \`/arkiver <id>\` - Arkiver samtale
• \`/slett <id>\` - Slett samtale permanent

💡 **Tips:** Bruk \`/threads\` for å se ID-er til samtalene dine.
🤖 **AI-tittel:** Kommandoen analyserer samtaleinnholdet og lager en smart tittel.
    `.trim();
  }
}
