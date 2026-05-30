import { AppConfig } from '../../common/config/app-config';
import { HttpError } from '../../common/http/http-error';

/**
 * AI-powered intelligence service for support workflows
 * Handles ticket categorization, priority detection, and intelligent routing
 */
export interface TicketAnalysis {
  ticketId: string;
  category: string;
  subcategory?: string;
  priority: 'critical' | 'high' | 'medium' | 'low';
  sentiment: 'positive' | 'neutral' | 'negative';
  suggestedTemplate: string;
  routingTarget: string;
  confidence: number;
  reasoning: string;
}

export interface NotificationRouting {
  notificationId: string;
  recommendedChannels: Array<{
    channel: 'email' | 'sms' | 'push' | 'in_app';
    priority: number;
    reasoning: string;
  }>;
  bestTimeToSend?: string;
}

export interface ConnectorRecommendation {
  connectorKey: string;
  connectorName: string;
  relevanceScore: number;
  reasoning: string;
  tags: string[];
}

export class SupportIntelligenceService {
  constructor(private readonly config: AppConfig) {}

  /**
   * Analyze a support ticket using AI
   * Categorizes, detects priority and sentiment, suggests response template
   */
  async analyzeTicket(ticketData: {
    id: string;
    title: string;
    description: string;
    createdBy?: string;
    status?: string;
  }): Promise<TicketAnalysis> {
    try {
      const response = await fetch(`${this.config.aiCoreUrl || 'http://ai-core:8001'}/v1/analyze`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${this.config.authCoreInternalApiKey}`,
        },
        body: JSON.stringify({
          type: 'ticket_analysis',
          payload: {
            ticketId: ticketData.id,
            title: ticketData.title,
            description: ticketData.description,
            metadata: {
              createdBy: ticketData.createdBy,
              status: ticketData.status,
            },
          },
        }),
      });

      if (!response.ok) {
        throw new HttpError(502, 'ai_analysis_failed', `AI analysis failed: ${response.statusText}`);
      }

      const result = (await response.json()) as {
        analysis: TicketAnalysis;
      };

      return result.analysis;
    } catch (err) {
      if (err instanceof HttpError) throw err;
      // Fallback to basic categorization if AI is unavailable
      return {
        ticketId: ticketData.id,
        category: 'general',
        priority: 'medium',
        sentiment: 'neutral',
        suggestedTemplate: 'default-response',
        routingTarget: 'support-queue',
        confidence: 0.5,
        reasoning: 'AI service unavailable; using default categorization',
      };
    }
  }

  /**
   * Determine optimal notification channels for a subscriber
   */
  async optimizeNotificationChannels(params: {
    subscriberId: string;
    notificationType: string;
    urgency: 'critical' | 'high' | 'normal' | 'low';
    context?: Record<string, unknown>;
  }): Promise<NotificationRouting> {
    try {
      const response = await fetch(`${this.config.aiCoreUrl || 'http://ai-core:8001'}/v1/routing`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${this.config.authCoreInternalApiKey}`,
        },
        body: JSON.stringify({
          type: 'notification_routing',
          payload: params,
        }),
      });

      if (!response.ok) {
        throw new HttpError(502, 'routing_failed', 'Notification routing failed');
      }

      const result = (await response.json()) as {
        routing: NotificationRouting;
      };

      return result.routing;
    } catch (err) {
      if (err instanceof HttpError) throw err;
      // Fallback: default to email for all notifications
      return {
        notificationId: params.subscriberId,
        recommendedChannels: [
          {
            channel: 'email',
            priority: 1,
            reasoning: 'Default channel when AI routing unavailable',
          },
        ],
      };
    }
  }

  /**
   * Get AI-powered connector recommendations based on ticket/workflow context
   */
  async recommendConnectors(params: {
    ticketId?: string;
    workflowContext?: Record<string, unknown>;
    existingConnectors?: string[];
  }): Promise<ConnectorRecommendation[]> {
    try {
      const response = await fetch(
        `${this.config.aiCoreUrl || 'http://ai-core:8001'}/v1/connector-recommendations`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'Authorization': `Bearer ${this.config.authCoreInternalApiKey}`,
          },
          body: JSON.stringify({
            type: 'connector_recommendation',
            payload: params,
          }),
        },
      );

      if (!response.ok) {
        throw new HttpError(502, 'recommendation_failed', 'Connector recommendation failed');
      }

      const result = (await response.json()) as {
        recommendations: ConnectorRecommendation[];
      };

      return result.recommendations;
    } catch (err) {
      if (err instanceof HttpError) throw err;
      // Fallback: empty recommendations when AI unavailable
      return [];
    }
  }
}
