import { Injectable } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';

export interface BotConfig {
  // Microsoft App Registration
  appId: string;
  appPassword: string;
  
  // Azure OpenAI Configuration
  azureOpenAIKey: string;
  azureOpenAIEndpoint: string;
  azureOpenAIModel: string;
  azureOpenAIModelForAdmins: string;
  
  // AI Orchestrator Configuration  
  orchestratorServiceUrl: string;
  
  // Environment
  nodeEnv: string;
  port: number;
}

@Injectable()
export class BotConfigService {
  constructor(private configService: ConfigService) {}

  getBotConfig(): BotConfig {
    return {
      // Microsoft App Registration - Required for Teams authentication
      appId: this.configService.get<string>('MICROSOFT_APP_ID', ''),
      appPassword: this.configService.get<string>('MICROSOFT_APP_PASSWORD', ''),
      
      // Azure OpenAI Configuration - Used by Teams AI library for direct AI integration
      azureOpenAIKey: this.configService.get<string>('AZURE_OPENAI_KEY', ''),
      azureOpenAIEndpoint: this.configService.get<string>('AZURE_OPENAI_ENDPOINT', ''),
      azureOpenAIModel: this.configService.get<string>('AZURE_OPENAI_MODEL', 'gpt-4o-mini'),
      azureOpenAIModelForAdmins: this.configService.get<string>('AZURE_OPENAI_MODEL_ADMINS', 'gpt-4o'),
      
      // AI Orchestrator - Fallback service for complex operations
      orchestratorServiceUrl: this.configService.get<string>('ORCHESTRATOR_SERVICE_URL', 'http://localhost:3021'),
      
      // Environment Configuration
      nodeEnv: this.configService.get<string>('NODE_ENV', 'development'),
      port: this.configService.get<number>('PORT', 3031),
    };
  }

  /**
   * Validate that all required configuration is present
   */
  validateConfig(): void {
    const config = this.getBotConfig();
    const requiredFields = [
      { field: 'appId', value: config.appId, name: 'MICROSOFT_APP_ID' },
      { field: 'appPassword', value: config.appPassword, name: 'MICROSOFT_APP_PASSWORD' },
    ];

    const missingFields = requiredFields.filter(field => !field.value);
    
    if (missingFields.length > 0) {
      const missingFieldNames = missingFields.map(field => field.name).join(', ');
      throw new Error(`Missing required configuration: ${missingFieldNames}`);
    }

    // Log configuration status (without sensitive values)
    const hasAzureOpenAI = !!(config.azureOpenAIKey && config.azureOpenAIEndpoint);
    console.log(`🔧 Bot Configuration validated:
      - App ID: ${config.appId ? '✅ Present' : '❌ Missing'}
      - App Password: ${config.appPassword ? '✅ Present' : '❌ Missing'}
      - Azure OpenAI: ${hasAzureOpenAI ? '✅ Configured' : '⚠️  Not configured (using Orchestrator)'}
      - Environment: ${config.nodeEnv}
      - Port: ${config.port}`);
  }
}
