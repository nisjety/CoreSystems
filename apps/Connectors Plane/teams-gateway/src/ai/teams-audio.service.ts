import { Injectable, Logger } from '@nestjs/common';
import { WebSocketClientService, AudioProcessingRequest, AudioProcessingResponse } from './websocket-client.service';

export interface TeamsAudioMessage {
  audioData: string; // Base64 encoded audio
  format?: string;
  userId: string;
  userName?: string;
  conversationId: string;
  language?: string;
  requestType: 'transcription' | 'synthesis' | 'translation' | 'realtime';
  responseFormat?: 'text' | 'audio' | 'both';
  targetLanguage?: string;
  sourceLanguage?: string;
  voice?: string;
  text?: string; // For synthesis and translation
}

export interface TeamsAudioResponse {
  messageId: string;
  userId: string;
  conversationId: string;
  type: 'transcription' | 'synthesis' | 'translation' | 'realtime';
  success?: boolean; // Added for backward compatibility
  text?: string;
  audioData?: string; // Base64 encoded
  translatedText?: string;
  originalLanguage?: string;
  targetLanguage?: string;
  confidence?: number;
  timestamp: Date;
  error?: string;
}

@Injectable()
export class TeamsAudioService {
  private readonly logger = new Logger(TeamsAudioService.name);

  constructor(private readonly webSocketClient: WebSocketClientService) {
    this.logger.log('🎤 Teams Audio Service initialized');
  }

  /**
   * Check if WebSocket is connected to AI Orchestrator
   */
  isWebSocketConnected(): boolean {
    return this.webSocketClient.isConnected();
  }

  /**
   * Check if service is connected (alias for isWebSocketConnected)
   */
  isConnected(): boolean {
    return this.webSocketClient.isConnected();
  }

  /**
   * Get health status of audio services
   */
  async getAudioServiceHealth(): Promise<any> {
    try {
      return await this.webSocketClient.getHealthStatus();
    } catch (error) {
      this.logger.error('Failed to get audio service health:', error);
      return {
        speech: { available: false },
        realtime: { available: false },
        error: error.message
      };
    }
  }

  /**
   * Process audio transcription request
   */
  async transcribeAudio(message: TeamsAudioMessage): Promise<TeamsAudioResponse> {
    try {
      this.logger.log(`🎤 Transcribing audio for user ${message.userId}`);
      
      const result = await this.webSocketClient.transcribeAudio(
        message.audioData,
        message.language
      );

      return {
        messageId: this.generateMessageId(),
        userId: message.userId,
        conversationId: message.conversationId,
        type: 'transcription',
        success: true,
        text: result.text,
        confidence: result.confidence,
        originalLanguage: message.language,
        timestamp: new Date(),
      };
    } catch (error) {
      this.logger.error('Transcription failed:', error);
      return {
        messageId: this.generateMessageId(),
        userId: message.userId,
        conversationId: message.conversationId,
        type: 'transcription',
        success: false,
        error: error.message,
        timestamp: new Date(),
      };
    }
  }

  /**
   * Process text-to-speech synthesis request
   */
  async synthesizeSpeech(message: TeamsAudioMessage): Promise<TeamsAudioResponse> {
    try {
      this.logger.log(`🔊 Synthesizing speech for user ${message.userId}`);
      
      if (!message.text) {
        throw new Error('Text is required for speech synthesis');
      }

      const result = await this.webSocketClient.synthesizeText(
        message.text,
        message.language || 'en-US'
      );

      return {
        messageId: this.generateMessageId(),
        userId: message.userId,
        conversationId: message.conversationId,
        type: 'synthesis',
        success: true,
        text: message.text,
        audioData: result.audio,
        timestamp: new Date(),
      };
    } catch (error) {
      this.logger.error('Speech synthesis failed:', error);
      return {
        messageId: this.generateMessageId(),
        userId: message.userId,
        conversationId: message.conversationId,
        type: 'synthesis',
        success: false,
        error: error.message,
        timestamp: new Date(),
      };
    }
  }

