import { Injectable } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { TypeOrmModuleOptions, TypeOrmOptionsFactory } from '@nestjs/typeorm';
import { User } from '../entities/user.entity';
import { Conversation } from '../entities/conversation.entity';
import { Message } from '../entities/message.entity';
import { UserSession } from '../entities/user-session.entity';
import { SessionMessage } from '../entities/session-message.entity';
import { UserState } from '../entities/user-state.entity';

@Injectable()
export class DatabaseConfig implements TypeOrmOptionsFactory {
  constructor(private configService: ConfigService) {}

  createTypeOrmOptions(): TypeOrmModuleOptions {
    const isAzurePostgres = this.configService.get<string>('POSTGRES_HOST', '').includes('postgres.database.azure.com');
    
    return {
      type: 'postgres',
      host: this.configService.get<string>('POSTGRES_HOST', 'localhost'),
      port: this.configService.get<number>('POSTGRES_PORT', 5432),
      username: this.configService.get<string>('POSTGRES_USER', 'postgres'),
      password: this.configService.get<string>('POSTGRES_PASSWORD', ''),
      database: this.configService.get<string>('POSTGRES_DB', 'aquatiqchat'),
      entities: [User, Conversation, Message, UserSession, SessionMessage, UserState],
      synchronize: this.configService.get<boolean>('DB_SYNCHRONIZE', false),
      logging: this.configService.get<boolean>('DB_LOGGING', true),
      // Enhanced SSL configuration for Azure PostgreSQL
      ssl: isAzurePostgres ? { 
        rejectUnauthorized: false,
        ca: undefined,
        cert: undefined,
        key: undefined,
      } : this.configService.get<boolean>('DB_SSL', false),
      extra: {
        options: '-c search_path=public',
        // Azure PostgreSQL connection settings
        ...(isAzurePostgres && {
          connectionLimit: 5,
          acquireTimeout: 30310,
          ssl: true,
        }),
      },
      retryAttempts: 3,
      retryDelay: 3031,
    };
  }
}
