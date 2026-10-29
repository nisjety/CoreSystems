import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { TurnContext } from 'botbuilder';

export interface TeamsUserContext {
  id: string;
  email: string;
  displayName: string;
  tenantId: string;
  objectId?: string;
  accessToken?: string;
}

@Injectable()
export class TeamsUserService {
  private readonly logger = new Logger(TeamsUserService.name);

  constructor(private configService: ConfigService) {}

  /**
   * Extract user information from Teams context
   */
  async extractUserFromTeamsContext(context: TurnContext): Promise<TeamsUserContext | null> {
    try {
      const activity = context.activity;
      
      // Get basic user info from activity
      const userId = activity.from?.id;
      const displayName = activity.from?.name;
      
      if (!userId) {
        this.logger.warn('No user ID found in Teams activity');
        return null;
      }

      // Extract tenant ID from the conversation or channel data
      const tenantId = activity.channelData?.tenant?.id || 
                      activity.conversation?.tenantId ||
                      activity.serviceUrl?.match(/https:\/\/.*\.sharepoint\.com/)?.[0];

      // Try to get Azure AD object ID and email from Teams context
      let email: string | null = null;
      let objectId = activity.from?.aadObjectId;

      // If we have Teams-specific channel data, try to extract more info
      if (activity.channelData?.teamsUser) {
        const teamsUser = activity.channelData.teamsUser;
        email = teamsUser.userPrincipalName || teamsUser.email;
        objectId = teamsUser.aadObjectId || objectId;
      }

      // Use proper UPN from Teams activity if available
      if (!email && activity.from?.aadObjectId) {
        // For Microsoft Graph API calls, use just the AAD Object ID (GUID)
        // NOT the malformed objectId@tenantId format
        email = activity.from.aadObjectId;
      }

      // Fallback to constructing email from user ID if no proper email found
      if (!email && userId.includes('@')) {
        email = userId;
      } else if (!email) {
        // Construct a pseudo-email for internal use
        email = `${userId}@teams.microsoft.com`;
      }

      const userContext: TeamsUserContext = {
        id: userId,
        email: email,
        displayName: displayName || 'Teams User',
        tenantId: tenantId || 'unknown',
        objectId: objectId
      };

      this.logger.log(`Extracted Teams user context: ${email} (${userId})`);
      return userContext;
    } catch (error) {
      this.logger.error('Failed to extract user from Teams context:', error);
      return null;
    }
  }

  /**
   * Get user access token if available (for Teams SSO scenarios)
   */
  async getUserAccessToken(context: TurnContext): Promise<string | null> {
    try {
      // This would be used in SSO scenarios where Teams provides an access token
      // For now, return null as we'll rely on app-only authentication
      return null;
    } catch (error) {
      this.logger.error('Failed to get user access token:', error);
      return null;
    }
  }

  /**
   * Check if the context represents a valid Teams user
   */
  isValidTeamsUser(userContext: TeamsUserContext | null): boolean {
    return !!(userContext?.id && userContext?.email && userContext?.tenantId);
  }

  /**
   * Sanitize email for logging (partial masking for privacy)
   */
  sanitizeEmailForLogging(email: string): string {
    if (!email) return 'unknown';
    
    const [localPart, domain] = email.split('@');
    if (!domain) return email;
    
    const maskedLocal = localPart.length > 2 
      ? `${localPart[0]}***${localPart[localPart.length - 1]}`
      : '***';
    
    return `${maskedLocal}@${domain}`;
  }
}