  /**
   * Process translation request
   */
  async translateText(
    textOrMessage: string | TeamsAudioMessage,
    fromLanguage = 'auto',
    toLanguage = 'en'
  ): Promise<TeamsAudioResponse> {
    try {
      let message: TeamsAudioMessage;
      
      if (typeof textOrMessage === 'string') {
        // Called with text, fromLanguage, toLanguage
        message = {
          text: textOrMessage,
          sourceLanguage: fromLanguage,
          targetLanguage: toLanguage,
          userId: 'system',
          conversationId: 'translation',
          requestType: 'translation',
          audioData: '', // Not needed for translation
        };
      } else {
        // Called with TeamsAudioMessage
        message = textOrMessage;
      }

      this.logger.log(`🌐 Translating text: ${message.sourceLanguage || 'auto'} → ${message.targetLanguage || 'en'}`);
      
      if (!message.text) {
        throw new Error('Text is required for translation');
      }

      const result = await this.webSocketClient.translateText(
        message.text,
        message.sourceLanguage || 'en',
        message.targetLanguage || 'no'
      );

      return {
        messageId: this.generateMessageId(),
        userId: message.userId,
        conversationId: message.conversationId,
        type: 'translation',
        success: true,
        text: message.text,
        translatedText: result.translatedText,
        originalLanguage: message.sourceLanguage,
        targetLanguage: message.targetLanguage,
        timestamp: new Date(),
      };
    } catch (error) {
      this.logger.error('Translation failed:', error);
      return {
        messageId: this.generateMessageId(),
        userId: typeof textOrMessage === 'string' ? 'system' : textOrMessage.userId,
        conversationId: typeof textOrMessage === 'string' ? 'translation' : textOrMessage.conversationId,
        type: 'translation',
        success: false,
        error: error.message,
        timestamp: new Date(),
      };
    }
  }

  /**
   * Synthesize text to speech (direct method for adapters)
   */
  async synthesizeText(text: string, userId: string, language = 'en-US'): Promise<TeamsAudioResponse> {
    const message: TeamsAudioMessage = {
      text,
      userId,
      conversationId: 'synthesis',
      requestType: 'synthesis',
      language,
      audioData: '', // Not needed for synthesis
    };

    return this.synthesizeSpeech(message);
  }

  /**
   * Start a realtime AI session
   */
  async startRealtimeSession(
    userId: string,
    model = 'gpt-4o-realtime-preview'
  ): Promise<{ sessionId: string }> {
    try {
      this.logger.log(`🤖 Starting realtime session for user ${userId}`);
      
      const session = await this.webSocketClient.startRealtimeSession(userId, model);
      
      return { sessionId: session.sessionId };
    } catch (error) {
      this.logger.error('Failed to start realtime session:', error);
      throw error;
    }
  }

  /**
   * Start realtime conversation (alias for startRealtimeSession)
   */
  async startRealtimeConversation(userId: string, model = 'gpt-4o-realtime-preview'): Promise<string | null> {
    try {
      const result = await this.startRealtimeSession(userId, model);
      return result.sessionId;
    } catch (error) {
      this.logger.error('Failed to start realtime conversation:', error);
      return null;
    }
  }

  /**
   * Get active realtime session for user
   */
  getRealtimeSession(userId: string): string | null {
    // This would need session management state - for now return null
    // In a full implementation, we'd track active sessions per user
    this.logger.log(`Getting realtime session for user ${userId} - session management not implemented`);
    return null;
  }

  /**
   * Send message to realtime AI session
   */
  async sendRealtimeMessage(
    sessionId: string,
    message: string
  ): Promise<{ text: string }> {
    try {
      this.logger.log(`🤖 Sending realtime message to session ${sessionId}`);
      
      const result = await this.webSocketClient.sendRealtimeMessage(sessionId, message);
      
      return result;
    } catch (error) {
      this.logger.error('Failed to send realtime message:', error);
      throw error;
    }
  }

  /**
   * End realtime AI session
   */
  async endRealtimeSession(sessionId: string): Promise<void> {
    try {
      this.logger.log(`🤖 Ending realtime session ${sessionId}`);
      
      await this.webSocketClient.endRealtimeSession(sessionId);
    } catch (error) {
      this.logger.error('Failed to end realtime session:', error);
      throw error;
    }
  }

  /**
   * End realtime conversation (alias for endRealtimeSession)
   */
  async endRealtimeConversation(userId: string): Promise<boolean> {
    try {
      // In a full implementation, we'd look up the session ID from the user ID
      this.logger.log(`🤖 Ending realtime conversation for user ${userId}`);
      // For now, just return success since we don't track sessions
      return true;
    } catch (error) {
      this.logger.error('Failed to end realtime conversation:', error);
      return false;
    }
  }

