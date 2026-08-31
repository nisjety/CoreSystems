import { Injectable } from '@nestjs/common';
import * as dotenv from 'dotenv';
import type { Twilio } from 'twilio';

// Load environment variables
dotenv.config();

// Twilio Verify configuration interface
interface TwilioVerifyConfig {
  accountSid: string;
  authToken: string;
  verifyServiceSid: string;
}

// SMS Service interface
export interface SmsService {
  sendOtp(phoneNumber: string, otp?: string, type?: string): Promise<void>;
  verifyOtp(phoneNumber: string, otp: string): Promise<boolean>;
  validatePhoneNumber(phoneNumber: string): boolean;
}

@Injectable()
export class TwilioVerifyService implements SmsService {
  private twilio: Twilio | undefined;
  private config: TwilioVerifyConfig;

  constructor() {
    this.config = {
      accountSid: process.env.TWILIO_ACCOUNT_SID || '',
      authToken: process.env.TWILIO_AUTH_TOKEN || '',
      verifyServiceSid: process.env.TWILIO_VERIFY_SERVICE_SID || '',
    };

    // Only initialize Twilio if credentials are provided
    if (
      this.config.accountSid &&
      this.config.authToken &&
      this.config.verifyServiceSid
    ) {
      try {
        // Dynamic import for Twilio (will be installed later)
        // eslint-disable-next-line @typescript-eslint/no-require-imports, @typescript-eslint/no-unsafe-assignment
        const twilio = require('twilio');
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-call
        this.twilio = twilio(this.config.accountSid, this.config.authToken);
        console.log('✅ Twilio Verify service initialized successfully');
      } catch {
        console.warn(
          '⚠️ Twilio SDK not found. Install with: npm install twilio',
        );
        console.warn(
          '📱 SMS functionality will be disabled until Twilio is configured',
        );
      }
    } else {
      console.warn(
        '⚠️ Twilio Verify credentials not configured. SMS functionality will be disabled.',
      );
      console.warn(
        'Required: TWILIO_ACCOUNT_SID, TWILIO_AUTH_TOKEN, TWILIO_VERIFY_SERVICE_SID',
      );
    }
  }

  /**
   * Send OTP using Twilio Verify API
   * Note: OTP parameter is ignored as Twilio generates its own codes
   */
  async sendOtp(
    phoneNumber: string,
    otp?: string, // Ignored - Twilio Verify generates its own codes
    type:
      | 'phone-verification'
      | '2fa'
      | 'password-reset' = 'phone-verification',
  ): Promise<void> {
    if (!this.twilio) {
      console.log(
        `📱 [MOCK SMS] To: ${phoneNumber}, Type: ${type} (Twilio Verify)`,
      );
      return;
    }

    try {
      // Format phone number to E.164 format
      const formattedPhone = this.formatPhoneNumber(phoneNumber);

      // Create verification using Twilio Verify API
      const verification = await this.twilio.verify.v2
        .services(this.config.verifyServiceSid)
        .verifications.create({
          to: formattedPhone,
          channel: 'sms',
        });

      console.log('✅ Verification SMS sent successfully:', verification.sid);
      console.log('📱 Verification status:', verification.status);
    } catch (error) {
      console.error('❌ Failed to send verification SMS:', error);
      throw error;
    }
  }

  /**
   * Verify OTP using Twilio Verify API
   */
  async verifyOtp(phoneNumber: string, otp: string): Promise<boolean> {
    if (!this.twilio) {
      console.log(
        `📱 [MOCK VERIFY] Phone: ${phoneNumber}, OTP: ${otp} - APPROVED`,
      );
      return true; // Mock verification always succeeds
    }

    try {
      // Format phone number to E.164 format
      const formattedPhone = this.formatPhoneNumber(phoneNumber);

      // Check verification using Twilio Verify API
      const verificationCheck = await this.twilio.verify.v2
        .services(this.config.verifyServiceSid)
        .verificationChecks.create({
          to: formattedPhone,
          code: otp,
        });

      const isApproved = verificationCheck.status === 'approved';

      if (isApproved) {
        console.log('✅ OTP verification successful:', verificationCheck.sid);
      } else {
        console.log('❌ OTP verification failed:', verificationCheck.status);
      }

      return isApproved;
    } catch (error) {
      console.error('❌ Failed to verify OTP:', error);
      return false;
    }
  }

  /**
   * Validate phone number format (basic validation)
   */
  validatePhoneNumber(phoneNumber: string): boolean {
    // Basic E.164 format validation
    const phoneRegex = /^\+[1-9]\d{1,14}$/;
    return phoneRegex.test(phoneNumber);
  }

  /**
   * Format phone number to E.164 format
   */
  formatPhoneNumber(
    phoneNumber: string,
    defaultCountryCode: string = '+47', // Norway default
  ): string {
    // Remove all non-digit characters except +
    let cleaned = phoneNumber.replace(/[^\d+]/g, '');

    // If it already starts with +, validate and return
    if (cleaned.startsWith('+')) {
      return cleaned;
    }

    // Remove leading zeros
    cleaned = cleaned.replace(/^0+/, '');

    // If it starts with country code (47 for Norway)
    if (cleaned.startsWith('47') && cleaned.length === 10) {
      return '+' + cleaned;
    }

    // If it's a local Norwegian number (8 digits)
    if (cleaned.length === 8 && defaultCountryCode === '+47') {
      return '+47' + cleaned;
    }

    // Default: add the default country code
    return defaultCountryCode + cleaned;
  }
}
