import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { HttpModule } from '@nestjs/axios';
import { TypeOrmModule } from '@nestjs/typeorm';
import { BotController } from './bot.controller';
import { BotService } from './bot.service';
import { ModernTeamsAdapter } from '../adapters/modern-teams.adapter';
import { BotConfigService } from '../config/bot.config';
import { AiModule } from '../ai/ai.module';
import { AuthModule } from '../auth/auth.module';
import { LiveTranslationService } from '../services/live-translation.service';
import { SmartModeService } from '../services/smart-mode.service';
import { SessionService } from '../services/session.service';
import { UserStateService } from '../services/user-state.service';
import { SessionCommandService } from '../services/session-command.service';
import { AdaptiveCardService } from '../services/adaptive-card.service';
import { AiTitleGeneratorService } from '../services/ai-title-generator.service';
import { UserSession } from '../entities/user-session.entity';
import { SessionMessage } from '../entities/session-message.entity';
import { UserState } from '../entities/user-state.entity';

@Module({
  imports: [
    ConfigModule, 
    HttpModule,
    AiModule, 
    AuthModule,
    TypeOrmModule.forFeature([UserSession, SessionMessage, UserState])
  ],
  controllers: [BotController],
  providers: [
    BotService, 
    ModernTeamsAdapter, 
    BotConfigService, 
    LiveTranslationService, 
    SmartModeService,
    SessionService,
    UserStateService,
    SessionCommandService,
    AdaptiveCardService,
    AiTitleGeneratorService,
  ],
  exports: [
    BotService,
    SessionService,
    UserStateService,
    SessionCommandService,
    AdaptiveCardService,
  ],
})
export class BotModule {}
