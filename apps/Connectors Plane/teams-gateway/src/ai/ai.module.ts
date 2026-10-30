import { Module } from '@nestjs/common';
import { AiOrchestratorClient } from './ai-orchestrator.client';
import { WebSocketClientService } from './websocket-client.service';
import { TeamsAudioService } from './teams-audio.service';

@Module({
  providers: [
    AiOrchestratorClient,
    WebSocketClientService,
    TeamsAudioService,
  ],
  exports: [
    AiOrchestratorClient,
    WebSocketClientService,
    TeamsAudioService,
  ],
})
export class AiModule {}
