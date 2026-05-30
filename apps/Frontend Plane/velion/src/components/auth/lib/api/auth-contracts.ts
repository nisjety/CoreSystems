import { z } from 'zod';

// Enhanced schemas for authentication providers
const AuthProviderSchemas = {
  // SSO schemas
  ssoDiscovery: z.object({
    email: z.string().email(),
    domain: z.string().optional(),
  }),

  ssoProvider: z.object({
    id: z.string(),
    name: z.string(),
    domain: z.string(),
    logoUrl: z.string().url().optional(),
    isEnabled: z.boolean(),
    loginUrl: z.string().url(),
    description: z.string().optional(),
    supportedFeatures: z.array(z.string()),
  }),

  ssoDiscoveryResponse: z.object({
    success: z.boolean(),
    providers: z.array(z.object({
      id: z.string(),
      name: z.string(),
      domain: z.string(),
      logoUrl: z.string().url().optional(),
      isEnabled: z.boolean(),
      loginUrl: z.string().url(),
      description: z.string().optional(),
      supportedFeatures: z.array(z.string()),
    })),
    domain: z.string(),
    message: z.string().optional(),
  }),

  ssoAuthenticate: z.object({
    email: z.string().email(),
    providerId: z.string(),
    redirectTo: z.string().url().optional(),
  }),

  ssoAuthResponse: z.object({
    success: z.boolean(),
    redirectUrl: z.string().url().optional(),
    message: z.string(),
  }),

  // Organization schemas
  organization: z.object({
    id: z.string(),
    name: z.string(),
    slug: z.string(),
    logo: z.string().url().optional(),
    domain: z.string().optional(),
    role: z.enum(['owner', 'admin', 'member']),
    memberCount: z.number(),
    createdAt: z.string(),
    isActive: z.boolean().optional(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  }),

  createOrganization: z.object({
    name: z.string().min(2),
    slug: z.string().min(2),
    domain: z.string().optional(),
  }),

  organizationMember: z.object({
    id: z.string(),
    userId: z.string(),
    user: z.object({
      id: z.string(),
      name: z.string().optional(),
      email: z.string().email(),
      image: z.string().url().optional(),
    }),
    role: z.enum(['owner', 'admin', 'member']),
    createdAt: z.string(),
  }),

  organizationInvitation: z.object({
    id: z.string(),
    email: z.string().email(),
    role: z.enum(['admin', 'member']),
    status: z.enum(['pending', 'expired', 'accepted', 'declined']),
    createdAt: z.string(),
    expiresAt: z.string(),
  }),

  createInvitation: z.object({
    email: z.string().email(),
    role: z.enum(['admin', 'member']),
    organizationId: z.string(),
  }),

  // Passkey schemas
  passkeyCredential: z.object({
    id: z.string(),
    name: z.string(),
    created: z.string(),
    lastUsed: z.string(),
  }),

  passkeyRegister: z.object({
    email: z.string().email(),
    displayName: z.string().optional(),
  }),

  passkeyAuthenticate: z.object({
    email: z.string().email().optional(),
  }),

  passkeyResponse: z.object({
    success: z.boolean(),
    credential: z.object({
      id: z.string(),
      name: z.string(),
      created: z.string(),
      lastUsed: z.string(),
    }).optional(),
    message: z.string(),
  }),

  // Social provider schemas
  socialProvider: z.object({
    name: z.string(),
    isEnabled: z.boolean(),
    authUrl: z.string().url().optional(),
  }),

  socialAuth: z.object({
    provider: z.enum(['google', 'microsoft']),
    redirectTo: z.string().url().optional(),
  }),

  socialAuthResponse: z.object({
    success: z.boolean(),
    redirectUrl: z.string().url().optional(),
    message: z.string(),
  }),

  // Email validation schemas
  emailValidation: z.object({
    email: z.string().email(),
  }),

  emailValidationResponse: z.object({
    available: z.boolean(),
    valid: z.boolean(),
    isBusinessEmail: z.boolean(),
    domain: z.string(),
    message: z.string().optional(),
  }),

  // ===================================
  // VERIFICATION SCHEMAS
  // ===================================
  
  // TOTP/Authenticator schemas
  totpSetup: z.object({
    userId: z.string().optional(),
  }),
  
  totpSetupResponse: z.object({
    success: z.boolean(),
    secret: z.string().optional(),
    qrCode: z.string().optional(), // Base64 QR code
    backupCodes: z.array(z.string()).optional(),
    message: z.string(),
  }),
  
  totpVerify: z.object({
    code: z.string().min(6).max(8),
    userId: z.string().optional(),
  }),
  
  totpVerifyResponse: z.object({
    success: z.boolean(),
    verified: z.boolean(),
    remaining: z.number().optional(), // Remaining attempts
    message: z.string(),
  }),
  
  // Email OTP schemas
  emailOtpSend: z.object({
    email: z.string().email(),
    purpose: z.enum(['verification', 'login', 'reset', 'change-email']).default('verification'),
    language: z.enum(['no', 'en']).default('no'),
  }),
  
  emailOtpSendResponse: z.object({
    success: z.boolean(),
    sent: z.boolean(),
    expiresAt: z.string(), // ISO string
    message: z.string(),
  }),
  
  emailOtpVerify: z.object({
    email: z.string().email(),
    code: z.string().min(4).max(8),
    purpose: z.enum(['verification', 'login', 'reset', 'change-email']).default('verification'),
  }),
  
  emailOtpVerifyResponse: z.object({
    success: z.boolean(),
    verified: z.boolean(),
    remaining: z.number().optional(),
    message: z.string(),
  }),
  
  // SMS OTP schemas
  smsOtpSend: z.object({
    phoneNumber: z.string().min(8),
    purpose: z.enum(['verification', 'login', 'reset']).default('verification'),
    language: z.enum(['no', 'en']).default('no'),
  }),
  
  smsOtpSendResponse: z.object({
    success: z.boolean(),
    sent: z.boolean(),
    maskedNumber: z.string().optional(), // e.g., "+47***1234"
    expiresAt: z.string(), // ISO string
    message: z.string(),
  }),
  
  smsOtpVerify: z.object({
    phoneNumber: z.string().min(8),
    code: z.string().min(4).max(8),
    purpose: z.enum(['verification', 'login', 'reset']).default('verification'),
  }),
  
  smsOtpVerifyResponse: z.object({
    success: z.boolean(),
    verified: z.boolean(),
    remaining: z.number().optional(),
    message: z.string(),
  }),
  
  // 2FA Management schemas
  twoFactorMethod: z.object({
    id: z.string(),
    type: z.enum(['totp', 'sms', 'email', 'passkey', 'backup-codes']),
    name: z.string(),
    isEnabled: z.boolean(),
    isPrimary: z.boolean(),
    lastUsed: z.string().optional(), // ISO string
    createdAt: z.string(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  }),
  
  twoFactorMethodsResponse: z.object({
    methods: z.array(z.object({
      id: z.string(),
      type: z.enum(['totp', 'sms', 'email', 'passkey', 'backup-codes']),
      name: z.string(),
      isEnabled: z.boolean(),
      isPrimary: z.boolean(),
      lastUsed: z.string().optional(), // ISO string
      createdAt: z.string(),
      metadata: z.record(z.string(), z.unknown()).optional(),
    })),
    total: z.number(),
  }),
  
  twoFactorToggle: z.object({
    methodId: z.string(),
    enabled: z.boolean(),
  }),
  
  twoFactorToggleResponse: z.object({
    success: z.boolean(),
    method: z.object({
      id: z.string(),
      type: z.string(),
      isEnabled: z.boolean(),
    }),
    message: z.string(),
  }),
  
  // Recovery codes schemas
  recoveryCodesResponse: z.object({
    codes: z.array(z.string()),
    remaining: z.number(),
    lastGenerated: z.string().optional(), // ISO string
  }),
  
  recoveryCodesRegenerate: z.object({
    userId: z.string().optional(),
    confirmRegenerate: z.boolean(),
  }),
  
  recoveryCodesRegenerateResponse: z.object({
    success: z.boolean(),
    codes: z.array(z.string()),
    message: z.string(),
  }),
  
  recoveryCodeVerify: z.object({
    code: z.string().min(8),
    userId: z.string().optional(),
  }),
  
  recoveryCodeVerifyResponse: z.object({
    success: z.boolean(),
    verified: z.boolean(),
    remaining: z.number(),
    message: z.string(),
  }),
  
  // Security audit schemas
  securityAuditEvent: z.object({
    id: z.string(),
    type: z.string(),
    description: z.string(),
    timestamp: z.string(), // ISO string
    ipAddress: z.string().optional(),
    userAgent: z.string().optional(),
    location: z.string().optional(),
    success: z.boolean(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  }),
  
  securityAuditResponse: z.object({
    events: z.array(z.object({
      id: z.string(),
      type: z.string(),
      description: z.string(),
      timestamp: z.string(), // ISO string
      ipAddress: z.string().optional(),
      userAgent: z.string().optional(),
      location: z.string().optional(),
      success: z.boolean(),
      metadata: z.record(z.string(), z.unknown()).optional(),
    })),
    total: z.number(),
    hasMore: z.boolean(),
  }),

  // ===================================
  // PROFILE MANAGEMENT SCHEMAS
  // ===================================
  
  userProfile: z.object({
    id: z.string(),
    email: z.string().email(),
    name: z.string(),
    image: z.string().url().optional(),
    emailVerified: z.boolean(),
    createdAt: z.string(),
    updatedAt: z.string(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  }),

  updateProfile: z.object({
    name: z.string().min(1).optional(),
    image: z.string().url().optional(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  }),

  profileUpdateResponse: z.object({
    success: z.boolean(),
    user: z.object({
      id: z.string(),
      email: z.string().email(),
      name: z.string(),
      image: z.string().url().optional(),
      emailVerified: z.boolean(),
      createdAt: z.string(),
      updatedAt: z.string(),
      metadata: z.record(z.string(), z.unknown()).optional(),
    }),
    message: z.string(),
  }),

  // ===================================
  // GDPR CONSENT SCHEMAS
  // ===================================
  
  consentPreferences: z.object({
    necessary: z.boolean(),
    analytics: z.boolean(),
    marketing: z.boolean(),
    performance: z.boolean().optional(),
    functional: z.boolean().optional(),
  }),

  consentResponse: z.object({
    necessary: z.boolean(),
    analytics: z.boolean(),
    marketing: z.boolean(),
    performance: z.boolean().optional(),
    functional: z.boolean().optional(),
    lastUpdated: z.string().optional(),
    version: z.string().optional(),
  }),

  updateConsent: z.object({
    necessary: z.boolean(),
    analytics: z.boolean(),
    marketing: z.boolean(),
    performance: z.boolean().optional(),
    functional: z.boolean().optional(),
  }),

  consentUpdateResponse: z.object({
    success: z.boolean(),
    consent: z.object({
      necessary: z.boolean(),
      analytics: z.boolean(),
      marketing: z.boolean(),
      performance: z.boolean().optional(),
      functional: z.boolean().optional(),
      lastUpdated: z.string(),
      version: z.string(),
    }),
    message: z.string(),
  }),

  consentWithdrawResponse: z.object({
    success: z.boolean(),
    message: z.string(),
  }),

  // ===================================
  // PASSWORD STRENGTH SCHEMAS
  // ===================================
  
  passwordStrengthCheck: z.object({
    password: z.string(),
  }),

  passwordStrengthResponse: z.object({
    isStrong: z.boolean(),
    isCompromised: z.boolean(),
    score: z.number().min(0).max(4), // 0-4 scale
    feedback: z.array(z.string()),
    suggestions: z.array(z.string()).optional(),
    estimatedCrackTime: z.string().optional(),
  }),
} as const;

// Export schemas for reuse
export type SSOProvider = z.infer<typeof AuthProviderSchemas.ssoProvider>;
export type Organization = z.infer<typeof AuthProviderSchemas.organization>;
export type OrganizationMember = z.infer<typeof AuthProviderSchemas.organizationMember>;
export type OrganizationInvitation = z.infer<typeof AuthProviderSchemas.organizationInvitation>;
export type PasskeyCredential = z.infer<typeof AuthProviderSchemas.passkeyCredential>;
export type SocialProvider = z.infer<typeof AuthProviderSchemas.socialProvider>;
export type EmailValidationResponse = z.infer<typeof AuthProviderSchemas.emailValidationResponse>;

// Verification type exports
export type TotpSetupResponse = z.infer<typeof AuthProviderSchemas.totpSetupResponse>;
export type TotpVerifyResponse = z.infer<typeof AuthProviderSchemas.totpVerifyResponse>;
export type EmailOtpSendResponse = z.infer<typeof AuthProviderSchemas.emailOtpSendResponse>;
export type EmailOtpVerifyResponse = z.infer<typeof AuthProviderSchemas.emailOtpVerifyResponse>;
export type SmsOtpSendResponse = z.infer<typeof AuthProviderSchemas.smsOtpSendResponse>;
export type SmsOtpVerifyResponse = z.infer<typeof AuthProviderSchemas.smsOtpVerifyResponse>;
type TwoFactorMethod = z.infer<typeof AuthProviderSchemas.twoFactorMethod>;
export type TwoFactorMethodsResponse = z.infer<typeof AuthProviderSchemas.twoFactorMethodsResponse>;
export type TwoFactorToggleResponse = z.infer<typeof AuthProviderSchemas.twoFactorToggleResponse>;
export type RecoveryCodesResponse = z.infer<typeof AuthProviderSchemas.recoveryCodesResponse>;
export type RecoveryCodesRegenerateResponse = z.infer<typeof AuthProviderSchemas.recoveryCodesRegenerateResponse>;
export type RecoveryCodeVerifyResponse = z.infer<typeof AuthProviderSchemas.recoveryCodeVerifyResponse>;
type SecurityAuditEvent = z.infer<typeof AuthProviderSchemas.securityAuditEvent>;
export type SecurityAuditResponse = z.infer<typeof AuthProviderSchemas.securityAuditResponse>;

// Profile management type exports
export type UserProfile = z.infer<typeof AuthProviderSchemas.userProfile>;
export type UpdateProfile = z.infer<typeof AuthProviderSchemas.updateProfile>;
export type ProfileUpdateResponse = z.infer<typeof AuthProviderSchemas.profileUpdateResponse>;

// GDPR consent type exports
type ConsentPreferences = z.infer<typeof AuthProviderSchemas.consentPreferences>;
export type ConsentResponse = z.infer<typeof AuthProviderSchemas.consentResponse>;
export type UpdateConsent = z.infer<typeof AuthProviderSchemas.updateConsent>;
export type ConsentUpdateResponse = z.infer<typeof AuthProviderSchemas.consentUpdateResponse>;
export type ConsentWithdrawResponse = z.infer<typeof AuthProviderSchemas.consentWithdrawResponse>;

// Password strength type exports
export type PasswordStrengthCheck = z.infer<typeof AuthProviderSchemas.passwordStrengthCheck>;
export type PasswordStrengthResponse = z.infer<typeof AuthProviderSchemas.passwordStrengthResponse>;
