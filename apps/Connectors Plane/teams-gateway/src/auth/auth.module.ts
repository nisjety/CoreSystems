import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { TeamsUserService } from './teams-user.service';

@Module({
  imports: [ConfigModule],
  providers: [TeamsUserService],
  exports: [TeamsUserService],
})
export class AuthModule {}
