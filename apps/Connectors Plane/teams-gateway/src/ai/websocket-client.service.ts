import { Injectable, Logger, OnModuleInit, OnModuleDestroy } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { io, Socket } from 'socket.io-client';

export interface AudioProcessingRequest {
  audioData: string; // Base64 encoded audio
  format?: string;
  sampleRate?: number;
  language?: string;
  responseFormat?: 'text' | 'audio' | 'both';
  userId?: string;
  sessionId?: string;
}

export interface AudioProcessingResponse {
  text?: string;
  audio?: string; // Base64 encoded audio response
  language?: string;
  confidence?: number;
  timestamp: Date;
  sessionId?: string;
}

export interface RealtimeSession {
  sessionId: string;
  userId: string;
  model?: string;
  modalities?: string[];
}

@Injectable()
export class WebSocketClientService implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(WebSocketClientService.name);
  private socket: Socket | null = null;
  private connectionPromise: Promise<void> | null = null;
  private isConnecting = false;
  private reconnectAttempts = 0;
  private readonly maxReconnectAttempts = 5;
  private readonly reconnectDelay = 5000;

  constructor(private readonly configService: ConfigService) {}

  async onModuleInit() {
    await this.connect();
  }

  onModuleDestroy() {
    this.disconnect();
  }

  private async connect(): Promise<void> {
    if (this.isConnecting || this.socket?.connected) {
      return this.connectionPromise || Promise.resolve();
    }

    this.isConnecting = true;

    this.connectionPromise = new Promise((resolve, reject) => {
      try {
        const websocketUrl = this.configService.get<string>('AI_ORCHESTRATOR_WEBSOCKET_URL', 'http://localhost:3021');
        const namespace = this.configService.get<string>('AI_ORCHESTRATOR_WEBSOCKET_NAMESPACE', '/realtime');
        
        this.logger.log(`Connecting to AI Orchestrator WebSocket: ${websocketUrl}${namespace}`);

        this.socket = io(`${websocketUrl}${namespace}`, {
          transports: ['websocket', 'polling'],
          timeout: 10000,
          reconnection: true,
          reconnectionAttempts: this.maxReconnectAttempts,
          reconnectionDelay: this.reconnectDelay,
        });

        this.socket.on('connect', () => {
          this.logger.log('✅ Connected to AI Orchestrator WebSocket');
          this.isConnecting = false;
          this.reconnectAttempts = 0;
          resolve();
        });

        this.socket.on('disconnect', (reason) => {
          this.logger.warn(`❌ Disconnected from AI Orchestrator WebSocket: ${reason}`);
          this.isConnecting = false;
        });

        this.socket.on('connect_error', (error) => {
          this.logger.error(`❌ WebSocket connection error: ${error.message}`);
          this.isConnecting = false;
          this.reconnectAttempts++;
          
          if (this.reconnectAttempts >= this.maxReconnectAttempts) {
            this.logger.error('❌ Max reconnection attempts reached. WebSocket connection failed.');
            reject(new Error(`Failed to connect after ${this.maxReconnectAttempts} attempts: ${error.message}`));
          }
        });

        this.socket.on('error', (error) => {
          this.logger.error('❌ WebSocket error:', error);
        });

        // Set up event listeners for responses
        this.setupEventListeners();

      } catch (error) {
        this.logger.error('❌ Failed to create WebSocket connection:', error);
        this.isConnecting = false;
        reject(error);
      }
    });

    return this.connectionPromise;
  }

  private setupEventListeners() {
    if (!this.socket) return;

    // Audio processing responses
    this.socket.on('audio-response', (data: AudioProcessingResponse) => {
      this.logger.log('🎤 Received audio processing response');
      // Handle audio response here if needed
    });

    // Transcription results
    this.socket.on('transcription_result', (data: any) => {
      this.logger.log(`🎤 Transcription: "${data.text}" (confidence: ${data.confidence})`);
    });

    // Speech synthesis results
    this.socket.on('synthesis_complete', (data: any) => {
      this.logger.log(`🔊 Speech synthesis complete (${data.audioFormat})`);
    });

    // Translation results
    this.socket.on('translation_result', (data: any) => {
      this.logger.log(`🌐 Translation: "${data.translatedText}"`);
    });

    // Realtime session events
    this.socket.on('realtime_session_started', (data: any) => {
      this.logger.log(`🤖 Realtime session started: ${data.sessionId}`);
    });

    this.socket.on('realtime_response', (data: any) => {
      this.logger.log(`🤖 AI Response: "${data.text || data.content}"`);
    });

    this.socket.on('realtime_session_ended', (data: any) => {
      this.logger.log(`🤖 Realtime session ended: ${data.sessionId}`);
    });

    // Health status
    this.socket.on('health_status', (data: any) => {
      this.logger.log(`🏥 Health Status - Speech: ${data.speech?.available}, Realtime: ${data.realtime?.available}`);
    });

    // Error handling
    this.socket.on('error', (data: any) => {
      this.logger.error(`❌ WebSocket Error: ${data.message}`);
    });
  }

  private disconnect() {
    if (this.socket) {
      this.logger.log('Disconnecting from AI Orchestrator WebSocket');
      this.socket.disconnect();
      this.socket = null;
    }
    this.isConnecting = false;
    this.connectionPromise = null;
  }

  async ensureConnection(): Promise<void> {
    if (!this.socket?.connected) {
      await this.connect();
    }
  }

  isConnected(): boolean {
    return this.socket?.connected || false;
  }

  /**
   * Alias for isConnected() for compatibility
   */
  isWebSocketConnected(): boolean {
    return this.isConnected();
  }

  /**
   * Process audio input through the AI Orchestrator
   */
  async processAudio(request: AudioProcessingRequest): Promise<AudioProcessingResponse> {
    await this.ensureConnection();
    
    if (!this.socket) {
      throw new Error('WebSocket not connected');
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('Audio processing timeout'));
      }, 30310);

      // Listen for the response
      this.socket.once('audio-response', (response: AudioProcessingResponse) => {
        clearTimeout(timeout);
        resolve(response);
      });

      this.socket.once('error', (error) => {
        clearTimeout(timeout);
        reject(new Error(error.message));
      });

      // Send the request
      this.socket.emit('audio-input', request);
    });
  }

  /**
   * Transcribe audio to text
   */
  async transcribeAudio(audioData: string, language?: string): Promise<{ text: string; confidence: number }> {
    await this.ensureConnection();
    
    if (!this.socket) {
      throw new Error('WebSocket not connected');
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('Transcription timeout'));
      }, 30310);

      this.socket.once('transcription_result', (result: any) => {
        clearTimeout(timeout);
        resolve({
          text: result.text,
          confidence: result.confidence || 0.9
        });
      });

      this.socket.once('error', (error) => {
        clearTimeout(timeout);
        reject(new Error(error.message));
      });

      this.socket.emit('process_audio', {
        audioData,
        format: 'wav',
        sampleRate: 16000,
        language
      });
    });
  }

  /**
   * Synthesize text to speech
   */
  async synthesizeText(text: string, language = 'en-US'): Promise<{ audio: string }> {
    await this.ensureConnection();
    
    if (!this.socket) {
      throw new Error('WebSocket not connected');
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('Speech synthesis timeout'));
      }, 30310);

      this.socket.once('synthesis_complete', (result: any) => {
        clearTimeout(timeout);
        resolve({
          audio: result.audioData
        });
      });

      this.socket.once('error', (error) => {
        clearTimeout(timeout);
        reject(new Error(error.message));
      });

      this.socket.emit('synthesize_speech', {
        text,
        language,
        voice: 'neural',
        format: 'mp3'
      });
    });
  }

  /**
   * Translate text
   */
  async translateText(text: string, fromLanguage = 'en', toLanguage = 'no'): Promise<{ translatedText: string }> {
    await this.ensureConnection();
    
    if (!this.socket) {
      throw new Error('WebSocket not connected');
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('Translation timeout'));
      }, 30310);

      this.socket.once('translation_result', (result: any) => {
        clearTimeout(timeout);
        resolve({
          translatedText: result.translatedText
        });
      });

      this.socket.once('error', (error) => {
        clearTimeout(timeout);
        reject(new Error(error.message));
      });

      this.socket.emit('translate_text', {
        text,
        fromLanguage,
        toLanguage
      });
    });
  }

  /**
   * Start a realtime AI session
   */
  async startRealtimeSession(userId: string, model = 'gpt-4o-realtime-preview'): Promise<RealtimeSession> {
    await this.ensureConnection();
    
    if (!this.socket) {
      throw new Error('WebSocket not connected');
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('Realtime session start timeout'));
      }, 30310);

      this.socket.once('realtime_session_started', (result: any) => {
        clearTimeout(timeout);
        resolve({
          sessionId: result.sessionId,
          userId,
          model,
          modalities: result.modalities
        });
      });

      this.socket.once('error', (error) => {
        clearTimeout(timeout);
        reject(new Error(error.message));
      });

      this.socket.emit('start_realtime_session', {
        userId,
        model,
        modalities: ['text', 'audio']
      });
    });
  }

  /**
   * Send message to realtime AI session
   */
  async sendRealtimeMessage(sessionId: string, content: string): Promise<{ text: string }> {
    await this.ensureConnection();
    
    if (!this.socket) {
      throw new Error('WebSocket not connected');
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('Realtime message timeout'));
      }, 30310);

      this.socket.once('realtime_response', (result: any) => {
        clearTimeout(timeout);
        resolve({
          text: result.text || result.content
        });
      });

      this.socket.once('error', (error) => {
        clearTimeout(timeout);
        reject(new Error(error.message));
      });

      this.socket.emit('send_realtime_message', {
        sessionId,
        type: 'text',
        content
      });
    });
  }

  /**
   * End realtime AI session
   */
  async endRealtimeSession(sessionId: string): Promise<void> {
    await this.ensureConnection();
    
    if (!this.socket) {
      throw new Error('WebSocket not connected');
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('End session timeout'));
      }, 10000);

      this.socket.once('realtime_session_ended', () => {
        clearTimeout(timeout);
        resolve();
      });

      this.socket.once('error', (error) => {
        clearTimeout(timeout);
        reject(new Error(error.message));
      });

      this.socket.emit('end_realtime_session', {
        sessionId
      });
    });
  }

  /**
   * Get health status of AI services
   */
  async getHealthStatus(): Promise<any> {
    await this.ensureConnection();
    
    if (!this.socket) {
      throw new Error('WebSocket not connected');
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('Health check timeout'));
      }, 10000);

      this.socket.once('health_status', (result: any) => {
        clearTimeout(timeout);
        resolve(result);
      });

      this.socket.once('error', (error) => {
        clearTimeout(timeout);
        reject(new Error(error.message));
      });

      this.socket.emit('get_health_status');
    });
  }
}
