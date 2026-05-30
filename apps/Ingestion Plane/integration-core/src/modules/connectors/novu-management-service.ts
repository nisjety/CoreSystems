import { AppConfig } from '../../common/config/app-config';
import { HttpError } from '../../common/http/http-error';

/**
 * Novu notification management service
 * Handles notification templates, subscriber preferences, and delivery channels
 */
export interface NovuNotificationTemplate {
  _id: string;
  name: string;
  identifier: string;
  description?: string;
  channels: Array<{
    type: 'email' | 'sms' | 'push' | 'in_app' | 'slack' | 'discord' | 'teams';
    enabled: boolean;
  }>;
}

export interface NovuSubscriberPreferences {
  subscriberId: string;
  email: string;
  channels: Array<{
    type: string;
    enabled: boolean;
  }>;
  globalPreferences: {
    email: boolean;
    sms: boolean;
    push: boolean;
    in_app: boolean;
  };
}

export interface SendNotificationRequest {
  subscriberId: string;
  templateId: string;
  payload: Record<string, unknown>;
  overrides?: {
    email?: { from?: string; to?: string };
    sms?: { from?: string };
    [key: string]: unknown;
  };
}

export interface NotificationDeliveryStatus {
  notificationId: string;
  status: 'sent' | 'failed' | 'pending' | 'read';
  channels: Array<{
    type: string;
    status: string;
    error?: string;
  }>;
}

export class NovuManagementService {
  constructor(private readonly config: AppConfig) {}

  /**
   * List all notification templates
   */
  async listTemplates(): Promise<NovuNotificationTemplate[]> {
    try {
      const response = await fetch(`${this.config.novuApiUrl || 'https://api.novu.co'}/v1/templates`, {
        method: 'GET',
        headers: {
          'Authorization': `ApiKey ${this.config.novuApiKey || ''}`,
          'Accept': 'application/json',
        },
      });

      if (!response.ok) {
        throw new HttpError(502, 'novu_list_failed', `Failed to fetch Novu templates: ${response.statusText}`);
      }

      const data = (await response.json()) as { data: NovuNotificationTemplate[] };
      return data.data || [];
    } catch (err) {
      if (err instanceof HttpError) throw err;
      throw new HttpError(500, 'novu_service_error', 'Failed to list Novu templates');
    }
  }

  /**
   * Get template by ID
   */
  async getTemplate(templateId: string): Promise<NovuNotificationTemplate | null> {
    try {
      const response = await fetch(
        `${this.config.novuApiUrl || 'https://api.novu.co'}/v1/templates/${templateId}`,
        {
          method: 'GET',
          headers: {
            'Authorization': `ApiKey ${this.config.novuApiKey || ''}`,
            'Accept': 'application/json',
          },
        },
      );

      if (response.status === 404) {
        return null;
      }

      if (!response.ok) {
        throw new HttpError(502, 'novu_get_template_failed', 'Failed to fetch Novu template');
      }

      const data = (await response.json()) as { data: NovuNotificationTemplate };
      return data.data || null;
    } catch (err) {
      if (err instanceof HttpError) throw err;
      throw new HttpError(500, 'novu_service_error', 'Failed to get Novu template');
    }
  }

  /**
   * Send a notification to a subscriber using a template
   */
  async sendNotification(request: SendNotificationRequest): Promise<NotificationDeliveryStatus> {
    try {
      const payload = {
        to: {
          subscriberId: request.subscriberId,
        },
        templateIdentifier: request.templateId,
        payload: request.payload,
        overrides: request.overrides,
      };

      const response = await fetch(`${this.config.novuApiUrl || 'https://api.novu.co'}/v1/trigger`, {
        method: 'POST',
        headers: {
          'Authorization': `ApiKey ${this.config.novuApiKey || ''}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(payload),
      });

      if (!response.ok) {
        throw new HttpError(502, 'novu_send_failed', `Failed to send notification: ${response.statusText}`);
      }

      const data = (await response.json()) as { data: { transactionId: string } };

      return {
        notificationId: data.data?.transactionId || '',
        status: 'sent',
        channels: [
          {
            type: 'email',
            status: 'queued',
          },
        ],
      };
    } catch (err) {
      if (err instanceof HttpError) throw err;
      throw new HttpError(500, 'novu_service_error', 'Failed to send notification');
    }
  }

  /**
   * Get subscriber notification preferences
   */
  async getSubscriberPreferences(subscriberId: string): Promise<NovuSubscriberPreferences | null> {
    try {
      const response = await fetch(
        `${this.config.novuApiUrl || 'https://api.novu.co'}/v1/subscribers/${subscriberId}/preferences`,
        {
          method: 'GET',
          headers: {
            'Authorization': `ApiKey ${this.config.novuApiKey || ''}`,
            'Accept': 'application/json',
          },
        },
      );

      if (response.status === 404) {
        return null;
      }

      if (!response.ok) {
        throw new HttpError(502, 'novu_get_prefs_failed', 'Failed to fetch subscriber preferences');
      }

      const data = (await response.json()) as { data: NovuSubscriberPreferences };
      return data.data || null;
    } catch (err) {
      if (err instanceof HttpError) throw err;
      throw new HttpError(500, 'novu_service_error', 'Failed to get subscriber preferences');
    }
  }

  /**
   * Update subscriber notification preferences
   */
  async updateSubscriberPreferences(
    subscriberId: string,
    preferences: Partial<NovuSubscriberPreferences>,
  ): Promise<NovuSubscriberPreferences | null> {
    try {
      const response = await fetch(
        `${this.config.novuApiUrl || 'https://api.novu.co'}/v1/subscribers/${subscriberId}/preferences`,
        {
          method: 'PUT',
          headers: {
            'Authorization': `ApiKey ${this.config.novuApiKey || ''}`,
            'Content-Type': 'application/json',
          },
          body: JSON.stringify(preferences),
        },
      );

      if (!response.ok) {
        throw new HttpError(502, 'novu_update_prefs_failed', 'Failed to update preferences');
      }

      const data = (await response.json()) as { data: NovuSubscriberPreferences };
      return data.data || null;
    } catch (err) {
      if (err instanceof HttpError) throw err;
      throw new HttpError(500, 'novu_service_error', 'Failed to update subscriber preferences');
    }
  }
}
