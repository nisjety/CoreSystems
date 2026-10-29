import { Injectable } from '@nestjs/common';
import { Resend } from 'resend';
import { generateEmailVerificationTemplate } from './templates/email-verification.template';
import { generatePasswordResetTemplate } from './templates/password-reset.template';
import { generateOtpTemplate } from './templates/otp.template';
import { EMAIL_CONSTANTS } from './templates/email-constants';

@Injectable()
export class ResendService {
  private readonly resend: Resend | null;

  constructor() {
    const apiKey = process.env.RESEND_API_KEY;
    if (apiKey) {
      this.resend = new Resend(apiKey);
    } else {
      console.warn('⚠️ RESEND_API_KEY not set, email service disabled');
      this.resend = null;
    }
  }

  async sendVerificationEmail(to: string, verificationUrl: string) {
    if (!this.resend) {
      console.log(`Mock: Verification email to ${to}`);
      return;
    }

    try {
      // Generate Norwegian email template
      const emailTemplate = generateEmailVerificationTemplate({
        userEmail: to,
        userName: undefined, // Will use email prefix as fallback
        verificationUrl,
        companyName: EMAIL_CONSTANTS.COMPANY.NAME,
        supportEmail: EMAIL_CONSTANTS.COMPANY.SUPPORT_EMAIL,
      });

      await this.resend.emails.send({
        from: `${process.env.RESEND_FROM_NAME || EMAIL_CONSTANTS.COMPANY.NAME} <${process.env.RESEND_FROM_EMAIL || EMAIL_CONSTANTS.COMPANY.SUPPORT_EMAIL}>`,
        to,
        subject: emailTemplate.subject,
        html: emailTemplate.html,
        text: emailTemplate.text,
        // Email authentication headers for better deliverability
        headers: {
          'X-Entity-Ref-ID': `verification-${Date.now()}`,
          'X-Priority': '1',
          'X-MSMail-Priority': 'High',
          Importance: 'high',
        },
      });

      console.log(`Norwegian verification email sent to ${to}`);
    } catch (error) {
      console.error('Failed to send verification email:', error);
      throw new Error('Failed to send verification email');
    }
  }

  async sendPasswordResetEmail(to: string, resetUrl: string) {
    if (!this.resend) {
      console.log(`Mock: Password reset email to ${to}`);
      return;
    }

    try {
      // Generate Norwegian email template
      const emailTemplate = generatePasswordResetTemplate({
        userEmail: to,
        userName: undefined, // Will use email prefix as fallback
        resetUrl,
        companyName: EMAIL_CONSTANTS.COMPANY.NAME,
        supportEmail: EMAIL_CONSTANTS.COMPANY.SUPPORT_EMAIL,
      });

      await this.resend.emails.send({
        from: `${process.env.RESEND_FROM_NAME || EMAIL_CONSTANTS.COMPANY.NAME} <${process.env.RESEND_FROM_EMAIL || EMAIL_CONSTANTS.COMPANY.SUPPORT_EMAIL}>`,
        to,
        subject: emailTemplate.subject,
        html: emailTemplate.html,
        text: emailTemplate.text,
        // Email authentication headers for better deliverability
        headers: {
          'X-Entity-Ref-ID': `password-reset-${Date.now()}`,
          'X-Priority': '1',
          'X-MSMail-Priority': 'High',
          Importance: 'high',
        },
      });

      console.log(`Norwegian password reset email sent to ${to}`);
    } catch (error) {
      console.error('Failed to send password reset email:', error);
      throw new Error('Failed to send password reset email');
    }
  }

  async sendOTPEmail(to: string, otp: string) {
    if (!this.resend) {
      console.log(`Mock: OTP email to ${to}`);
      return;
    }

    try {
      // Generate Norwegian OTP email template
      const emailTemplate = generateOtpTemplate({
        userEmail: to,
        userName: undefined, // Will use email prefix as fallback
        otp,
        type: 'sign-in', // Default type, can be configured based on context
        companyName: EMAIL_CONSTANTS.COMPANY.NAME,
        supportEmail: EMAIL_CONSTANTS.COMPANY.SUPPORT_EMAIL,
        expiresInMinutes: EMAIL_CONSTANTS.SECURITY.OTP_DEFAULT_EXPIRY_MINUTES,
      });

      await this.resend.emails.send({
        from: `${process.env.RESEND_FROM_NAME || EMAIL_CONSTANTS.COMPANY.NAME} <${process.env.RESEND_FROM_EMAIL || EMAIL_CONSTANTS.COMPANY.SUPPORT_EMAIL}>`,
        to,
        subject: emailTemplate.subject,
        html: emailTemplate.html,
        text: emailTemplate.text,
        // Email authentication headers for better deliverability
        headers: {
          'X-Entity-Ref-ID': `otp-${Date.now()}`,
          'X-Priority': '1',
          'X-MSMail-Priority': 'High',
          Importance: 'high',
        },
      });

      console.log(`Norwegian OTP email sent to ${to}`);
    } catch (error) {
      console.error('Failed to send OTP email:', error);
      throw new Error('Failed to send OTP email');
    }
  }
}
