import { betterAuth } from 'better-auth';
import { drizzleAdapter } from 'better-auth/adapters/drizzle';
import {
  emailOTP,
  twoFactor,
  phoneNumber,
  organization,
  oidcProvider,
  multiSession,
  genericOAuth,
  haveIBeenPwned,
  bearer,
  admin,
} from 'better-auth/plugins';
import { apiKey } from '@better-auth/api-key';
import { passkey } from '@better-auth/passkey';
import { sso } from '@better-auth/sso';
import { db } from '../db';
import * as schema from '../db/schema';
import { Resend } from 'resend';
import { redisSecondaryStorage } from '../db/redis';
import { createCipheriv, randomBytes } from 'crypto';
import { importPKCS8, SignJWT } from 'jose';
import * as dotenv from 'dotenv';
import {
  generateEmailVerificationTemplate,
  generatePasswordResetTemplate,
  generateOtpTemplate,
  generateEmailChangeTemplate,
  generateAccountDeletionTemplate,
} from '../email/templates';
import { TwilioVerifyService } from '../sms/twilio-verify.service';
import { auditPlugin } from './audit-plugin';
import { userServiceIntegrationPlugin } from './user-service-integration.plugin';
import { organizationEventsPlugin } from './organization-events.plugin';

// Ensure environment variables are loaded
dotenv.config();

const isProductionLike = process.env.NODE_ENV === 'production';

function envFlag(name: string, defaultValue = false): boolean {
  const value = process.env[name];
  if (value === undefined) return defaultValue;
  return value === 'true';
}

function requireProductionConfig(name: string): string {
  const value = process.env[name]?.trim();
  if (!value && isProductionLike) {
    throw new Error(`${name} is required in production`);
  }
  return value ?? '';
}