  /**
   * Process audio with AI (comprehensive audio processing)
   */
  async processAudioWithAI(message: TeamsAudioMessage): Promise<TeamsAudioResponse> {
    try {
      this.logger.log(`🎵 Processing audio with AI for user ${message.userId}`);
      
      const request: AudioProcessingRequest = {
        audioData: message.audioData,
        format: message.format || 'wav',
        sampleRate: 16000,
        language: message.language,
        responseFormat: message.responseFormat || 'text',
        userId: message.userId,
        sessionId: message.conversationId,
      };

      const result = await this.webSocketClient.processAudio(request);

      return {
        messageId: this.generateMessageId(),
        userId: message.userId,
        conversationId: message.conversationId,
        type: 'realtime',
        success: true,
        text: result.text,
        audioData: result.audio,
        confidence: result.confidence,
        timestamp: new Date(),
      };
    } catch (error) {
      this.logger.error('AI audio processing failed:', error);
      return {
        messageId: this.generateMessageId(),
        userId: message.userId,
        conversationId: message.conversationId,
        type: 'realtime',
        success: false,
        error: error.message,
        timestamp: new Date(),
      };
    }
  }

  /**
   * Process audio attachment from Teams message
   */
  async processAudioAttachment(
    audioBuffer: Buffer,
    userId: string,
    conversationId: string,
    language?: string
  ): Promise<{ text: string; confidence: number }> {
    try {
      const audioBase64 = audioBuffer.toString('base64');
      
      const result = await this.webSocketClient.transcribeAudio(audioBase64, language);
      
      this.logger.log(`🎤 Transcribed audio: "${result.text}" (confidence: ${result.confidence})`);
      
      return result;
    } catch (error) {
      this.logger.error('Failed to process audio attachment:', error);
      throw error;
    }
  }

  /**
   * Generate audio response for text
   */
  async generateAudioResponse(
    text: string,
    language = 'en-US'
  ): Promise<{ audioData: string }> {
    try {
      const result = await this.webSocketClient.synthesizeText(text, language);
      
      return { audioData: result.audio };
    } catch (error) {
      this.logger.error('Failed to generate audio response:', error);
      throw error;
    }
  }

  /**
   * Helper method to generate unique message IDs
   */
  private generateMessageId(): string {
    return `audio_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
  }

  /**
   * Get supported audio formats
   */
  getSupportedFormats(): string[] {
    return ['wav', 'mp3', 'mp4', 'flac', 'ogg'];
  }

  /**
   * Get supported languages
   */
  getSupportedLanguages(): string[] {
    return [
      'nb-NO', // Norwegian Bokmål
      'en-US', // English (US)
      'sv-SE', // Swedish
      'da-DK', // Danish
      'de-DE', // German
      'fr-FR', // French
      'es-ES', // Spanish
      'is-IS', // Icelandic
    ];
  }

  /**
   * Validate audio message
   */
  validateAudioMessage(message: TeamsAudioMessage): { valid: boolean; errors: string[] } {
    const errors: string[] = [];

    if (!message.audioData && !message.text) {
      errors.push('Either audioData or text is required');
    }

    if (!message.userId) {
      errors.push('userId is required');
    }

    if (!message.conversationId) {
      errors.push('conversationId is required');
    }

    if (message.requestType === 'synthesis' && !message.text) {
      errors.push('text is required for synthesis');
    }

    if (message.requestType === 'translation' && !message.text) {
      errors.push('text is required for translation');
    }

    if (message.requestType === 'transcription' && !message.audioData) {
      errors.push('audioData is required for transcription');
    }

    return {
      valid: errors.length === 0,
      errors,
    };
  }

  /**
   * Get service status
   */
  getServiceStatus() {
    return {
      connected: this.isConnected(),
      activeRealtimeSessions: 0, // Would track in full implementation
      pendingAudioResponses: 0, // Would track in full implementation
      availableFeatures: {
        speechToText: this.isConnected(),
        textToSpeech: this.isConnected(),
        translation: this.isConnected(),
        realtimeAI: this.isConnected(),
      },
    };
  }
}
