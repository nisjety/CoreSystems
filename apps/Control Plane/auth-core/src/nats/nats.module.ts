import { Module, Global } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { DirectNatsService } from './direct-nats.service';
import { SharedNatsService } from './shared-nats.service';
import { SharedPublisher } from './shared-publisher';
import { ApiKeyRevocationSweeper } from './api-key-revocation.publisher';

@Global()
@Module({
  imports: [ConfigModule],
  providers: [
    DirectNatsService,
    SharedNatsService,
    SharedPublisher,
    ApiKeyRevocationSweeper,
  ],
  exports: [DirectNatsService, SharedNatsService, SharedPublisher],
})
export class NatsModule {}
