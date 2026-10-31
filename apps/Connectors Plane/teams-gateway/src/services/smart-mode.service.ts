import { Injectable, Logger, OnModuleInit, OnModuleDestroy } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import io from 'socket.io-client';
import { EventEmitter } from 'events';

export interface SmartModeSession {
  sessionId: string;
  userId: string;
  conversationId: string;
  isActive: boolean;
  startTime: Date;
  features: {
    meetingAssistance: boolean;
    realTimeChat: boolean;
    contextAware: boolean;
  };
}

export interface SmartModeResponse {
  sessionId: string;
  responseType: 'text' | 'audio' | 'action';
  content: string;
  confidence: number;
  timestamp: Date;
  metadata?: {
    intent?: string;
    entities?: any[];
    sentiment?: string;
  };
}

@Injectable()
export class SmartModeService extends EventEmitter implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(SmartModeService.name);
  private socket: any;
  private activeSessions = new Map<string, SmartModeSession>();
  private orchestratorUrl: string;
  private isConnected = false;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;

  constructor(private configService: ConfigService) {
    super();
    this.orchestratorUrl = this.configService.get<string>('orchestrator_service_url', 'http://localhost:3021');
  }

  async onModuleInit() {
    const isEnabled = this.configService.get<string>('ENABLE_SMART_MODE', 'false');
    const isEnabledBool = isEnabled === 'true';
    this.logger.log(`🔧 Smart Mode config check: ENABLE_SMART_MODE=${isEnabled}`);
    if (isEnabledBool) {
      this.logger.log('✅ Smart Mode enabled - connecting to orchestrator');
      await this.connectToOrchestrator();
    } else {
      this.logger.log('⚠️ Smart Mode is disabled in configuration - skipping connection');
      return; // Explicit return to prevent any further execution
    }
  }

  async onModuleDestroy() {
    if (this.socket) {
      this.socket.disconnect();
    }
  }

  private initializeSocket(): void {
    if (!this.orchestratorUrl) {
      this.logger.error('❌ AI Orchestrator URL not configured for Smart Mode');
      return;
    }

    this.logger.log(`🤖 Connecting to AI Orchestrator Smart Mode at ${this.orchestratorUrl}`);

    try {
      this.socket = io(`${this.orchestratorUrl}/realtime`, {
        transports: ['polling', 'websocket'], // Start with polling for Azure Container Apps
        timeout: 30310,
        forceNew: true,
        upgrade: true,
        rememberUpgrade: false,
        reconnection: true,
        reconnectionAttempts: 5,
        reconnectionDelay: 3031,
        autoConnect: true,
        // Force polling for initial connection stability
        forceBase64: false,
      });

      this.setupSocketEventHandlers();
    } catch (error) {
      this.logger.error(`❌ Smart Mode socket initialization error: ${error.message}`);
    }
  }

  private async connectToOrchestrator(): Promise<void> {
    this.logger.log('🤖 Initializing connection to AI Orchestrator Smart Mode service...');
    this.initializeSocket();
  }

  private setupSocketEventHandlers(): void {
    this.socket.on('connect', () => {
      this.logger.log('✅ Connected to AI Orchestrator Smart Mode Gateway');
      this.isConnected = true;
      this.reconnectAttempts = 0;
      this.emit('connected');
    });

    this.socket.on('disconnect', (reason) => {
      this.logger.warn(`⚠️ Disconnected from AI Orchestrator Smart Mode Gateway: ${reason}`);
      this.isConnected = false;
      this.emit('disconnected', reason);
    });

    this.socket.on('connect_error', (error) => {
      this.logger.error(`❌ Smart Mode connection error: ${error.message}`);
      this.reconnectAttempts++;
      
      if (this.reconnectAttempts >= this.maxReconnectAttempts) {
        this.logger.error('🔴 Max reconnection attempts reached. Smart Mode unavailable.');
        this.emit('connectionFailed');
      }
    });

    this.socket.on('translationResult', (data: any) => {
      this.logger.log(`🧠 Smart Mode translation received for session ${data.sessionId}`);
      this.emit('smartResponse', {
        sessionId: data.sessionId,
        responseType: 'text',
        content: data.translatedText,
        confidence: data.confidence,
        timestamp: new Date(),
        metadata: {
          originalText: data.originalText,
          sourceLanguage: data.sourceLanguage,
          targetLanguage: data.targetLanguage
        }
      });
    });

    this.socket.on('sessionStarted', (data: { sessionId: string; message: string }) => {
      this.logger.log(`🚀 Smart Mode session started: ${data.sessionId}`);
      this.emit('sessionStarted', data);
    });

    this.socket.on('sessionEnded', (data: { sessionId: string; message: string }) => {
      this.logger.log(`🛑 Smart Mode session ended: ${data.sessionId}`);
      this.activeSessions.delete(data.sessionId);
      this.emit('sessionEnded', data);
    });

    this.socket.on('smartResponse', (data: any) => {
      this.logger.log(`💡 Smart insight received: ${data.insight || data.response}`);
      this.emit('meetingInsight', data);
    });

    this.socket.on('error', (error) => {
      this.logger.error(`❌ Smart Mode error: ${error.message || error}`);
      // Don't re-emit the error to avoid unhandled error exceptions
      // Instead, log it and handle gracefully
    });

    this.socket.on('session_creation_failed', (data) => {
      this.logger.error(`❌ Smart Mode session creation failed: ${data.message}`);
      // Handle session creation failure gracefully
    });

    this.socket.on('smart_error', (data) => {
      this.logger.error(`❌ Smart Mode error for session ${data.sessionId}: ${data.message}`);
      // Handle smart mode errors gracefully
    });
  }

  /**
   * Start a new smart mode session
   */
  async startSmartModeSession(
    userId: string,
    conversationId: string,
    features?: {
      meetingAssistance?: boolean;
      realTimeChat?: boolean;
      contextAware?: boolean;
    }
  ): Promise<SmartModeSession> {
    if (!this.isConnected) {
      throw new Error('Smart Mode service is not connected to AI Orchestrator');
    }

    const sessionId = `smart-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
    
    const session: SmartModeSession = {
      sessionId,
      userId,
      conversationId,
      isActive: true,
      startTime: new Date(),
      features: {
        meetingAssistance: features?.meetingAssistance ?? true,
        realTimeChat: features?.realTimeChat ?? true,
        contextAware: features?.contextAware ?? true
      }
    };

    try {
      // Send session start request to AI Orchestrator using standard format
      this.socket.emit('start_session', {
        sessionId,
        userId,
        conversationId,
        type: 'smart_mode',
        source_language: 'nb-NO',
        target_language: 'en-US',
        features: session.features,
        config: {
          model: this.configService.get<string>('AZURE_OPENAI_DEPLOYMENT_REALTIME', 'gpt-4o-mini-realtime-preview'),
          apiVersion: this.configService.get<string>('openai_realtime_api_version', '2024-12-17'),
          language: 'nb-NO',
          enableContextAwareness: true,
          enableMeetingAssistance: session.features.meetingAssistance
        }
      });

      this.activeSessions.set(sessionId, session);
      this.logger.log(`🚀 Started Smart Mode session ${sessionId} for user ${userId}`);
      
      return session;
    } catch (error) {
      this.logger.error(`❌ Failed to start Smart Mode session: ${error.message}`);
      throw error;
    }
  }

  /**
   * End an active smart mode session
   */
  async endSmartModeSession(sessionId: string): Promise<void> {
    if (!this.isConnected) {
      throw new Error('Smart Mode service is not connected to AI Orchestrator');
    }

    const session = this.activeSessions.get(sessionId);
    if (!session) {
      throw new Error(`Smart Mode session ${sessionId} not found`);
    }

    try {
      this.socket.emit('end_session', { sessionId });
      session.isActive = false;
      this.activeSessions.delete(sessionId);
      this.logger.log(`🛑 Ended Smart Mode session ${sessionId}`);
    } catch (error) {
      this.logger.error(`❌ Failed to end Smart Mode session: ${error.message}`);
      throw error;
    }
  }

  /**
   * Send real-time message to Smart Mode
   */
  async sendRealtimeMessage(sessionId: string, message: string, context?: any): Promise<void> {
    if (!this.isConnected) {
      throw new Error('Smart Mode service is not connected to AI Orchestrator');
    }

    const session = this.activeSessions.get(sessionId);
    if (!session || !session.isActive) {
      throw new Error(`Smart Mode session ${sessionId} is not active`);
    }

    try {
      this.socket.emit('smart_request', {
        sessionId,
        type: 'message',
        message,
        context,
        timestamp: new Date().toISOString()
      });
      
      this.logger.log(`📨 Sent real-time message to session ${sessionId}`);
    } catch (error) {
      this.logger.error(`❌ Failed to send real-time message: ${error.message}`);
      throw error;
    }
  }

  /**
   * Send audio data for real-time processing
   */
  async sendRealtimeAudio(sessionId: string, audioData: Buffer): Promise<void> {
    if (!this.isConnected) {
      throw new Error('Smart Mode service is not connected to AI Orchestrator');
    }

    const session = this.activeSessions.get(sessionId);
    if (!session || !session.isActive) {
      throw new Error(`Smart Mode session ${sessionId} is not active`);
    }

    try {
      this.socket.emit('audio_data', {
        sessionId,
        audioData: audioData.toString('base64'),
        timestamp: new Date().toISOString()
      });
    } catch (error) {
      this.logger.error(`❌ Failed to send real-time audio: ${error.message}`);
      throw error;
    }
  }

  /**
   * Request meeting assistance
   */
  async requestMeetingAssistance(
    sessionId: string,
    assistanceType: 'summary' | 'action_items' | 'next_steps' | 'insights',
    context?: any
  ): Promise<void> {
    if (!this.isConnected) {
      throw new Error('Smart Mode service is not connected to AI Orchestrator');
    }

    const session = this.activeSessions.get(sessionId);
    if (!session || !session.isActive) {
      throw new Error(`Smart Mode session ${sessionId} is not active`);
    }

    if (!session.features.meetingAssistance) {
      throw new Error('Meeting assistance is not enabled for this session');
    }

    try {
      this.socket.emit('smart_request', {
        sessionId,
        type: assistanceType,
        context,
        timestamp: new Date().toISOString()
      });
      
      this.logger.log(`🤝 Requested meeting assistance (${assistanceType}) for session ${sessionId}`);
    } catch (error) {
      this.logger.error(`❌ Failed to request meeting assistance: ${error.message}`);
      throw error;
    }
  }

  /**
   * Get active session by conversation ID
   */
  getActiveSessionByConversation(conversationId: string): SmartModeSession | undefined {
    for (const session of this.activeSessions.values()) {
      if (session.conversationId === conversationId && session.isActive) {
        return session;
      }
    }
    return undefined;
  }

  /**
   * Get all active sessions for a user
   */
  getActiveSessionsForUser(userId: string): SmartModeSession[] {
    return Array.from(this.activeSessions.values()).filter(
      session => session.userId === userId && session.isActive
    );
  }

  /**
   * Check if Smart Mode is available
   */
  isAvailable(): boolean {
    return this.isConnected && this.isRealtimeApiConfigured();
  }

  /**
   * Check if OpenAI Realtime API is configured
   */
  private isRealtimeApiConfigured(): boolean {
    const endpoint = this.configService.get<string>('AZURE_OPENAI_ENDPOINT_REALTIME');
    const apiKey = this.configService.get<string>('AZURE_OPENAI_API_KEY_REALTIME');
    const deployment = this.configService.get<string>('AZURE_OPENAI_DEPLOYMENT_REALTIME');
    
    return !!(endpoint && apiKey && deployment);
  }

  /**
   * Update session features
   */
  async updateSessionFeatures(
    sessionId: string,
    features: {
      meetingAssistance?: boolean;
      realTimeChat?: boolean;
      contextAware?: boolean;
    }
  ): Promise<void> {
    if (!this.isConnected) {
      throw new Error('Smart Mode service is not connected to AI Orchestrator');
    }

    const session = this.activeSessions.get(sessionId);
    if (!session || !session.isActive) {
      throw new Error(`Smart Mode session ${sessionId} is not active`);
    }

    try {
      this.socket.emit('change_mode', {
        sessionId,
        features
      });

      // Update local session
      Object.assign(session.features, features);
      
      this.logger.log(`🔧 Updated features for session ${sessionId}`);
    } catch (error) {
      this.logger.error(`❌ Failed to update session features: ${error.message}`);
      throw error;
    }
  }
}
