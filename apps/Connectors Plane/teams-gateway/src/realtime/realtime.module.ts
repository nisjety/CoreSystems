import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { LiveTranslationService } from '../services/live-translation.service';
import { SmartModeService } from '../services/smart-mode.service';
import { RealtimeGatewayController } from './realtime-gateway.controller';

@Module({
  imports: [ConfigModule],
  providers: [
    LiveTranslationService,
    SmartModeService,
  ],
  controllers: [RealtimeGatewayController],
  exports: [
    LiveTranslationService,
    SmartModeService,
  ],
})
export class RealtimeModule {}
