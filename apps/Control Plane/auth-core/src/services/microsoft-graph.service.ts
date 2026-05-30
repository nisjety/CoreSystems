import { Injectable, Logger } from '@nestjs/common';
import { HttpService } from '@nestjs/axios';
import { firstValueFrom } from 'rxjs';

export interface MicrosoftGraphProfile {
  displayName?: string;
  givenName?: string;
  surname?: string;
  mail?: string;
  userPrincipalName?: string;
  jobTitle?: string;
  officeLocation?: string;
  mobilePhone?: string;
  photo?: string;
}

@Injectable()
export class MicrosoftGraphService {
  private readonly logger = new Logger(MicrosoftGraphService.name);
  private readonly graphApiBaseUrl = 'https://graph.microsoft.com/v1.0';

  constructor(private readonly httpService: HttpService) {}

  /**
   * Fetch user profile from Microsoft Graph API
   */
  async getUserProfile(accessToken: string): Promise<MicrosoftGraphProfile> {
    try {
      const profile = await this.fetchProfile(accessToken);
      const photo = await this.fetchProfilePhoto(accessToken);

      return {
        ...profile,
        photo,
      };
    } catch (error) {
      this.logger.error(
        'Error fetching Microsoft Graph profile:',
        error.message,
      );
      throw error;
    }
  }

  /**
   * Fetch basic profile information
   */
  private async fetchProfile(
    accessToken: string,
  ): Promise<Partial<MicrosoftGraphProfile>> {
    try {
      const response = await firstValueFrom(
        this.httpService.get<{
          displayName: string;
          givenName: string;
          surname: string;
          mail: string;
          userPrincipalName: string;
          jobTitle: string;
          officeLocation: string;
          mobilePhone: string;
        }>(`${this.graphApiBaseUrl}/me`, {
          headers: {
            Authorization: `Bearer ${accessToken}`,
          },
          timeout: 5000,
        }),
      );

      this.logger.log(
        `Fetched Microsoft Graph profile for: ${response.data.displayName}`,
      );

      return {
        displayName: response.data.displayName,
        givenName: response.data.givenName,
        surname: response.data.surname,
        mail: response.data.mail,
        userPrincipalName: response.data.userPrincipalName,
        jobTitle: response.data.jobTitle,
        officeLocation: response.data.officeLocation,
        mobilePhone: response.data.mobilePhone,
      };
    } catch (error) {
      this.logger.warn(
        'Failed to fetch Microsoft Graph profile:',
        error instanceof Error ? error.message : String(error),
      );
      throw error;
    }
  }

  /**
   * Fetch user profile photo as base64 string
   */
  private async fetchProfilePhoto(
    accessToken: string,
  ): Promise<string | undefined> {
    try {
      const response = await firstValueFrom(
        this.httpService.get<ArrayBuffer>(
          `${this.graphApiBaseUrl}/me/photo/$value`,
          {
            headers: {
              Authorization: `Bearer ${accessToken}`,
            },
            responseType: 'arraybuffer',
            timeout: 5000,
          },
        ),
      );

      // Convert ArrayBuffer to base64
      const base64Photo = Buffer.from(response.data).toString('base64');
      const photoDataUrl = `data:image/jpeg;base64,${base64Photo}`;

      this.logger.log('Fetched Microsoft Graph profile photo');
      return photoDataUrl;
    } catch {
      // Photo is optional - don't log as error
      this.logger.debug('No profile photo available from Microsoft Graph');
      return undefined;
    }
  }

  /**
   * Check if user has a valid Microsoft account with access token
   */
  hasValidMicrosoftAccount(user: any): {
    valid: boolean;
    accessToken?: string;
  } {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
    if (!user || !user.accounts || !Array.isArray(user.accounts)) {
      return { valid: false };
    }

    // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access, @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-assignment
    const msAccount = user.accounts.find(
      // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
      (account: any) => account.providerId === 'microsoft',
    );

    // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
    if (!msAccount || !msAccount.accessToken) {
      return { valid: false };
    }

    // Check if token is expired
    // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access, @typescript-eslint/no-unsafe-argument
    if (msAccount.expiresAt && new Date(msAccount.expiresAt) < new Date()) {
      this.logger.warn('Microsoft access token expired');
      return { valid: false };
    }

    return {
      valid: true,
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-member-access
      accessToken: msAccount.accessToken,
    };
  }
}