function parsePositiveInt(value: string | undefined, fallback: number): number {
  const parsed = Number.parseInt(value ?? '', 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

if (isProductionLike && process.env.RATE_LIMIT_ENABLED === 'false') {
  throw new Error('RATE_LIMIT_ENABLED=false is not allowed in production');
}

function splitEnvList(value?: string): string[] {
  return (value ?? '')
    .split(',')
    .map((origin) => origin.trim().replace(/\/+$/, ''))
    .filter(Boolean);
}

function uniqueOrigins(values: Array<string | undefined>) {
  return Array.from(
    new Set(
      values
        .flatMap((value) => splitEnvList(value))
        .filter(
          (origin) =>
            origin.startsWith('http://') || origin.startsWith('https://'),
        ),
    ),
  );
}

const trustedOrigins = uniqueOrigins([
  'http://localhost:3000',
  'http://127.0.0.1:3000',
  'http://localhost:3107',
  'http://127.0.0.1:3107',
  process.env.APPLE_CLIENT_ID ? 'https://appleid.apple.com' : undefined,
  process.env.BETTER_AUTH_URL || 'http://localhost:3011',
  process.env.FRONTEND_URL || 'http://localhost:3000',
  process.env.NEXT_PUBLIC_APP_URL,
  process.env.BETTER_AUTH_TRUSTED_ORIGINS,
  process.env.AUTH_ALLOWED_ORIGINS,
]);

// Define interface for SMS service
interface SmsServiceInterface {
  sendOtp(phoneNumber: string, otp?: string, type?: string): Promise<void>;
  verifyOtp?(phoneNumber: string, otp: string): Promise<boolean>;
  validatePhoneNumber(phoneNumber: string): boolean;
}

// Define interface for OAuth tokens
interface OAuthTokens {
  accessToken: string;
  refreshToken?: string;
  idToken?: string;
}

// Define interface for Vipps user info response
interface VippsUserInfo {
  sub: string;
  email: string;
  given_name?: string;
  family_name?: string;
  picture?: string;
  email_verified?: boolean;
}

// Define interface for Okta user info response
interface OktaUserInfo {
  sub: string;
  email: string;
  name?: string;
  given_name?: string;
  family_name?: string;
  picture?: string;
  email_verified?: boolean;
}

// Define interface for OIDC user with metadata
interface OIDCUser {
  metadata?: {
    department?: string;
    jobTitle?: string;
  };
}

// Define interface for OIDC client with metadata
interface OIDCClient {
  metadata?: {
    organization?: string;
  };
}

// Define interface for admin user
interface AdminUser {
  email?: string;
}

// Initialize Resend (email provider)

// Define interface for mock resend service
interface MockResendService {
  emails: {
    send: (data: unknown) => Promise<{ data: { id: string }; error: null }>;
  };
}

function createMockResendService(): MockResendService {
  return {
    emails: {
      send: async () => {
        console.log('📧 [MOCK EMAIL] Email send skipped in development');
        await Promise.resolve();
        return { data: { id: 'mock-email-id' }, error: null };
      },
    },
  };
}

function emailFlowsEnabled(): boolean {
  return (
    envFlag('EMAIL_PASSWORD_ENABLED') ||
    envFlag('EMAIL_OTP_ENABLED') ||
    envFlag('ORGANIZATION_ENABLED') ||
    envFlag('REQUIRE_2FA_ON_FIRST_SIGNIN') ||
    envFlag('REQUIRE_2FA_ON_NEW_IP')
  );
}

// Initialize Resend. Production email flows must fail closed if delivery is not configured.
let resend: Resend | MockResendService;
try {
  if (process.env.RESEND_API_KEY) {
    resend = new Resend(process.env.RESEND_API_KEY);
  } else if (isProductionLike && emailFlowsEnabled()) {
    throw new Error(
      'RESEND_API_KEY is required in production when email auth flows are enabled',
    );
  } else {
    console.warn('⚠️ RESEND_API_KEY not set, using mock resend');
    resend = createMockResendService();
  }
} catch (error) {
  console.error('❌ Failed to initialize Resend:', error);
  if (isProductionLike && emailFlowsEnabled()) {
    throw error;
  }
  resend = createMockResendService();
}

// Initialize Twilio Verify service. Phone auth must not approve all OTPs in production.
let smsService: SmsServiceInterface;
try {
  smsService = new TwilioVerifyService();
} catch (error) {
  if (isProductionLike && envFlag('PHONE_AUTH_ENABLED')) {
    throw error;
  }
  console.warn(
    '⚠️ Failed to initialize Twilio Verify service, using mock service',
  );
  // Create a mock service for development/testing
  smsService = {
    sendOtp: async (phoneNumber: string, otp?: string, type?: string) => {
      console.log(
        `📱 [MOCK SMS] To: ${phoneNumber}, OTP: ${otp || 'AUTO'}, Type: ${type}`,
      );
      await Promise.resolve();
    },
    verifyOtp: async (phoneNumber: string, otp: string) => {
      console.log(
        `📱 [MOCK VERIFY] Phone: ${phoneNumber}, OTP: ${otp} - APPROVED`,
      );
      await Promise.resolve();
      return true;
    },
    validatePhoneNumber: (phoneNumber: string) => {
      const phoneRegex = /^\+[1-9]\d{1,14}$/;
      return phoneRegex.test(phoneNumber);
    },
  } as SmsServiceInterface;
}

// Simple AES-256-GCM token encryption for provider tokens
const TOKEN_ENC_KEY_B64 = process.env.TOKEN_ENCRYPTION_KEY;
function encryptToken(token: string): string {
  if (!TOKEN_ENC_KEY_B64) {
    if (isProductionLike) {
      throw new Error('TOKEN_ENCRYPTION_KEY is required in production');
    }
    return token;
  }
  const key = Buffer.from(TOKEN_ENC_KEY_B64, 'base64');
  if (key.length !== 32) {
    if (isProductionLike) {
      throw new Error(
        'TOKEN_ENCRYPTION_KEY must be a 32-byte base64 value in production',
      );
    }
    return token;
  }
  const iv = randomBytes(12);
  const cipher = createCipheriv('aes-256-gcm', key, iv);
  const enc = Buffer.concat([cipher.update(token, 'utf8'), cipher.final()]);
  const tag = cipher.getAuthTag();
  // Store as base64 segments: iv.tag.ciphertext
  return `${iv.toString('base64')}.${tag.toString('base64')}.${enc.toString('base64')}`;
}

function configuredTrustedProviders(): string[] {
  const providers = new Set<string>();
  if (process.env.MICROSOFT_CLIENT_ID && process.env.MICROSOFT_CLIENT_SECRET) {
    providers.add('microsoft');
  }
  if (process.env.GOOGLE_CLIENT_ID && process.env.GOOGLE_CLIENT_SECRET) {
    providers.add('google');
  }
  if (process.env.GITHUB_CLIENT_ID && process.env.GITHUB_CLIENT_SECRET) {
    providers.add('github');
  }
  if (process.env.APPLE_CLIENT_ID) {
    providers.add('apple');
  }
  if (process.env.VIPPS_CLIENT_ID && process.env.VIPPS_CLIENT_SECRET) {
    providers.add('vipps');
  }
  if (
    process.env.OKTA_CLIENT_ID &&
    process.env.OKTA_CLIENT_SECRET &&
    process.env.OKTA_DOMAIN
  ) {
    providers.add('okta');
  }
  return Array.from(providers);
}

function normalizePrivateKey(value: string): string {
  return value.replace(/\\n/g, '\n');
}

async function generateAppleClientSecret(): Promise<string> {
  const clientId = requireProductionConfig('APPLE_CLIENT_ID');
  const teamId = requireProductionConfig('APPLE_TEAM_ID');
  const keyId = requireProductionConfig('APPLE_KEY_ID');
  const privateKey = requireProductionConfig('APPLE_PRIVATE_KEY');

  if (!clientId || !teamId || !keyId || !privateKey) {
    throw new Error(
      'APPLE_CLIENT_ID, APPLE_TEAM_ID, APPLE_KEY_ID and APPLE_PRIVATE_KEY are required when Apple auth is enabled',
    );
  }

  const key = await importPKCS8(normalizePrivateKey(privateKey), 'ES256');
  const now = Math.floor(Date.now() / 1000);
  return new SignJWT({})
    .setProtectedHeader({ alg: 'ES256', kid: keyId })
    .setIssuer(teamId)
    .setSubject(clientId)
    .setAudience('https://appleid.apple.com')
    .setIssuedAt(now)
    .setExpirationTime(now + 180 * 24 * 60 * 60)
    .sign(key);
}

async function appleSocialProviderConfig() {
  const clientId = requireProductionConfig('APPLE_CLIENT_ID');
  const configuredClientSecret = process.env.APPLE_CLIENT_SECRET?.trim();
  const clientSecret =
    configuredClientSecret && configuredClientSecret.length > 0
      ? configuredClientSecret
      : await generateAppleClientSecret();

  return {
    clientId,
    clientSecret,
    appBundleIdentifier: process.env.APPLE_APP_BUNDLE_IDENTIFIER,
  };
}

export const auth: any = betterAuth({
  database: drizzleAdapter(db, {
    provider: 'pg',
    schema,
  }),
  secondaryStorage: redisSecondaryStorage,
  appName: 'ID-Knuten',
  secret: process.env.BETTER_AUTH_SECRET,
  baseURL: process.env.BETTER_AUTH_URL || 'http://localhost:3011',
  trustedOrigins,

  // Rate limiting configuration with IP detection
  rateLimit: {
    enabled: process.env.RATE_LIMIT_ENABLED !== 'false',
    window: 60, // 60 seconds
    max: 100, // 100 requests per window
    storage: 'secondary-storage', // Use Redis (already wired) — avoids rate_limit table write on every request
    customRules: {
      '/sign-in/email': {
        window: 10,
        max: 3,
      },
      '/two-factor/verify-totp': {
        window: parsePositiveInt(process.env.RATE_LIMIT_2FA_VERIFY_WINDOW, 300),
        max: parsePositiveInt(process.env.RATE_LIMIT_2FA_VERIFY_MAX, 5),
      },
      '/two-factor/verify-otp': {
        window: parsePositiveInt(process.env.RATE_LIMIT_2FA_VERIFY_WINDOW, 300),
        max: parsePositiveInt(process.env.RATE_LIMIT_2FA_VERIFY_MAX, 5),
      },
      '/two-factor/send-otp': {
        window: parsePositiveInt(process.env.RATE_LIMIT_OTP_SEND_WINDOW, 60),
        max: parsePositiveInt(process.env.RATE_LIMIT_OTP_SEND_MAX, 2),
      },
      '/phone-number/send-otp': {
        window: parsePositiveInt(process.env.RATE_LIMIT_OTP_SEND_WINDOW, 60),
        max: parsePositiveInt(process.env.RATE_LIMIT_OTP_SEND_MAX, 2),
      },
    },
  },

  // Advanced IP address detection for security
  advanced: {
    // IP address detection headers (Cloudflare, load balancers, etc.)
    ipAddress: {
      ipAddressHeaders: [
        'cf-connecting-ip', // Cloudflare
        'x-forwarded-for', // Standard proxy header
        'x-real-ip', // Nginx real IP
        'true-client-ip', // Cloudflare Enterprise
      ],
    },

    // Prefix all Better Auth cookies (default is 'better-auth')
    cookiePrefix: process.env.COOKIE_PREFIX || 'idknuten',

    // Always secure cookies in non-dev; httpOnly is enforced by Better Auth
    useSecureCookies:
      process.env.USE_SECURE_COOKIES_AUTO === 'true'
        ? process.env.NODE_ENV !== 'development'
        : true,

    // Optional cross-subdomain cookies (enable only if needed)
    crossSubDomainCookies:
      process.env.CROSS_SUBDOMAIN_COOKIES_ENABLED === 'true' &&
      process.env.COOKIE_DOMAIN
        ? { enabled: true, domain: process.env.COOKIE_DOMAIN }
        : undefined,

    // Customize cookie names and attributes
    cookies: {
      session_token: {
        name: process.env.SESSION_COOKIE_NAME || 'sid',
        attributes: {
          path: '/',
          httpOnly: true,
          secure: process.env.NODE_ENV !== 'development',
          sameSite:
            (process.env.COOKIE_SAME_SITE as 'lax' | 'strict' | 'none') ||
            'lax',
          domain: process.env.COOKIE_DOMAIN || undefined,
        },
      },
      // Used when cookieCache is enabled
      session_data: {
        name: process.env.SESSION_DATA_COOKIE_NAME || 'sdata',
        attributes: {
          path: '/',
          httpOnly: true,
          secure: process.env.NODE_ENV !== 'development',
          sameSite:
            (process.env.COOKIE_SAME_SITE as 'lax' | 'strict' | 'none') ||
            'lax',
          domain: process.env.COOKIE_DOMAIN || undefined,
          maxAge: 5 * 60, // keep aligned with session.cookieCache.maxAge
        },
      },
      // Set when remember-me is disabled
      dont_remember: {
        name: process.env.NO_REMEMBER_COOKIE_NAME || 'no_remember',
        attributes: {
          path: '/',
          httpOnly: true,
          secure: process.env.NODE_ENV !== 'development',
          sameSite:
            (process.env.COOKIE_SAME_SITE as 'lax' | 'strict' | 'none') ||
            'lax',
          domain: process.env.COOKIE_DOMAIN || undefined,
        },
      },
    },
  },
  emailAndPassword: {
    enabled: process.env.EMAIL_PASSWORD_ENABLED === 'true',
    requireEmailVerification: process.env.REQUIRE_EMAIL_VERIFICATION === 'true',
    autoSignIn: process.env.AUTO_SIGNIN_AFTER_SIGNUP === 'true',
    // Request Password Reset email sender per docs
    sendResetPassword: async ({
      user,
      url,
    }: {
      user: { email: string; name?: string };
      url: string;
    }) => {
      console.log('🚀 Sending password reset email to:', user.email);
      const companyName = process.env.RESEND_FROM_NAME || 'ID-Knuten';
      const supportEmail =
        process.env.RESEND_SUPPORT_EMAIL || 'support@id-knuten.no';

      const { subject, html, text } = generatePasswordResetTemplate({
        userEmail: user.email,
        userName: user.name,
        resetUrl: url,
        companyName,
        supportEmail,
      });

      try {
        const result = await resend.emails.send({
          from: `${process.env.RESEND_FROM_NAME} <${process.env.RESEND_FROM_EMAIL}>`,
          to: user.email,
          subject,
          html,
          text,
        });
        console.log('✅ Password reset email sent successfully:', result);
      } catch (error) {
        console.error('❌ Password reset email send failed:', error);
        throw error;
      }
    },
    onPasswordReset: async ({ user }: { user: { email: string } }) => {
      console.log(`Password reset for ${user.email}`);
    },
  },

  emailVerification: {
    sendOnSignUp: false, // Automatically send verification email on signup
    autoSignInAfterVerification: false, // Auto sign in after email verification
    callbackURL:
      (process.env.FRONTEND_URL || 'http://localhost:3000') + '/dashboard', // Redirect to frontend after verification
    sendVerificationEmail: async ({
      user,
      url,
    }: {
      user: { email: string; name?: string };
      url: string;
    }) => {
      console.log('🚀 Sending verification email to:', user.email);

      // The verification URL should already point to frontend because of callbackURL config
      // But let's ensure it uses the correct frontend URL
      const frontendUrl = process.env.FRONTEND_URL || 'http://localhost:3000';
      let verificationUrl = url;

      // If the URL points to backend, replace it with frontend
      if (
        url.includes('localhost:3011') ||
        url.includes(process.env.BETTER_AUTH_URL || '')
      ) {
        const urlObj = new URL(url);
        verificationUrl = url.replace(urlObj.origin, frontendUrl);
      }

      const companyName = process.env.RESEND_FROM_NAME || 'ID-Knuten';
      const supportEmail =
        process.env.RESEND_SUPPORT_EMAIL || 'support@id-knuten.no';

      const { subject, html, text } = generateEmailVerificationTemplate({
        userEmail: user.email,
        userName: user.name,
        verificationUrl,
        companyName,
        supportEmail,
      });

      try {
        const result = await resend.emails.send({
          from: `${process.env.RESEND_FROM_NAME} <${process.env.RESEND_FROM_EMAIL}>`,
          to: user.email,
          subject,
          html,
          text,
        });
        console.log('✅ Email sent successfully:', result);
      } catch (error) {
        console.error('❌ Email send failed:', error);
        throw error;
      }
    },
  },

  // Enable user management features
  user: {
    changeEmail: {
      enabled: true,
      async sendChangeEmailVerification({
        user,
        newEmail,
        url,
      }: {
        user: { email: string; name?: string };
        newEmail: string;
        url: string;
      }) {
        const companyName = process.env.RESEND_FROM_NAME || 'ID-Knuten';
        const supportEmail =
          process.env.RESEND_SUPPORT_EMAIL || 'support@id-knuten.no';

        const { subject, html, text } = generateEmailChangeTemplate({
          userEmail: user.email,
          userName: user.name,
          newEmail,
          verificationUrl: url,
          companyName,
          supportEmail,
        });

        // Verification email must be sent to the CURRENT email to approve the change
        await resend.emails.send({
          from: `${process.env.RESEND_FROM_NAME} <${process.env.RESEND_FROM_EMAIL}>`,
          to: user.email,
          subject,
          html,
          text,
        });
      },
    },
    deleteUser: {
      enabled: true,
      async sendDeleteAccountVerification({
        user,
        url,
      }: {
        user: { email: string; name?: string };
        url: string;
      }) {
        const companyName = process.env.RESEND_FROM_NAME || 'ID-Knuten';
        const supportEmail =
          process.env.RESEND_SUPPORT_EMAIL || 'support@id-knuten.no';

        const { subject, html, text } = generateAccountDeletionTemplate({
          userEmail: user.email,
          userName: user.name,
          verificationUrl: url,
          companyName,
          supportEmail,
        });

        await resend.emails.send({
          from: `${process.env.RESEND_FROM_NAME} <${process.env.RESEND_FROM_EMAIL}>`,
          to: user.email,
          subject,
          html,
          text,
        });
      },
      // Optional lifecycle hooks for cleanup/controls
      beforeDelete: async () => {
        // e.g., cleanup external resources or export data
      },
      afterDelete: async () => {
        // e.g., revoke webhooks, notify systems, etc.
      },
    },
  },

  // Encrypt provider tokens before storing in DB
  databaseHooks: {
    account: {
      create: {
        async before(account) {
          const result = { ...account };
          if (typeof account.accessToken === 'string') {
            result.accessToken = encryptToken(account.accessToken);
          }
          if (typeof account.refreshToken === 'string') {
            result.refreshToken = encryptToken(account.refreshToken);
          }
          return { data: result };
        },
      },
      update: {
        async before(account) {
          const result = { ...account };
          if (typeof account.accessToken === 'string') {
            result.accessToken = encryptToken(account.accessToken);
          }
          if (typeof account.refreshToken === 'string') {
            result.refreshToken = encryptToken(account.refreshToken);
          }
          return { data: result };
        },
      },
    },
  },

  // Session settings for security and responsiveness - Extended persistence
  session: {
    expiresIn: parsePositiveInt(process.env.SESSION_EXPIRES_IN, 604800), // 1 week default
    updateAge: parsePositiveInt(process.env.SESSION_UPDATE_AGE, 3600),
    freshAge: parsePositiveInt(process.env.SESSION_FRESH_AGE, 300),
    cookieCache: {
      enabled: process.env.SESSION_COOKIE_CACHE_ENABLED !== 'false', // ON by default; set SESSION_COOKIE_CACHE_ENABLED=false to disable
      maxAge: parsePositiveInt(process.env.SESSION_COOKIE_CACHE_MAX_AGE, 300),
    },
    // Store session ID in Redis for better state management
    storeSessionId: true,
    // Cleanup expired sessions automatically
    cleanupExpiredSessions: true,
  },

  // Expose account linking and unlinking
  // Enabled by default — set ACCOUNT_LINKING_ENABLED=false to disable
  account: {
    accountLinking: {
      enabled: process.env.ACCOUNT_LINKING_ENABLED !== 'false',
      // keep default allowUnlinkingAll=false to prevent lockout; can be toggled via env
      allowUnlinkingAll: process.env.ALLOW_UNLINKING_ALL === 'true',
      // update user info (name, avatar) from linked provider by default
      updateUserInfoOnLink:
        process.env.ACCOUNT_LINKING_UPDATE_USER_INFO !== 'false',
      // Only configured providers are trusted for same-email account linking.
      trustedProviders: configuredTrustedProviders(),
    },
  },

  plugins: [
    // Sprint 4: Audit logging for authentication events
    ...(process.env.AUDIT_ENABLED === 'true' ? [auditPlugin()] : []),

    // Multi-session plugin for device session management (Sprint 4)
    multiSession({
      maximumSessions: parsePositiveInt(process.env.MAX_SESSIONS_PER_USER, 10),
    }),

    // Sprint 2: Generic OAuth for external IDPs (Vipps, Okta)
    ...(process.env.VIPPS_CLIENT_ID && process.env.VIPPS_CLIENT_SECRET
      ? [
          genericOAuth({
            config: [
              {
                providerId: 'vipps',
                clientId: process.env.VIPPS_CLIENT_ID,
                clientSecret: process.env.VIPPS_CLIENT_SECRET,
                discoveryUrl:
                  process.env.VIPPS_ENVIRONMENT === 'production'
                    ? 'https://api.vipps.no/access-management-1.0/access/.well-known/openid-configuration'
                    : 'https://apitest.vipps.no/access-management-1.0/access/.well-known/openid-configuration',
                scopes: ['openid', 'email', 'name', 'address', 'phoneNumber'],
                redirectURI:
                  process.env.VIPPS_REDIRECT_URI ||
                  `${process.env.BETTER_AUTH_URL}/api/auth/callback/vipps`,
                // Custom user info mapping for Vipps
                getUserInfo: async (tokens: OAuthTokens) => {
                  const userInfoUrl =
                    process.env.VIPPS_ENVIRONMENT === 'production'
                      ? 'https://api.vipps.no/access-management-1.0/access/userinfo'
                      : 'https://apitest.vipps.no/access-management-1.0/access/userinfo';

                  const response = await fetch(userInfoUrl, {
                    headers: {
                      Authorization: `Bearer ${tokens.accessToken}`,
                      'Content-Type': 'application/json',
                    },
                  });

                  if (!response.ok) {
                    throw new Error(
                      `Failed to fetch Vipps user info: ${response.statusText}`,
                    );
                  }

                  const userInfo = (await response.json()) as VippsUserInfo;
                  console.log('🦊 Vipps user info:', userInfo);

                  return {
                    id: userInfo.sub,
                    email: userInfo.email,
                    name: `${userInfo.given_name || ''} ${userInfo.family_name || ''}`.trim(),
                    image: userInfo.picture,
                    emailVerified: userInfo.email_verified || false,
                  };
                },
              },
              // Okta OIDC configuration
              ...(process.env.OKTA_CLIENT_ID &&
              process.env.OKTA_CLIENT_SECRET &&
              process.env.OKTA_DOMAIN
                ? [
                    {
                      providerId: 'okta',
                      clientId: process.env.OKTA_CLIENT_ID,
                      clientSecret: process.env.OKTA_CLIENT_SECRET,
                      discoveryUrl: `https://${process.env.OKTA_DOMAIN}/.well-known/openid-configuration`,
                      scopes: ['openid', 'email', 'profile', 'groups'],
                      redirectURI: `${process.env.BETTER_AUTH_URL}/api/auth/callback/okta`,
                      // Custom user info mapping for Okta with JIT provisioning
                      getUserInfo: async (tokens: OAuthTokens) => {
                        const userInfoUrl = `https://${process.env.OKTA_DOMAIN}/oauth2/v1/userinfo`;

                        const response = await fetch(userInfoUrl, {
                          headers: {
                            Authorization: `Bearer ${tokens.accessToken}`,
                            'Content-Type': 'application/json',
                          },
                        });

                        if (!response.ok) {
                          throw new Error(
                            `Failed to fetch Okta user info: ${response.statusText}`,
                          );
                        }

                        const userInfo =
                          (await response.json()) as OktaUserInfo;
                        console.log('🔐 Okta user info:', userInfo);

                        return {
                          id: userInfo.sub,
                          email: userInfo.email,
                          name:
                            userInfo.name ||
                            `${userInfo.given_name || ''} ${userInfo.family_name || ''}`.trim(),
                          image: userInfo.picture,
                          emailVerified: userInfo.email_verified || false,
                        };
                      },
                    },
                  ]
                : []),
            ],
          }),
        ]
      : []),

    // Organization plugin for multi-tenant organization management
    ...(process.env.ORGANIZATION_ENABLED === 'true'
      ? [
          organization({
            allowUserToCreateOrganization:
              process.env.ALLOW_USER_CREATE_ORG === 'true',
            organizationLimit: parsePositiveInt(
              process.env.ORG_LIMIT_PER_USER,
              5,
            ),
            creatorRole: (process.env.ORG_CREATOR_ROLE || 'owner') as
              | 'admin'
              | 'owner',
            membershipLimit: parsePositiveInt(
              process.env.ORG_MEMBERSHIP_LIMIT,
              100,
            ),
            invitationExpiresIn: parsePositiveInt(
              process.env.ORG_INVITATION_EXPIRES_IN,
              172800,
            ), // 48 hours
            requireEmailVerificationOnInvitation:
              process.env.ORG_REQUIRE_EMAIL_VERIFICATION === 'true',
            async sendInvitationEmail(data) {
              const companyName = process.env.RESEND_FROM_NAME || 'ID-Knuten';
              const supportEmail =
                process.env.RESEND_SUPPORT_EMAIL || 'support@id-knuten.no';

              const inviteLink = `${process.env.BETTER_AUTH_URL}/accept-invitation/${data.id}`;

              await resend.emails.send({
                from: `${process.env.RESEND_FROM_NAME} <${process.env.RESEND_FROM_EMAIL}>`,
                to: data.email,
                subject: `You're invited to join ${data.organization.name}`,
                html: `
                  <div style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto;">
                    <h2>Organization Invitation</h2>
                    <p>Hello!</p>
                    <p>You've been invited by <strong>${data.inviter.user.name}</strong> (${data.inviter.user.email}) to join the organization <strong>${data.organization.name}</strong>.</p>
                    <p>Click the link below to accept the invitation:</p>
                    <a href="${inviteLink}" style="background-color: #007bff; color: white; padding: 10px 20px; text-decoration: none; border-radius: 5px; display: inline-block;">Accept Invitation</a>
                    <p>This invitation will expire in 48 hours.</p>
                    <p>If you have any questions, please contact our support team at ${supportEmail}.</p>
                    <p>Best regards,<br>${companyName} Team</p>
                  </div>
                `,
                text: `
                  You've been invited to join ${data.organization.name}
                  
                  Invited by: ${data.inviter.user.name} (${data.inviter.user.email})
                  
                  Click here to accept: ${inviteLink}
                  
                  This invitation expires in 48 hours.
                  
                  Support: ${supportEmail}
                `,
              });
            },
          }),
        ]
      : []),

    // Single Sign-On (SSO) plugin for enterprise authentication
    ...(process.env.SSO_ENABLED === 'true'
      ? [
          sso({
            provisionUser: async ({ user, provider }) => {
              console.log(
                `🏢 Provisioning SSO user: ${user.email} via ${provider.providerId}`,
              );
              // Custom user provisioning logic can be added here
              // e.g., sync with CRM, create workspaces, assign licenses, etc.
            },
            organizationProvisioning: {
              disabled: process.env.SSO_ORG_PROVISIONING_DISABLED === 'true',
              defaultRole: (process.env.SSO_DEFAULT_ROLE || 'member') as
                | 'admin'
                | 'member',
              getRole: async ({
                userInfo,
              }: {
                userInfo: {
                  attributes?: { department?: string; jobTitle?: string };
                };
              }) => {
                // Assign roles based on SSO attributes
                const department = userInfo.attributes?.department;
                const jobTitle = userInfo.attributes?.jobTitle;

                // Admins based on job title
                if (
                  jobTitle?.toLowerCase().includes('manager') ||
                  jobTitle?.toLowerCase().includes('director') ||
                  jobTitle?.toLowerCase().includes('admin')
                ) {
                  return 'admin';
                }

                // IT department gets admin access
                if (department?.toLowerCase() === 'it') {
                  return 'admin';
                }

                return 'member';
              },
            },
          }),
        ]
      : []),

    // OIDC Provider plugin to act as an identity provider
    ...(process.env.OIDC_PROVIDER_ENABLED === 'true'
      ? [
          oidcProvider({
            loginPage: process.env.OIDC_LOGIN_PAGE || '/sign-in',
            consentPage: process.env.OIDC_CONSENT_PAGE || '/consent',
            allowDynamicClientRegistration:
              process.env.OIDC_ALLOW_DYNAMIC_REGISTRATION === 'true',
            useJWTPlugin: process.env.OIDC_USE_JWT_PLUGIN === 'true',
            trustedClients: process.env.OIDC_TRUSTED_CLIENTS
              ? (JSON.parse(process.env.OIDC_TRUSTED_CLIENTS) as any[])
              : [],
            getAdditionalUserInfoClaim: (
              user: any,
              scopes: string[],
              client: any,
            ) => {
              const additionalClaims: Record<string, unknown> = {};

              if (scopes.includes('profile')) {
                additionalClaims.department = (
                  user as OIDCUser
                ).metadata?.department;
                additionalClaims.job_title = (
                  user as OIDCUser
                ).metadata?.jobTitle;
              }

              if (
                scopes.includes('organization') &&
                (client as OIDCClient).metadata?.organization
              ) {
                additionalClaims.organization = (
                  client as OIDCClient
                ).metadata?.organization;
              }

              return additionalClaims;
            },
          }),
        ]
      : []),

    // Sprint 3: API Key plugin for programmatic access
    ...(process.env.API_KEY_ENABLED === 'true'
      ? [
          apiKey([
            {
              configId: 'user-keys',
              references: 'user',
              enableMetadata: true,
              defaultPrefix: process.env.API_KEY_USER_PREFIX || 'user_',
              defaultKeyLength: parsePositiveInt(
                process.env.API_KEY_DEFAULT_LENGTH,
                64,
              ),
              apiKeyHeaders: ['x-api-key'],
              rateLimit: {
                enabled: process.env.API_KEY_RATE_LIMIT_ENABLED !== 'false',
                timeWindow: parsePositiveInt(
                  process.env.API_KEY_RATE_LIMIT_WINDOW_MS,
                  86_400_000,
                ),
                maxRequests: parsePositiveInt(
                  process.env.API_KEY_RATE_LIMIT_MAX,
                  1_000,
                ),
              },
            },
            {
              configId: 'org-keys',
              references: 'organization',
              enableMetadata: true,
              defaultPrefix: process.env.API_KEY_ORG_PREFIX || 'org_',
              defaultKeyLength: parsePositiveInt(
                process.env.API_KEY_DEFAULT_LENGTH,
                64,
              ),
              apiKeyHeaders: ['x-api-key'],
              rateLimit: {
                enabled: process.env.API_KEY_RATE_LIMIT_ENABLED !== 'false',
                timeWindow: parsePositiveInt(
                  process.env.API_KEY_RATE_LIMIT_WINDOW_MS,
                  86_400_000,
                ),
                maxRequests: parsePositiveInt(
                  process.env.API_KEY_RATE_LIMIT_MAX,
                  1_000,
                ),
              },
            },
          ]),
        ]
      : []),

    // Sprint 3: Bearer token authentication plugin
    ...(process.env.BEARER_TOKEN_ENABLED === 'true' ? [bearer()] : []),

    // Sprint 3: Admin plugin for administrative functions
    ...(process.env.ADMIN_ENABLED === 'true'
      ? [
          admin({
            adminUserIds: process.env.ADMIN_USER_IDS
              ? process.env.ADMIN_USER_IDS.split(',').map((id) => id.trim())
              : [],
            adminRoles: process.env.ADMIN_ROLES
              ? process.env.ADMIN_ROLES.split(',').map((role) => role.trim())
              : ['admin', 'superadmin'],
            // Custom hook to automatically assign admin role to pre-approved emails
            onUserCreate({ user }: { user: AdminUser }) {
              const adminEmails = process.env.ADMIN_DEFAULT_EMAILS
                ? process.env.ADMIN_DEFAULT_EMAILS.split(',').map((email) =>
                    email.trim().toLowerCase(),
                  )
                : [];

              if (
                user?.email &&
                adminEmails.includes(user.email.toLowerCase())
              ) {
                console.log(`🔐 Auto-assigning admin role to: ${user.email}`);
                // Note: The user will have admin privileges based on their email
                // The admin plugin will handle this through its built-in logic
                return true;
              }
              return false;
            },
          }),
        ]
      : []),

    ...(process.env.EMAIL_OTP_ENABLED === 'true'
      ? [
          emailOTP({
            otpLength: parsePositiveInt(process.env.EMAIL_OTP_LENGTH, 6),
            expiresIn: parsePositiveInt(process.env.EMAIL_OTP_EXPIRES_IN, 300),
            allowedAttempts: parsePositiveInt(
              process.env.EMAIL_OTP_ALLOWED_ATTEMPTS,
              3,
            ),
            sendVerificationOnSignUp:
              process.env.EMAIL_OTP_SEND_ON_SIGNUP === 'true',
            disableSignUp: process.env.EMAIL_OTP_DISABLE_SIGNUP === 'true',
            async sendVerificationOTP({
              email,
              otp,
              type,
            }: {
              email: string;
              otp: string;
              type: 'sign-in' | 'email-verification' | 'forget-password';
            }) {
              const companyName = process.env.RESEND_FROM_NAME || 'ID-Knuten';
              const supportEmail =
                process.env.RESEND_SUPPORT_EMAIL || 'support@id-knuten.no';

              const { subject, html, text } = generateOtpTemplate({
                userEmail: email,
                otp,
                type,
                companyName,
                supportEmail,
                expiresInMinutes: Math.floor(
                  parsePositiveInt(process.env.EMAIL_OTP_EXPIRES_IN, 300) / 60,
                ),
              });

              await resend.emails.send({
                from: `${process.env.RESEND_FROM_NAME} <${process.env.RESEND_FROM_EMAIL}>`,
                to: email,
                subject,
                html,
                text,
              });
            },
          }),
        ]
      : []),

    // Two-Factor Authentication Plugin
    ...(process.env.TWO_FACTOR_ENABLED !== 'false'
      ? [
          twoFactor({
            issuer: process.env.TOTP_ISSUER || 'ID-Knuten', // App name for TOTP authenticator apps
            skipVerificationOnEnable: false, // Require verification when enabling 2FA
            totpOptions: {
              digits: parsePositiveInt(process.env.TOTP_DIGITS, 6) as 6 | 8,
              period: parsePositiveInt(process.env.TOTP_PERIOD, 30),
            },
            otpOptions: {
              period: Math.floor(
                parsePositiveInt(process.env.OTP_EXPIRES_IN, 300) / 60,
              ), // Convert seconds to minutes
              async sendOTP({ user, otp }) {
                console.log('🔐 Sending 2FA OTP to:', user.email);
                const companyName = process.env.RESEND_FROM_NAME || 'ID-Knuten';
                const supportEmail =
                  process.env.RESEND_SUPPORT_EMAIL || 'support@id-knuten.no';

                const { subject, html, text } = generateOtpTemplate({
                  userEmail: user.email,
                  userName: user.name,
                  otp,
                  type: 'sign-in', // Use sign-in type for 2FA
                  companyName,
                  supportEmail,
                  expiresInMinutes: Math.floor(
                    parsePositiveInt(process.env.OTP_EXPIRES_IN, 300) / 60,
                  ),
                });

                await resend.emails.send({
                  from: `${process.env.RESEND_FROM_NAME} <${process.env.RESEND_FROM_EMAIL}>`,
                  to: user.email,
                  subject,
                  html,
                  text,
                });
              },
            },
            backupCodeOptions: {
              amount: parsePositiveInt(process.env.BACKUP_CODES_AMOUNT, 10),
              length: parsePositiveInt(process.env.BACKUP_CODES_LENGTH, 10),
            },
          }),
        ]
      : []),

    // Phone Number Plugin
    ...(process.env.PHONE_AUTH_ENABLED === 'true'
      ? [
          phoneNumber({
            otpLength: parsePositiveInt(process.env.OTP_LENGTH, 6),
            expiresIn: parsePositiveInt(process.env.OTP_EXPIRES_IN, 300),
            allowedAttempts: parsePositiveInt(
              process.env.PHONE_OTP_ALLOWED_ATTEMPTS,
              3,
            ), // 3 attempts before OTP is invalidated
            requireVerification:
              process.env.PHONE_REQUIRE_VERIFICATION === 'true', // Allow unverified phone logins initially
            async sendOTP({ phoneNumber, code }) {
              console.log('📱 Sending SMS OTP to:', phoneNumber);
              try {
                await smsService.sendOtp(
                  phoneNumber,
                  code,
                  'phone-verification',
                );
              } catch (error) {
                console.error('❌ Failed to send SMS OTP:', error);
                throw error;
              }
            },
            phoneNumberValidator: (phoneNumber: string) => {
              try {
                return smsService.validatePhoneNumber(phoneNumber);
              } catch {
                // Fallback validation
                const phoneRegex = /^\+[1-9]\d{1,14}$/;
                return phoneRegex.test(phoneNumber);
              }
            },
            async sendPasswordResetOTP({ phoneNumber, code }) {
              console.log('🔒 Sending password reset OTP to:', phoneNumber);
              try {
                await smsService.sendOtp(phoneNumber, code, 'password-reset');
              } catch (error) {
                console.error('❌ Failed to send password reset SMS:', error);
                throw error;
              }
            },
            signUpOnVerification:
              process.env.PHONE_SIGNUP_ON_VERIFICATION === 'true'
                ? {
                    getTempEmail: (phoneNumber: string) => {
                      // Generate temporary email for phone-only signups
                      const sanitized = phoneNumber.replace(/\D/g, '');
                      return `${sanitized}@temp.id-knuten.no`;
                    },
                    getTempName: (phoneNumber: string) => {
                      return `User ${phoneNumber}`;
                    },
                  }
                : undefined,
            callbackOnVerification: async ({ phoneNumber, user }) => {
              console.log(
                `✅ Phone number ${phoneNumber} verified for user:`,
                user.id,
              );
              // Optional: Additional verification logic
              await Promise.resolve(); // Ensure this is properly async
            },
          }),
        ]
      : []),

    // Sprint 2: Have I Been Pwned plugin for compromised password detection
    ...(process.env.HIBP_ENABLED === 'true'
      ? [
          haveIBeenPwned({
            customPasswordCompromisedMessage:
              process.env.HIBP_CUSTOM_MESSAGE ||
              'Your password has been found in a data breach. Please choose a more secure password.',
          }),
        ]
      : []),

    // Passkey plugin for passwordless authentication
    passkey({
      rpID:
        process.env.NODE_ENV === 'development'
          ? 'localhost'
          : process.env.PASSKEY_RP_ID || 'idknuten.no',
      rpName: process.env.PASSKEY_RP_NAME || 'ID-Knuten',
      origin:
        process.env.NODE_ENV === 'development'
          ? 'http://localhost:3000'
          : process.env.PASSKEY_ORIGIN || 'https://idknuten.no',
      authenticatorSelection: {
        authenticatorAttachment: undefined, // Allow both platform and cross-platform
        residentKey: 'preferred', // Encourage credential storage
        userVerification: 'preferred', // Encourage biometric verification
      },
    }),

    // User service integration plugin for syncing user data
    userServiceIntegrationPlugin(),

    // Organization events plugin for publishing organization lifecycle events to NATS
    ...(process.env.ORGANIZATION_ENABLED === 'true'
      ? [organizationEventsPlugin()]
      : []),
  ],
  socialProviders: {
    // Environment-driven social provider configuration.
    //
    // G29 (velion-gap.md): every social provider that we want to keep
    // refreshable on the back-end (so user-core's Graph enrichment + the
    // /internal/oauth/refresh endpoint can keep working past the 1h
    // access-token expiry) must request a refresh-token-issuing scope at
    // sign-in. For Microsoft Entra that means `offline_access`; for Google
    // it means `accessType: 'offline'`. Without these, refresh attempts
    // silently fail with `no_refresh_token`.
    ...(process.env.GOOGLE_CLIENT_ID && process.env.GOOGLE_CLIENT_SECRET
      ? {
          google: {
            clientId: process.env.GOOGLE_CLIENT_ID,
            clientSecret: process.env.GOOGLE_CLIENT_SECRET,
            scope: ['openid', 'profile', 'email'],
            // Required for refresh tokens. With 'online' (default) Google
            // never returns a refresh_token, so /internal/oauth/refresh would
            // always return no_refresh_token for Google sign-ins.
            accessType: 'offline',
          },
        }
      : {}),
    // Microsoft Entra ID (Azure AD) provider for enterprise SSO
    ...(process.env.MICROSOFT_CLIENT_ID && process.env.MICROSOFT_CLIENT_SECRET
      ? {
          microsoft: {
            clientId: process.env.MICROSOFT_CLIENT_ID,
            clientSecret: process.env.MICROSOFT_CLIENT_SECRET,
            tenantId: process.env.MICROSOFT_TENANT_ID || 'common', // 'common' for multi-tenant, specific tenant ID for single-tenant
            // `offline_access` is required for refresh tokens.
            // `User.Read` lets user-core call Microsoft Graph `/me` and
            // `/me/photo/$value` for the zero-input enrichment described in
            // docs/zero-input-enterprise-onboarding-roadmap.md (Phase 2).
            scope: [
              'openid',
              'profile',
              'email',
              'offline_access',
              'User.Read',
            ],
          },
        }
      : {}),
    ...(process.env.APPLE_CLIENT_ID
      ? {
          apple: appleSocialProviderConfig,
        }
      : {}),
  },
});
