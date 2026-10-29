import { Controller, Get } from '@nestjs/common';
import { AiOrchestratorClient } from '../ai/ai-orchestrator.client';
import { WebSocketClientService } from '../ai/websocket-client.service';
import { TeamsAudioService } from '../ai/teams-audio.service';

@Controller('health')
export class HealthController {
  constructor(
    private readonly aiOrchestratorClient: AiOrchestratorClient,
    private readonly webSocketClient: WebSocketClientService,
    private readonly teamsAudioService: TeamsAudioService,
  ) {}

  @Get()
  async get() {
    const audioStatus = this.teamsAudioService.getServiceStatus();
    
    return {
      status: 'ok',
      service: 'teams-gateway',
      timestamp: new Date().toISOString(),
      services: {
        aiOrchestrator: {
          connected: await this.aiOrchestratorClient.healthCheck(),
        },
        webSocket: {
          connected: this.webSocketClient.isWebSocketConnected(),
        },
        audio: {
          connected: audioStatus.connected,
          features: audioStatus.availableFeatures,
          activeRealtimeSessions: audioStatus.activeRealtimeSessions,
        },
      },
      features: {
        chatBot: true,
        fileProcessing: true,
        audioTranscription: audioStatus.availableFeatures.speechToText,
        textToSpeech: audioStatus.availableFeatures.textToSpeech,
        translation: audioStatus.availableFeatures.translation,
        realtimeAI: audioStatus.availableFeatures.realtimeAI,
        webSocketConnection: this.webSocketClient.isWebSocketConnected(),
      },
    };
  }

  @Get('audio')
  getAudioHealth() {
    const audioStatus = this.teamsAudioService.getServiceStatus();
    
    return {
      status: audioStatus.connected ? 'connected' : 'disconnected',
      service: 'teams-audio',
      timestamp: new Date().toISOString(),
      webSocket: {
        connected: this.webSocketClient.isWebSocketConnected(),
      },
      features: audioStatus.availableFeatures,
      sessions: {
        active: audioStatus.activeRealtimeSessions,
        pending: audioStatus.pendingAudioResponses,
      },
    };
  }
}
