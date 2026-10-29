import { Controller, Get, Post, Body, Param, Logger } from '@nestjs/common';
import { LiveTranslationService, LiveTranslationSession } from '../services/live-translation.service';
import { SmartModeService, SmartModeSession } from '../services/smart-mode.service';

@Controller('api/realtime')
export class RealtimeGatewayController {
  private readonly logger = new Logger(RealtimeGatewayController.name);

  constructor(
    private readonly liveTranslationService: LiveTranslationService,
    private readonly smartModeService: SmartModeService,
  ) {}

  @Get('status')
  async getRealtimeStatus() {
    return {
      liveTranslation: {
        available: this.liveTranslationService.isAvailable(),
        supportedLanguages: this.liveTranslationService.getSupportedLanguages(),
      },
      smartMode: {
        available: this.smartModeService.isAvailable(),
      },
      timestamp: new Date().toISOString(),
    };
  }

  @Post('live-translation/start')
  async startLiveTranslation(@Body() body: {
    userId: string;
    conversationId: string;
    sourceLanguage?: string;
    targetLanguage?: string;
    engine?: 'azure' | 'openai';
  }): Promise<LiveTranslationSession> {
    this.logger.log(`Starting live translation for user ${body.userId}`);
    
    return await this.liveTranslationService.startTranslationSession(
      body.userId,
      body.conversationId,
      body.sourceLanguage,
      body.targetLanguage,
      body.engine
    );
  }

  @Post('live-translation/:sessionId/end')
  async endLiveTranslation(@Param('sessionId') sessionId: string) {
    this.logger.log(`Ending live translation session ${sessionId}`);
    
    await this.liveTranslationService.endTranslationSession(sessionId);
    
    return {
      success: true,
      sessionId,
      endedAt: new Date().toISOString(),
    };
  }

  @Post('smart-mode/start')
  async startSmartMode(@Body() body: {
    userId: string;
    conversationId: string;
    features?: {
      meetingAssistance?: boolean;
      realTimeChat?: boolean;
      contextAware?: boolean;
    };
  }): Promise<SmartModeSession> {
    this.logger.log(`Starting Smart Mode for user ${body.userId}`);
    
    return await this.smartModeService.startSmartModeSession(
      body.userId,
      body.conversationId,
      body.features
    );
  }

  @Post('smart-mode/:sessionId/end')
  async endSmartMode(@Param('sessionId') sessionId: string) {
    this.logger.log(`Ending Smart Mode session ${sessionId}`);
    
    await this.smartModeService.endSmartModeSession(sessionId);
    
    return {
      success: true,
      sessionId,
      endedAt: new Date().toISOString(),
    };
  }

  @Post('smart-mode/:sessionId/meeting-assistance')
  async requestMeetingAssistance(@Param('sessionId') sessionId: string, @Body() body: {
    assistanceType: 'summary' | 'action_items' | 'next_steps' | 'insights';
    context?: any;
  }) {
    this.logger.log(`Requesting meeting assistance for session ${sessionId}: ${body.assistanceType}`);
    
    await this.smartModeService.requestMeetingAssistance(
      sessionId,
      body.assistanceType,
      body.context
    );
    
    return {
      success: true,
      requestedAt: new Date().toISOString(),
    };
  }
}
