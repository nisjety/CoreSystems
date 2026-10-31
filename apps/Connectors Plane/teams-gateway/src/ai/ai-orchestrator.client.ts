import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import axios, { AxiosInstance } from 'axios';

interface AiOrchestratorRequest {
  message: string;
  userId: string;
  userName?: string;
  userEmail?: string;
  conversationId: string;
  channelId?: string;
  teamId?: string;
  tenantId?: string;
  objectId?: string;
  systemPrompt?: string;
  attachments?: any[];
}

interface AiOrchestratorResponse {
  response: string;
  conversationId: string;
  timestamp: string;
  usage?: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  };
  model: string;
  securityLevel?: 'public' | 'restricted' | 'confidential';
  accessViolations?: string[];
}

@Injectable()
export class AiOrchestratorClient {
  private readonly logger = new Logger(AiOrchestratorClient.name);
  private readonly httpClient: AxiosInstance;
  private readonly baseUrl: string;

  constructor(private configService: ConfigService) {
    this.baseUrl = this.configService.get<string>('ORCHESTRATOR_SERVICE_URL', 'http://localhost:3021');
    
    this.httpClient = axios.create({
      baseURL: this.baseUrl,
      timeout: 30310, // 30 seconds
      headers: {
        'Content-Type': 'application/json',
      },
    });

    this.logger.log(`Initialized AI Orchestrator client for ${this.baseUrl}`);
  }

  /**
   * Send a message to the AI Orchestrator and get an intelligent response
   */
  async getAiResponse(request: AiOrchestratorRequest): Promise<AiOrchestratorResponse> {
    try {
      this.logger.log(`Sending message to AI Orchestrator for user ${request.userId}`);
      
      const response = await this.httpClient.post<AiOrchestratorResponse>('/api/chat/message', request);
      
      this.logger.log(`Received AI response for conversation ${request.conversationId}: ${response.data.model}`);
      
      return response.data;
    } catch (error) {
      this.logger.error(`Error getting AI response: ${error.message}`, error.stack);
      
      // Return a fallback response
      return {
        response: "I apologize, but I'm having trouble connecting to my AI brain right now. Please try again later.",
        conversationId: request.conversationId,
        timestamp: new Date().toISOString(),
        model: 'fallback-client',
      };
    }
  }

  /**
   * Check if AI Orchestrator is healthy
   */
  async healthCheck(): Promise<boolean> {
    try {
      const response = await this.httpClient.get('/api/chat/health');
      return response.status === 200 && response.data.status === 'ok';
    } catch (error) {
      this.logger.warn(`AI Orchestrator health check failed: ${error.message}`);
      return false;
    }
  }

  /**
   * Get conversation history
   */
  async getConversationHistory(conversationId: string) {
    try {
      const response = await this.httpClient.get(`/api/chat/conversation/${conversationId}`);
      return response.data;
    } catch (error) {
      this.logger.error(`Error getting conversation history: ${error.message}`, error.stack);
      throw error;
    }
  }

  /**
   * Clear conversation history
   */
  async clearConversation(conversationId: string) {
    try {
      const response = await this.httpClient.post(`/api/chat/conversation/${conversationId}/clear`);
      return response.data;
    } catch (error) {
      this.logger.error(`Error clearing conversation: ${error.message}`, error.stack);
      throw error;
    }
  }
}
