import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { TypeOrmModule } from '@nestjs/typeorm';
import { HttpModule } from '@nestjs/axios';
import { BotModule } from './bot/bot.module';
import { AiModule } from './ai/ai.module';
import { DatabaseConfig } from './config/database.config';
import { HealthController } from './health/health.controller';
import { UserSession } from './entities/user-session.entity';
import { SessionMessage } from './entities/session-message.entity';
import { UserState } from './entities/user-state.entity';
import { SessionService } from './services/session.service';
import { UserStateService } from './services/user-state.service';
import { SessionCommandService } from './services/session-command.service';
import { AdaptiveCardService } from './services/adaptive-card.service';
import { AiTitleGeneratorService } from './services/ai-title-generator.service';

@Module({
  imports: [
    // Configuration module with environment variables
    ConfigModule.forRoot({
      isGlobal: true,
      envFilePath: ['.env.local', '.env'],
    }),
    
    // HTTP module for external API calls
    HttpModule.register({
      timeout: 30310,
      maxRedirects: 5,
    }),
    
    // Database connection
    TypeOrmModule.forRootAsync({
      useClass: DatabaseConfig,
    }),
    
    // TypeORM repositories for session management
    TypeOrmModule.forFeature([UserSession, SessionMessage, UserState]),
    
    // Bot module for Microsoft Teams integration
    BotModule,
    
    // AI module for intelligent responses and audio services
    AiModule,
  ],
  controllers: [HealthController],
  providers: [
    SessionService,
    UserStateService,
    SessionCommandService,
    AdaptiveCardService,
    AiTitleGeneratorService,
  ],
  exports: [
    SessionService,
    UserStateService,
    SessionCommandService,
    AdaptiveCardService,
    AiTitleGeneratorService,
  ],
})
export class AppModule {}
