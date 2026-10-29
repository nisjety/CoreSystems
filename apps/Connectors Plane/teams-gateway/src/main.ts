import 'reflect-metadata';
import { NestFactory } from '@nestjs/core';
import { AppModule } from './app.module';
import { ValidationPipe } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { BotConfigService } from './config/bot.config';

async function bootstrap() {
  const app = await NestFactory.create(AppModule);
  
  // Global validation pipe
  app.useGlobalPipes(new ValidationPipe({
    whitelist: true,
    forbidNonWhitelisted: true,
    transform: true,
  }));

  // CORS configuration for Teams
  app.enableCors({
    origin: [
      'https://teams.microsoft.com',
      'https://*.teams.microsoft.com',
      'https://localhost:*',
      'http://localhost:*',
      'https://ca-ai-orchestrator-zqkqk3y7enzwm.icyforest-10854d59.norwayeast.azurecontainerapps.io', // Azure AI Orchestrator
      /https:\/\/.*\.azurecontainerapps\.io$/ // All Azure Container Apps
    ],
    credentials: true,
  });

  // Validate bot configuration on startup
  try {
    const botConfigService = app.get(BotConfigService);
    botConfigService.validateConfig();
  } catch (error) {
    console.error('❌ Bot configuration validation failed:', error.message);
    console.error('Please check your environment variables and try again.');
    process.exit(1);
  }

  const configService = app.get(ConfigService);
  const port = configService.get<number>('PORT', 3031);
  
  await app.listen(port);
  console.log(`🚀 Teams Gateway Service running on port ${port}`);
  console.log(`📱 Bot endpoint: http://localhost:${port}/api/messages`);
  console.log(`🤖 Teams AI library integration: Ready`);
  console.log(`🔐 Authentication: Microsoft App ID configured`);
}

bootstrap().catch((error) => {
  console.error('❌ Failed to start Teams Gateway Service:', error);
  process.exit(1);
});
