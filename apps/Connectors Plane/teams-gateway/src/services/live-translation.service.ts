import { Injectable, Logger, OnModuleInit, OnModuleDestroy } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import * as SocketIOClient from 'socket.io-client';
import { EventEmitter } from 'events';

type Socket = SocketIOClient.Socket;

export interface LiveTranslationSession {
  sessionId: string;
  userId: string;
  conversationId: string;
  sourceLanguage: string;
  targetLanguage: string;
  isActive: boolean;
  startTime: Date;
  engine: 'azure' | 'openai';
}

export interface TranslationResult {
  sessionId: string;
  originalText: string;
  translatedText: string;
  sourceLanguage: string;
  targetLanguage: string;
  confidence: number;
  timestamp: Date;
  engine: string;
}

@Injectable()
export class LiveTranslationService extends EventEmitter implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(LiveTranslationService.name);
  private socket: Socket;
  private activeSessions = new Map<string, LiveTranslationSession>();
  private orchestratorUrl: string;
  private isConnected = false;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;

  constructor(private configService: ConfigService) {
    super();
    this.orchestratorUrl = this.configService.get<string>('orchestrator_service_url', 'http://localhost:3021');
  }

  async onModuleInit() {
    const isEnabled = this.configService.get<string>('ENABLE_LIVE_TRANSLATION', 'false');
    const isEnabledBool = isEnabled === 'true';
    this.logger.log(`🔧 Live Translation config check: ENABLE_LIVE_TRANSLATION=${isEnabled}`);
    if (isEnabledBool) {
      this.logger.log('✅ Live Translation enabled - connecting to orchestrator');
      await this.connectToOrchestrator();
    } else {
      this.logger.log('⚠️ Live Translation is disabled in configuration - skipping connection');
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
      this.logger.error('❌ AI Orchestrator URL not configured');
      return;
    }

    this.logger.log(`🔌 Connecting to AI Orchestrator WebSocket at ${this.orchestratorUrl}`);

    try {
      this.socket = SocketIOClient.connect(`${this.orchestratorUrl}/realtime`, {
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
      this.logger.error(`❌ Socket initialization error: ${error.message}`);
    }
  }

  private async connectToOrchestrator(): Promise<void> {
    this.logger.log('🤖 Initializing connection to AI Orchestrator Live Translation service...');
    this.initializeSocket();
  }

  private setupSocketEventHandlers(): void {
    this.socket.on('connect', () => {
      this.logger.log('✅ Connected to AI Orchestrator Live Translation Gateway');
      this.isConnected = true;
      this.reconnectAttempts = 0;
      this.emit('connected');
    });

    this.socket.on('disconnect', (reason) => {
      this.logger.warn(`⚠️ Disconnected from AI Orchestrator Live Translation Gateway: ${reason}`);
      this.isConnected = false;
      this.emit('disconnected', reason);
    });

    this.socket.on('connect_error', (error) => {
      this.logger.error(`❌ Connection error: ${error.message}`);
      this.reconnectAttempts++;
      
      if (this.reconnectAttempts >= this.maxReconnectAttempts) {
        this.logger.error('🔴 Max reconnection attempts reached. Live translation unavailable.');
        this.emit('connectionFailed');
      }
    });

    this.socket.on('translationResult', (data: TranslationResult) => {
      this.logger.log(`🌐 Translation result received for session ${data.sessionId}`);
      this.emit('translationResult', data);
    });

    this.socket.on('sessionStarted', (data: { sessionId: string; message: string }) => {
      this.logger.log(`🎙️ Live translation session started: ${data.sessionId}`);
      this.emit('sessionStarted', data);
    });

    this.socket.on('sessionEnded', (data: { sessionId: string; message: string }) => {
      this.logger.log(`🛑 Live translation session ended: ${data.sessionId}`);
      this.activeSessions.delete(data.sessionId);
      this.emit('sessionEnded', data);
    });

    this.socket.on('error', (error) => {
      this.logger.error(`❌ Live translation error: ${error.message || error}`);
      // Don't re-emit the error to avoid unhandled error exceptions
      // Instead, log it and handle gracefully
    });

    this.socket.on('session_creation_failed', (data) => {
      this.logger.error(`❌ Session creation failed: ${data.message}`);
      // Handle session creation failure gracefully
    });

    this.socket.on('translation_error', (data) => {
      this.logger.error(`❌ Translation error for session ${data.sessionId}: ${data.message}`);
      // Handle translation errors gracefully
    });
  }

  /**
   * Start a new live translation session
   */
  async startTranslationSession(
    userId: string,
    conversationId: string,
    sourceLanguage?: string,
    targetLanguage?: string,
    engine?: 'azure' | 'openai'
  ): Promise<LiveTranslationSession> {
    if (!this.isConnected) {
      throw new Error('Live translation service is not connected to AI Orchestrator');
    }

    const sessionId = `live-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
    const defaultSource = this.configService.get<string>('LIVE_TRANSLATE_DEFAULT_SOURCE_LANGUAGE', 'nb-NO');
    const defaultTarget = this.configService.get<string>('LIVE_TRANSLATE_DEFAULT_TARGET_LANGUAGE', 'en-US');
    const defaultEngine = this.configService.get<string>('LIVE_TRANSLATE_DEFAULT_ENGINE', 'azure') as 'azure' | 'openai';

    const session: LiveTranslationSession = {
      sessionId,
      userId,
      conversationId,
      sourceLanguage: sourceLanguage || defaultSource,
      targetLanguage: targetLanguage || defaultTarget,
      isActive: true,
      startTime: new Date(),
      engine: engine || defaultEngine
    };

    try {
      // Send session start request to AI Orchestrator using standard format
      this.socket.emit('start_session', {
        sessionId,
        userId,
        conversationId,
        type: 'live_translation',
        source_language: session.sourceLanguage,
        target_language: session.targetLanguage,
        translation_engine: session.engine,
        config: {
          enableDiarization: this.configService.get<boolean>('ENABLE_DIARIZATION', true),
          enableAutoLanguageDetection: this.configService.get<boolean>('ENABLE_AUTO_LANGUAGE_DETECTION', true),
          audioSampleRate: this.configService.get<number>('LIVE_TRANSLATE_AUDIO_SAMPLE_RATE', 16000),
          audioChannels: this.configService.get<number>('LIVE_TRANSLATE_AUDIO_CHANNELS', 1)
        }
      });

      this.activeSessions.set(sessionId, session);
      this.logger.log(`🎙️ Started live translation session ${sessionId} for user ${userId}`);
      
      return session;
    } catch (error) {
      this.logger.error(`❌ Failed to start translation session: ${error.message}`);
      throw error;
    }
  }

  /**
   * End an active translation session
   */
  async endTranslationSession(sessionId: string): Promise<void> {
    if (!this.isConnected) {
      throw new Error('Live translation service is not connected to AI Orchestrator');
    }

    const session = this.activeSessions.get(sessionId);
    if (!session) {
      throw new Error(`Translation session ${sessionId} not found`);
    }

    try {
      this.socket.emit('end_session', { sessionId });
      session.isActive = false;
      this.activeSessions.delete(sessionId);
      this.logger.log(`🛑 Ended live translation session ${sessionId}`);
    } catch (error) {
      this.logger.error(`❌ Failed to end translation session: ${error.message}`);
      throw error;
    }
  }

  /**
   * Send audio data for translation
   */
  async sendAudioData(sessionId: string, audioData: Buffer): Promise<void> {
    if (!this.isConnected) {
      throw new Error('Live translation service is not connected to AI Orchestrator');
    }

    const session = this.activeSessions.get(sessionId);
    if (!session || !session.isActive) {
      throw new Error(`Translation session ${sessionId} is not active`);
    }

    try {
      this.socket.emit('audio_data', {
        sessionId,
        audioData: audioData.toString('base64'),
        timestamp: new Date().toISOString()
      });
    } catch (error) {
      this.logger.error(`❌ Failed to send audio data: ${error.message}`);
      throw error;
    }
  }

  /**
   * Get active session by conversation ID
   */
  getActiveSessionByConversation(conversationId: string): LiveTranslationSession | undefined {
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
  getActiveSessionsForUser(userId: string): LiveTranslationSession[] {
    return Array.from(this.activeSessions.values()).filter(
      session => session.userId === userId && session.isActive
    );
  }

  /**
   * Check if live translation is available
   */
  isAvailable(): boolean {
    return this.isConnected && this.configService.get<boolean>('ENABLE_LIVE_TRANSLATION', false);
  }

  /**
   * Get supported languages
   */
  getSupportedLanguages(): string[] {
    const supportedLangs = this.configService.get<string>('LIVE_TRANSLATE_SUPPORTED_LANGUAGES', 'nb-NO,en-US');
    return supportedLangs.split(',').map(lang => lang.trim());
  }

  /**
   * Change translation languages for an active session
   */
  async changeSessionLanguages(
    sessionId: string, 
    sourceLanguage: string, 
    targetLanguage: string
  ): Promise<void> {
    if (!this.isConnected) {
      throw new Error('Live translation service is not connected to AI Orchestrator');
    }

    const session = this.activeSessions.get(sessionId);
    if (!session || !session.isActive) {
      throw new Error(`Translation session ${sessionId} is not active`);
    }

    try {
      this.socket.emit('change_mode', {
        sessionId,
        sourceLanguage,
        targetLanguage
      });

      session.sourceLanguage = sourceLanguage;
      session.targetLanguage = targetLanguage;
      
      this.logger.log(`🔄 Changed languages for session ${sessionId}: ${sourceLanguage} → ${targetLanguage}`);
    } catch (error) {
      this.logger.error(`❌ Failed to change session languages: ${error.message}`);
      throw error;
    }
  }
}
