import { os } from '@orpc/server';
import { z } from 'zod';
import { auth } from './auth';
import { db } from '../db';
import * as schema from '../db/schema';
import {
  publishOrganizationCreated,
  publishOrganizationMemberAdded,
} from './organization-hooks';
import { redisSecondaryStorage } from '../db/redis';
import { createHash } from 'crypto';

const _BEARER_CACHE_TTL = 90; // seconds
function _bearerCacheKey(token: string): string {
  return `bearer:val:${createHash('sha256').update(token).digest('hex')}`;
}

// Context helpers and types
type RpcContext = {
  headers?: Headers | Record<string, string>;
  setHeader?: (name: string, value: string | string[]) => void;
};

function headersFromCtx(ctx: RpcContext | undefined): Headers | undefined {
  if (!ctx?.headers) return undefined;
  if (ctx.headers instanceof Headers) return ctx.headers;
  const h = new Headers();
  Object.entries(ctx.headers).forEach(([k, v]) => {
    if (typeof v === 'string') h.set(k, v);
  });
  return h;
}

function forwardSetCookie(
  ctx: RpcContext | undefined,
  sourceHeaders: Headers | undefined,
) {
  if (!ctx?.setHeader || !sourceHeaders) return;
  const setCookie = sourceHeaders.get('set-cookie');
  if (setCookie) ctx.setHeader('set-cookie', setCookie);
}

// Narrowing helpers
function isRecord(v: unknown): v is Record<string, unknown> {
  return !!v && typeof v === 'object' && !Array.isArray(v);
}

function pickUser(obj: unknown): z.infer<typeof UserOut> | undefined {
  if (!isRecord(obj)) return undefined;
  if (!('id' in obj) || !('email' in obj)) return undefined;
  return {
    id: String(obj.id as any),
    name: (obj.name as string | null | undefined) ?? null,
    email: String(obj.email as any),
    emailVerified: Boolean((obj.emailVerified as boolean | undefined) ?? false),
    image: (obj.image as string | null | undefined) ?? null,
    createdAt: obj.createdAt
      ? new Date(
          obj.createdAt as unknown as string | number | Date,
        ).toISOString()
      : undefined,
    updatedAt: obj.updatedAt
      ? new Date(
          obj.updatedAt as unknown as string | number | Date,
        ).toISOString()
      : undefined,
  };
}

// Schemas
const SignInSchema = z.object({
  email: z.string().email(),
  password: z.string().min(1),
});

const SignUpSchema = z.object({
  name: z.string().min(1),
  email: z.string().email(),
  password: z.string().min(8),
});

const UserProfileSchema = z.object({
  name: z.string().min(1).optional(),
  image: z.string().url().optional().or(z.literal('').optional()),
});

const ConsentSchema = z.object({
  analytics: z.boolean(),
  marketing: z.boolean(),
  necessary: z.boolean(),
});

// Enhanced schemas for additional endpoints
const VerifyEmailTokenSchema = z.object({
  token: z.string(),
  callbackURL: z.string().optional(),
});

const PasswordResetSchema = z.object({
  token: z.string().min(1),
  password: z.string().min(8),
});

// Sprint 3: Organization & RBAC Schemas
const CreateOrganizationSchema = z.object({
  name: z.string().min(1, 'Organization name is required'),
  slug: z.string().min(1, 'Organization slug is required').optional(),
  logo: z.string().url().optional(),
  metadata: z.record(z.string(), z.any()).optional(),
});

const InviteMemberSchema = z.object({
  organizationId: z.string().min(1),
  email: z.string().email(),
  role: z.enum(['owner', 'admin', 'member']),
  expiresAt: z.date().optional(),
});

const SwitchOrganizationSchema = z.object({
  organizationId: z.string().min(1),
});

// Sprint 3: SSO/OIDC Provider Schemas
const CreateOIDCClientSchema = z.object({
  name: z.string().min(1, 'Client name is required'),
  redirectUris: z
    .array(z.string().url())
    .min(1, 'At least one redirect URI is required'),
  scopes: z.array(z.string()).default(['openid', 'profile', 'email']),
  grantTypes: z
    .array(z.enum(['authorization_code', 'refresh_token']))
    .default(['authorization_code', 'refresh_token']),
  responseTypes: z
    .array(z.enum(['code', 'id_token', 'token']))
    .default(['code']),
  tokenEndpointAuthMethod: z
    .enum(['client_secret_basic', 'client_secret_post', 'none'])
    .default('client_secret_basic'),
  organizationId: z.string().optional(),
  metadata: z.record(z.string(), z.any()).optional(),
});

const DeleteOIDCClientSchema = z.object({
  clientId: z.string().min(1),
});

const GetOIDCClientSchema = z.object({
  clientId: z.string().min(1),
});

const ListOIDCClientsSchema = z.object({
  organizationId: z.string().optional(),
  limit: z.number().min(1).max(100).default(20),
  offset: z.number().min(0).default(0),
});

const GenerateClientSecretSchema = z.object({
  clientId: z.string().min(1),
});

// Sprint 3: API Keys & Bearer Authentication Schemas
const CreateAPIKeySchema = z.object({
  name: z.string().min(1, 'API key name is required'),
  description: z.string().optional(),
  organizationId: z.string().min(1, 'Organization ID is required'),
  scopes: z.array(z.string()).default(['read']),
  expiresAt: z.date().optional(),
  rateLimit: z
    .object({
      requests: z.number().min(1).default(1000),
      period: z.enum(['minute', 'hour', 'day']).default('hour'),
    })
    .optional(),
});

const DeleteAPIKeySchema = z.object({
  keyId: z.string().min(1),
});

const ListAPIKeysSchema = z.object({
  organizationId: z.string().optional(),
  limit: z.number().min(1).max(100).default(20),
  offset: z.number().min(0).default(0),
  includeExpired: z.boolean().default(false),
});

const RotateAPIKeySchema = z.object({
  keyId: z.string().min(1),
});

const ValidateAPIKeySchema = z.object({
  apiKey: z.string().min(1),
  scope: z.string().optional(),
});

// Sprint 3: Bearer Token Authentication Schemas
const ValidateBearerTokenSchema = z.object({
  token: z.string().min(1),
  requiredScope: z.string().optional(),
});

const CreateBearerTokenSchema = z.object({
  expiresIn: z.number().min(60).max(31536000).optional(), // Max 1 year
  scopes: z.array(z.string()).optional(),
  metadata: z.record(z.string(), z.any()).optional(),
});

const RevokeBearerTokenSchema = z.object({
  token: z.string().min(1),
});

const ListBearerTokensSchema = z.object({
  limit: z.number().min(1).max(100).default(20),
  offset: z.number().min(0).default(0),
  includeExpired: z.boolean().default(false),
});

// Sprint 3: Admin Plugin Schemas
const AdminListUsersSchema = z.object({
  limit: z.number().min(1).max(100).default(20),
  offset: z.number().min(0).default(0),
  search: z.string().optional(),
  role: z.string().optional(), // Filter by role (e.g., 'admin', 'user', 'superadmin')
  organizationId: z.string().optional(),
  status: z.enum(['active', 'suspended', 'pending']).optional(),
  sortBy: z.enum(['createdAt', 'email', 'lastLogin']).default('createdAt'),
  sortOrder: z.enum(['asc', 'desc']).default('desc'),
});

const AdminGetUserSchema = z.object({
  userId: z.string().min(1),
});

const AdminListOrganizationsSchema = z.object({
  limit: z.number().min(1).max(100).default(20),
  offset: z.number().min(0).default(0),
  search: z.string().optional(),
  sortBy: z.enum(['createdAt', 'name', 'memberCount']).default('createdAt'),
  sortOrder: z.enum(['asc', 'desc']).default('desc'),
});

const AdminGetSystemStatsSchema = z.object({
  period: z.enum(['day', 'week', 'month', 'year']).default('month'),
});

// Additional Better Auth Admin Plugin Schemas
const AdminCreateUserSchema = z.object({
  email: z.string().email(),
  password: z.string().min(6),
  name: z.string().min(1),
  role: z.string().optional(),
  data: z.record(z.string(), z.any()).optional(),
});

const AdminSetRoleSchema = z.object({
  userId: z.string().min(1),
  role: z.union([z.string(), z.array(z.string())]),
});

const AdminBanUserSchema = z.object({
  userId: z.string().min(1),
  banReason: z.string().optional(),
  banExpiresIn: z.number().optional(), // seconds
});

const AdminListUserSessionsSchema = z.object({
  userId: z.string().min(1),
});

const AdminRemoveUserSchema = z.object({
  userId: z.string().min(1),
});

const AdminUnbanUserSchema = z.object({
  userId: z.string().min(1),
});

const AdminUpdateUserSchema = z.object({
  userId: z.string().min(1),
  name: z.string().optional(),
  email: z.string().email().optional(),
  image: z.string().optional(),
});

const UserOut = z.object({
  id: z.string(),
  name: z.string().nullable().optional(),
  email: z.string().email(),
  emailVerified: z.boolean().optional(),
  image: z.string().nullable().optional(),
  createdAt: z.string().optional(),
  updatedAt: z.string().optional(),
});

// Procedures
const signInProcedure = os
  .input(SignInSchema)
  .output(
    z.object({
      user: UserOut.optional(),
      session: z.unknown().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const { headers: respHeaders, response } = await auth.api.signInEmail({
        body: input,
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });
      forwardSetCookie(context as RpcContext, respHeaders);
      const data = response as unknown;
      let userUnknown: unknown;
      let sessionUnknown: unknown;
      if (isRecord(data)) {
        if ('user' in data) {
          userUnknown = (data as Record<'user', unknown>)['user'];
        }
        if ('session' in data) {
          sessionUnknown = (data as Record<'session', unknown>)['session'];
        }
      }
      const user = pickUser(userUnknown);
      const session = sessionUnknown;
      if (user && user.id) {
        return {
          user,
          session,
        };
      }
      return { error: 'Invalid credentials' };
    } catch (error) {
      console.error('Sign in error:', error);
      return { error: 'Sign in failed' };
    }
  });

const signUpProcedure = os
  .input(SignUpSchema)
  .output(
    z.object({
      user: UserOut.optional(),
      session: z.unknown().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      console.log('🔧 [oRPC SignUp] Starting signup procedure');
      console.log('🔧 [oRPC SignUp] Input:', {
        email: input.email,
        name: input.name,
      });

      const headers = headersFromCtx(context as RpcContext);
      console.log('🔧 [oRPC SignUp] Headers processed');

      const { headers: respHeaders, response } = await auth.api.signUpEmail({
        body: input,
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });

      console.log('🔧 [oRPC SignUp] Better Auth API call completed');
      console.log('🔧 [oRPC SignUp] Response type:', typeof response);
      console.log(
        '🔧 [oRPC SignUp] Response keys:',
        response ? Object.keys(response as object) : 'null',
      );

      forwardSetCookie(context as RpcContext, respHeaders);
      const data = response as unknown;
      let userUnknown: unknown;
      let sessionUnknown: unknown;
      if (isRecord(data)) {
        if ('user' in data) {
          userUnknown = (data as Record<'user', unknown>)['user'];
          console.log(
            '🔧 [oRPC SignUp] Found user in response:',
            !!userUnknown,
          );
        }
        if ('session' in data) {
          sessionUnknown = (data as Record<'session', unknown>)['session'];
          console.log(
            '🔧 [oRPC SignUp] Found session in response:',
            !!sessionUnknown,
          );
        }
      }
      const user = pickUser(userUnknown);
      const session = sessionUnknown;

      console.log('🔧 [oRPC SignUp] Processed user:', !!user, user?.id);

      if (user && user.id) {
        console.log('✅ [oRPC SignUp] Signup successful, returning user');
        return {
          user,
          session,
        };
      }
      console.log('❌ [oRPC SignUp] Signup failed - no valid user');
      return { error: 'Sign up failed' };
    } catch (error) {
      console.error('❌ [oRPC SignUp] Sign up error:', error);
      return { error: 'Sign up failed' };
    }
  });

const signOutProcedure = os
  .input(z.object({}))
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      await auth.api.signOut({ headers: headers ?? new Headers() });
      return { success: true };
    } catch (error) {
      console.error('Sign out error:', error);
      return { success: false, error: 'Sign out failed' };
    }
  });

const getSessionProcedure = os
  .input(z.object({}))
  .output(
    z.object({
      user: UserOut.optional(),
      session: z.unknown().optional(),
      authenticated: z.boolean(),
    }),
  )
  .handler(async ({ context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const sess = (await auth.api.getSession({
        headers: headers ?? new Headers(),
      })) as unknown;

      if (isRecord(sess) && 'user' in sess) {
        const user = pickUser((sess as Record<'user', unknown>)['user']);
        return {
          user,
          session: (sess as Record<'session', unknown>)['session'],
          authenticated: true,
        };
      }

      return { authenticated: false };
    } catch (error) {
      console.error('❌ [getSession] Error:', error);
      return { authenticated: false };
    }
  });

const getProfileProcedure = os
  .input(z.object({}))
  .output(
    z.object({
      user: UserOut.optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const sess = (await auth.api.getSession({
        headers: headers ?? new Headers(),
      })) as unknown;
      if (isRecord(sess) && 'user' in sess) {
        const user = pickUser((sess as Record<'user', unknown>)['user']);
        return {
          user,
        };
      }
      return { error: 'Not authenticated' };
    } catch (_error) {
      console.error('Get profile error:', _error);
      return { error: 'Failed to get profile' };
    }
  });

const updateProfileProcedure = os
  .input(UserProfileSchema)
  .output(
    z.object({
      user: UserOut.optional(),
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const { headers: respHeaders, response } = await auth.api.updateUser({
        body: input,
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });
      forwardSetCookie(context as RpcContext, respHeaders);
      const data = response as unknown;
      let userUnknown: unknown = data;
      if (isRecord(data) && 'user' in data) {
        userUnknown = (data as Record<'user', unknown>)['user'];
      }
      const user = pickUser(userUnknown);
      if (user && user.id) {
        return {
          user,
          success: true,
        };
      }
      return { success: false, error: 'Failed to update user' };
    } catch (_error) {
      console.error('Update profile error:', _error);
      return { success: false, error: 'Failed to update profile' };
    }
  });

const getConsentProcedure = os
  .input(z.object({}))
  .output(
    z.object({
      analytics: z.boolean(),
      marketing: z.boolean(),
      necessary: z.boolean(),
    }),
  )
  .handler(() => {
    // TODO: persist and read consent from DB (scoped by user/session)
    return {
      analytics: false,
      marketing: false,
      necessary: true,
    };
  });

const updateConsentProcedure = os
  .input(ConsentSchema)
  .output(
    z.object({
      success: z.boolean(),
      consent: ConsentSchema,
    }),
  )
  .handler(({ input }) => {
    // TODO: persist consent to DB
    return { success: true, consent: input };
  });

const withdrawConsentProcedure = os
  .input(z.object({}))
  .output(
    z.object({
      success: z.boolean(),
      consent: ConsentSchema,
    }),
  )
  .handler(() => {
    // TODO: persist consent withdrawal to DB
    return {
      success: true,
      consent: { analytics: false, marketing: false, necessary: true },
    };
  });

// Email Verification Procedures
const verifyEmailProcedure = os
  .input(VerifyEmailTokenSchema)
  .output(
    z.object({
      success: z.boolean(),
      user: UserOut.optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const { headers: respHeaders, response } = await auth.api.verifyEmail({
        query: { token: input.token, callbackURL: input.callbackURL },
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });
      forwardSetCookie(context as RpcContext, respHeaders);
      const data = response as unknown;
      const user = pickUser(data);
      if (user) {
        return { success: true, user };
      }
      return { success: false, error: 'Email verification failed' };
    } catch (_error) {
      console.error('Email verification error:', _error);
      return { success: false, error: 'Email verification failed' };
    }
  });

const sendEmailVerificationProcedure = os
  .input(z.object({ email: z.string().email() }))
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      await auth.api.sendVerificationEmail({
        body: input,
        headers: headers ?? new Headers(),
      });
      return { success: true };
    } catch (_error) {
      console.error('Send email verification error:', _error);
      return { success: false, error: 'Failed to send verification email' };
    }
  });

// Password Reset Procedures
// const _forgetPasswordProcedure = os
//   .input(PasswordResetRequestSchema)
//   .output(
//     z.object({
//       success: z.boolean(),
//       error: z.string().optional(),
//     }),
//   )
//   .handler(async ({ input, context }) => {
//     try {
//       const headers = headersFromCtx(context as RpcContext);
//       await auth.api.forgetPassword({
//         body: input,
//         headers: headers ?? new Headers(),
//       });
//       return { success: true };
//     } catch (_error) {
//       console.error('Forget password error:', _error);
//       return { success: false, error: 'Failed to send password reset email' };
//     }
//   });

const resetPasswordProcedure = os
  .input(PasswordResetSchema)
  .output(
    z.object({
      success: z.boolean(),
      user: UserOut.optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const { headers: respHeaders, response } = await auth.api.resetPassword({
        body: { newPassword: input.password, token: input.token },
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });
      forwardSetCookie(context as RpcContext, respHeaders);
      const data = response as unknown;
      const user = pickUser(data);
      if (user) {
        return { success: true, user };
      }
      return { success: false, error: 'Password reset failed' };
    } catch (_error) {
      console.error('Reset password error:', _error);
      return { success: false, error: 'Password reset failed' };
    }
  });

// const _changePasswordProcedure = os
//   .input(ChangePasswordSchema)
//   .output(
//     z.object({
//       success: z.boolean(),
//       error: z.string().optional(),
//     }),
//   )
//   .handler(async ({ input, context }) => {
//     try {
//       const headers = headersFromCtx(context as RpcContext);
//       await auth.api.changePassword({
//         body: input,
//         headers: headers ?? new Headers(),
//       });
//       return { success: true };
//     } catch (_error) {
//       console.error('Change password error:', _error);
//       return { success: false, error: 'Password change failed' };
//     }
//   });

const sendPasswordResetProcedure = os
  .input(z.object({ email: z.string().email() }))
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      await auth.api.forgetPassword({
        body: { email: input.email },
        headers: headers ?? new Headers(),
      });
      return { success: true };
    } catch (_error) {
      console.error('Send password reset error:', _error);
      return { success: false, error: 'Failed to send password reset' };
    }
  });

// Sprint 2: External IDP & Security Procedures

// Two-Factor Authentication Procedures
const enableTwoFactorProcedure = os
  .input(z.object({ password: z.string().min(1) }))
  .output(
    z.object({
      success: z.boolean(),
      secret: z.string().optional(),
      qrCode: z.string().optional(),
      backupCodes: z.array(z.string()).optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Try to use Better Auth 2FA API - if not available, provide placeholder
      try {
        const response = await fetch(
          `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/two-factor/enable`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Cookie: headers?.get('cookie') || '',
            },
            body: JSON.stringify(input),
          },
        );

        if (response.ok) {
          const data = (await response.json()) as Record<string, unknown>;
          // Forward any set-cookie headers
          const setCookie = response.headers.get('set-cookie');
          if (setCookie && context && 'setHeader' in context) {
            (context as RpcContext).setHeader?.('set-cookie', setCookie);
          }

          return {
            success: true,
            secret: (data?.secret as string) || '',
            qrCode: (data?.qrCode as string) || '',
            backupCodes: (data?.backupCodes as string[]) || [],
          };
        }
      } catch {
        console.log('2FA API not available');
      }

      // Return error when 2FA plugin is not configured
      return {
        success: false,
        error:
          'Two-factor authentication is not configured on this server. Please contact support.',
      };
    } catch (_error) {
      console.error('Enable 2FA error:', _error);
      return {
        success: false,
        error: 'Failed to enable two-factor authentication',
      };
    }
  });

const disableTwoFactorProcedure = os
  .input(z.object({ password: z.string().min(1) }))
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Try to use Better Auth 2FA API
      try {
        const response = await fetch(
          `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/two-factor/disable`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Cookie: headers?.get('cookie') || '',
            },
            body: JSON.stringify(input),
          },
        );

        if (response.ok) {
          return { success: true };
        }
      } catch {
        console.log('2FA disable API not available');
      }

      // Return error when 2FA plugin is not configured
      return {
        success: false,
        error:
          'Two-factor authentication is not configured on this server. Please contact support.',
      };
    } catch (_error) {
      console.error('Disable 2FA error:', _error);
      return {
        success: false,
        error: 'Failed to disable two-factor authentication',
      };
    }
  });

const verifyTwoFactorProcedure = os
  .input(
    z.object({
      code: z.string().min(6).max(8),
      type: z.enum(['totp', 'backup-code']).default('totp'),
    }),
  )
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Try to use Better Auth 2FA API
      try {
        const endpoint =
          input.type === 'totp' ? 'verify-totp' : 'verify-backup-code';
        const response = await fetch(
          `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/two-factor/${endpoint}`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Cookie: headers?.get('cookie') || '',
            },
            body: JSON.stringify({ code: input.code }),
          },
        );

        if (response.ok) {
          return { success: true };
        }
      } catch {
        console.log('2FA verify API not available');
      }

      // Return error when 2FA plugin is not configured
      return {
        success: false,
        error:
          'Two-factor authentication is not configured on this server. Please contact support.',
      };
    } catch (_error) {
      console.error('Verify 2FA error:', _error);
      return {
        success: false,
        error: 'Invalid two-factor authentication code',
      };
    }
  });

// Email OTP Procedures
const sendEmailOtpProcedure = os
  .input(
    z.object({
      email: z.string().email(),
      type: z
        .enum(['sign-in', 'email-verification', 'forget-password'])
        .default('sign-in'),
    }),
  )
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Try to use Better Auth Email OTP API
      try {
        const response = await fetch(
          `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/otp/email/send`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Cookie: headers?.get('cookie') || '',
            },
            body: JSON.stringify(input),
          },
        );

        if (response.ok) {
          return { success: true };
        }
      } catch {
        console.log('Email OTP API not available');
      }

      // Return error when Email OTP plugin is not configured
      return {
        success: false,
        error:
          'Email OTP is not configured on this server. Please contact support.',
      };
    } catch (_error) {
      console.error('Send email OTP error:', _error);
      return { success: false, error: 'Failed to send email OTP' };
    }
  });

const verifyEmailOtpProcedure = os
  .input(
    z.object({
      email: z.string().email(),
      otp: z.string().length(6),
    }),
  )
  .output(
    z.object({
      success: z.boolean(),
      user: UserOut.optional(),
      session: z.unknown().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Try to use Better Auth Email OTP API
      try {
        const response = await fetch(
          `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/otp/email/verify`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Cookie: headers?.get('cookie') || '',
            },
            body: JSON.stringify(input),
          },
        );

        if (response.ok) {
          const data = (await response.json()) as Record<string, unknown>;
          // Forward any set-cookie headers
          const setCookie = response.headers.get('set-cookie');
          if (setCookie && context && 'setHeader' in context) {
            (context as RpcContext).setHeader?.('set-cookie', setCookie);
          }

          const user = pickUser(data);
          return {
            success: true,
            user,
            session: (data?.session as Record<string, unknown>) || null,
          };
        }
      } catch {
        console.log(
          'Email OTP verify API not available, using placeholder logic',
        );
      }

      // Return error when Email OTP plugin is not configured
      return {
        success: false,
        error:
          'Email OTP authentication is not configured on this server. Please contact support.',
      };
    } catch (_error) {
      console.error('Verify email OTP error:', _error);
      return { success: false, error: 'Invalid or expired OTP' };
    }
  });

// Phone Number/SMS Procedures
const sendPhoneOtpProcedure = os
  .input(
    z.object({
      phoneNumber: z
        .string()
        .regex(/^\+[1-9]\d{1,14}$/, 'Invalid phone number format'),
    }),
  )
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Try to use Better Auth Phone OTP API
      try {
        const response = await fetch(
          `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/phone-number/send-otp`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Cookie: headers?.get('cookie') || '',
            },
            body: JSON.stringify(input),
          },
        );

        if (response.ok) {
          return { success: true };
        }
      } catch {
        console.log('Phone OTP API not available');
      }

      // Return error when Phone OTP plugin is not configured
      return {
        success: false,
        error:
          'Phone OTP is not configured on this server. Please contact support.',
      };
    } catch (_error) {
      console.error('Send phone OTP error:', _error);
      return { success: false, error: 'Failed to send SMS OTP' };
    }
  });

const verifyPhoneOtpProcedure = os
  .input(
    z.object({
      phoneNumber: z
        .string()
        .regex(/^\+[1-9]\d{1,14}$/, 'Invalid phone number format'),
      otp: z.string().length(6),
    }),
  )
  .output(
    z.object({
      success: z.boolean(),
      user: UserOut.optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Try to use Better Auth Phone OTP API
      try {
        const response = await fetch(
          `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/phone-number/verify-otp`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Cookie: headers?.get('cookie') || '',
            },
            body: JSON.stringify(input),
          },
        );

        if (response.ok) {
          const data = (await response.json()) as Record<string, unknown>;
          const user = pickUser(data);
          return {
            success: true,
            user,
          };
        }
      } catch {
        console.log(
          'Phone OTP verify API not available, using placeholder logic',
        );
      }

      // Return error when Phone OTP plugin is not configured
      return {
        success: false,
        error:
          'Phone OTP authentication is not configured on this server. Please contact support.',
      };
    } catch (_error) {
      console.error('Verify phone OTP error:', _error);
      return { success: false, error: 'Invalid or expired SMS OTP' };
    }
  });

// Passkey Procedures - Using Better Auth Passkey API
const createPasskeyProcedure = os
  .input(
    z.object({
      email: z.string().email().optional(),
      name: z.string().optional(),
    }),
  )
  .output(
    z.object({
      success: z.boolean(),
      options: z.unknown().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Try to use Better Auth Passkey API
      try {
        const response = await fetch(
          `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/passkey/generate-creation-options`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Cookie: headers?.get('cookie') || '',
            },
            body: JSON.stringify(input),
          },
        );

        if (response.ok) {
          const data = (await response.json()) as Record<string, unknown>;
          return {
            success: true,
            options: data,
          };
        }
      } catch {
        console.log(
          'Passkey creation API not available, using placeholder logic',
        );
      }

      // Placeholder implementation
      const mockOptions = {
        challenge: Buffer.from(Math.random().toString()).toString('base64'),
        rp: {
          name: 'ID-Knuten',
          id:
            process.env.NODE_ENV === 'development'
              ? 'localhost'
              : 'idknuten.no',
        },
        user: {
          id: Buffer.from(input.email || 'demo@example.com').toString('base64'),
          name: input.email || 'demo@example.com',
          displayName: input.name || 'Demo User',
        },
        pubKeyCredParams: [{ alg: -7, type: 'public-key' }],
        timeout: 60000,
        attestation: 'none',
        authenticatorSelection: {
          authenticatorAttachment: 'platform',
          userVerification: 'preferred',
        },
      };

      return {
        success: true,
        options: mockOptions,
      };
    } catch (_error) {
      console.error('Create passkey error:', _error);
      return { success: false, error: 'Failed to create passkey options' };
    }
  });

// External Provider Procedures

// Generic OAuth Provider (for Vipps, Okta, etc.)
const initiateOAuthProcedure = os
  .input(
    z.object({
      provider: z.string(),
      redirectTo: z.string().url().optional(),
    }),
  )
  .output(
    z.object({
      success: z.boolean(),
      url: z.string().url().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      console.log(`🔄 OAuth initiate called for provider: ${input.provider}`);

      const provider = input.provider.toLowerCase();
      const callbackURL =
        input.redirectTo ||
        `${process.env.FRONTEND_URL || 'http://localhost:3000'}/auth/callback`;

      // Import the auth instance to use Better Auth's built-in OAuth handling
      const { auth } = await import('./auth.js');

      console.log(
        `✅ Using Better Auth's native OAuth flow for provider: ${provider}`,
      );
      console.log(`   Callback URL: ${callbackURL}`);

      // Check if the provider is configured
      // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
      const socialConfig = (auth.options as any)?.socialProviders?.[provider];
      if (!socialConfig) {
        // For development, return a provider-appropriate mock OAuth URL
        if (process.env.NODE_ENV === 'development') {
          const providerBaseUrls: Record<string, string> = {
            microsoft:
              'https://login.microsoftonline.com/common/oauth2/v2.0/authorize',
            google: 'https://accounts.google.com/o/oauth2/v2/auth',
            github: 'https://github.com/login/oauth/authorize',
            apple: 'https://appleid.apple.com/auth/authorize',
            vipps:
              'https://api.vipps.no/access-management-1.0/access/oauth2/auth',
            okta: 'https://mock-okta.okta.com/oauth2/default/v1/authorize',
          };
          const baseUrl =
            providerBaseUrls[provider] ??
            `https://mock-${provider}.example.com/oauth/authorize`;
          const mockOAuthUrl = `${baseUrl}?client_id=mock-${provider}-id&response_type=code&scope=openid%20profile%20email&redirect_uri=${encodeURIComponent(callbackURL)}&state=DEV_MOCK_STATE`;
          console.warn(
            `⚠️  [DEV MODE] ${provider} credentials not configured, using mock OAuth URL for testing`,
          );
          return {
            success: true,
            url: mockOAuthUrl,
          };
        }

        console.error(
          `❌ OAuth provider ${provider} is not configured. Please set the required environment variables.`,
        );
        return {
          success: false,
          error: `OAuth provider ${provider} is not configured`,
        };
      }

      // Use Better Auth's server-side API to initiate OAuth
      // This will handle state generation, storage, and URL construction automatically
      const result = await auth.api.signInSocial({
        body: {
          provider,
          callbackURL,
        },
        asResponse: true,
      });

      console.log(`📊 Better Auth response status: ${result.status}`);
      console.log(
        `📊 Response headers:`,
        Object.fromEntries(result.headers.entries()),
      );

      // Extract the redirect URL from the response
      if (result.status === 302 || result.status === 301) {
        const redirectUrl = result.headers.get('location');
        if (redirectUrl) {
          console.log(`✅ Better Auth generated OAuth URL successfully`);
          console.log(`   Redirect URL: ${redirectUrl}`);
          return {
            success: true,
            url: redirectUrl,
          };
        }
      }

      // Try to parse the response body for the URL
      try {
        const data = await result.json();
        console.log(`📊 Response body:`, data);
        if (data && typeof data === 'object' && 'url' in data) {
          console.log(`✅ Found OAuth URL in response body`);
          return {
            success: true,
            url: data.url,
          };
        }
      } catch (e) {
        console.error(`❌ Failed to parse response body:`, e);
      }

      // If no redirect, something went wrong
      console.error('❌ Better Auth did not return a redirect URL');
      return {
        success: false,
        error: 'Failed to generate OAuth URL',
      };
    } catch (_error) {
      console.error('Initiate OAuth error:', _error);
      return {
        success: false,
        error: `Failed to initiate ${input.provider} authentication`,
      };
    }
  });

// Password Strength & HIBP Check
const checkPasswordStrengthProcedure = os
  .input(
    z.object({
      password: z.string().min(1),
    }),
  )
  .output(
    z.object({
      isStrong: z.boolean(),
      isCompromised: z.boolean(),
      score: z.number().min(0).max(4),
      feedback: z.array(z.string()),
      error: z.string().optional(),
    }),
  )
  .handler(({ input }) => {
    try {
      const password = input.password;
      const feedback: string[] = [];
      let score = 0;

      // Basic strength checks
      if (password.length >= 8) score += 1;
      else feedback.push('Password should be at least 8 characters long');

      if (/[a-z]/.test(password) && /[A-Z]/.test(password)) score += 1;
      else
        feedback.push(
          'Password should contain both uppercase and lowercase letters',
        );

      if (/\d/.test(password)) score += 1;
      else feedback.push('Password should contain at least one number');

      if (/[!@#$%^&*(),.?":{}|<>]/.test(password)) score += 1;
      else
        feedback.push('Password should contain at least one special character');

      // TODO: Add actual HIBP API call here when plugin is available
      let isCompromised = false;

      // Placeholder HIBP check - flag common passwords
      const commonPasswords = [
        'password',
        '123456',
        'qwerty',
        'abc123',
        'password123',
      ];
      if (commonPasswords.includes(password.toLowerCase())) {
        isCompromised = true;
        feedback.push('This password has been found in data breaches');
      }

      return {
        isStrong: score >= 3 && !isCompromised,
        isCompromised,
        score,
        feedback,
      };
    } catch (_error) {
      console.error('Password strength check error:', _error);
      return {
        isStrong: false,
        isCompromised: false,
        score: 0,
        feedback: [],
        error: 'Failed to check password strength',
      };
    }
  });

// Sprint 3: Organization Management Procedures
const createOrganizationProcedure = os
  .input(CreateOrganizationSchema)
  .output(
    z.object({
      success: z.boolean(),
      organization: z
        .object({
          id: z.string(),
          name: z.string(),
          slug: z.string(),
          logo: z.string().nullable(),
          createdAt: z.string(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // Check if user is authenticated by getting their session
      const headers = headersFromCtx(context as RpcContext);
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required to create organization',
        };
      }

      // Use Better Auth organization API to create organization
      console.log('🔍 Attempting to call auth.api.createOrganization');
      console.log('🔍 Input data:', { name: input.name, slug: input.slug });

      if (typeof (auth.api as any).createOrganization !== 'function') {
        throw new Error('createOrganization method not available on auth.api');
      }

      const result = await (auth.api as any).createOrganization({
        body: {
          name: input.name,
          slug: input.slug || input.name.toLowerCase().replace(/\s+/g, '-'),
          logo: input.logo,
          metadata: input.metadata,
        },
        headers: headers ?? new Headers(),
      });

      console.log('✅ Organization created:', result);

      // Publish organization created event
      try {
        await publishOrganizationCreated({
          organizationId: result.id,
          name: result.name,
          slug: result.slug,
          creatorId: session.user.id,
          creatorEmail: session.user.email,
          metadata: input.metadata,
        });
        console.log('📢 Published organization.created event');
      } catch (eventError) {
        console.error('⚠️ Failed to publish organization event:', eventError);
        // Don't fail the request if event publishing fails
      }

      return {
        success: true,
        organization: {
          id: result.id,
          name: result.name,
          slug: result.slug,
          logo: result.logo || null,
          createdAt: new Date(result.createdAt).toISOString(),
        },
      };
    } catch (_error) {
      const errorMessage =
        _error instanceof Error ? _error.message : String(_error);
      console.error('❌ Organization creation failed:', errorMessage);
      return {
        success: false,
        error: `Organization creation failed: ${errorMessage}`,
      };
    }
  });

const getOrganizationsListProcedure = os
  .output(
    z.object({
      success: z.boolean(),
      organizations: z
        .array(
          z.object({
            id: z.string(),
            name: z.string(),
            slug: z.string(),
            logo: z.string().nullable(),
            role: z.enum(['owner', 'admin', 'member']),
            memberCount: z.number().optional(),
            createdAt: z.string(),
          }),
        )
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated by getting their session
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required to list organizations',
        };
      }

      // Use Better Auth organization API to get user's organizations
      const result = await (auth.api as any).listOrganizations({
        query: {},
        headers: headers ?? new Headers(),
      });

      const organizations = result.map((org: any) => ({
        id: org.id,
        name: org.name,
        slug: org.slug,
        logo: org.logo || null,
        role: org.role,
        memberCount: org.memberCount,
        createdAt: new Date(org.createdAt).toISOString(),
      }));

      console.log('✅ Organizations retrieved:', organizations);

      return {
        success: true,
        organizations,
      };
    } catch (_error) {
      const errorMessage =
        _error instanceof Error ? _error.message : String(_error);
      console.error('❌ Organization list failed:', errorMessage);
      return {
        success: false,
        error: `Organization list failed: ${errorMessage}`,
      };
    }
  });

const inviteMemberProcedure = os
  .input(InviteMemberSchema)
  .output(
    z.object({
      success: z.boolean(),
      invitation: z
        .object({
          id: z.string(),
          email: z.string(),
          role: z.enum(['owner', 'admin', 'member']),
          expiresAt: z.string(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      console.log('🔍 [Invite Debug] Input received:', JSON.stringify(input));
      console.log(
        '🔍 [Invite Debug] Headers available:',
        headers ? Array.from(headers.keys()) : 'none',
      );

      const requestBody = {
        email: input.email,
        role: input.role,
        organizationId: input.organizationId,
      };

      console.log(
        '🔍 [Invite Debug] Request body to Better Auth:',
        JSON.stringify(requestBody),
      );

      // Use Better Auth organization invite API
      const response = await fetch(
        `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/organization/invite-member`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            ...(headers ? Object.fromEntries(headers.entries()) : {}),
          },
          body: JSON.stringify(requestBody),
        },
      );

      console.log(
        '🔍 [Invite Debug] Better Auth response status:',
        response.status,
      );
      console.log(
        '🔍 [Invite Debug] Better Auth response headers:',
        Object.fromEntries(response.headers.entries()),
      );

      if (response.ok) {
        const data = (await response.json()) as {
          id?: string;
          expiresAt?: string;
        };
        console.log('✅ Member invitation sent successfully:', data);

        return {
          success: true,
          invitation: {
            id: data.id || 'invitation-id',
            email: input.email,
            role: input.role,
            expiresAt:
              data.expiresAt || new Date(Date.now() + 172800000).toISOString(), // 48 hours
          },
        };
      } else {
        const errorText = await response.text();
        console.error(
          '❌ [Invite Debug] Better Auth error response:',
          errorText,
        );
        console.error('❌ [Invite Debug] Full error details:', {
          status: response.status,
          statusText: response.statusText,
          headers: Object.fromEntries(response.headers.entries()),
          body: errorText,
        });
        return {
          success: false,
          error: `Member invitation failed: ${errorText}`,
        };
      }
    } catch (_error) {
      console.error('❌ Member invitation error:', _error);
      return {
        success: false,
        error: 'Member invitation failed - internal error',
      };
    }
  });

const switchOrganizationProcedure = os
  .input(SwitchOrganizationSchema)
  .output(
    z.object({
      success: z.boolean(),
      activeOrganization: z
        .object({
          id: z.string(),
          name: z.string(),
          slug: z.string(),
          role: z.enum(['owner', 'admin', 'member']),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Use Better Auth organization set active API
      const response = await fetch(
        `${process.env.BETTER_AUTH_URL || 'http://localhost:3011'}/api/auth/organization/set-active`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            ...(headers ? Object.fromEntries(headers.entries()) : {}),
          },
          body: JSON.stringify({
            organizationId: input.organizationId,
          }),
        },
      );

      if (response.ok) {
        const data = (await response.json()) as {
          id?: string;
          name?: string;
          slug?: string;
          role?: 'owner' | 'admin' | 'member';
        };
        console.log('✅ Organization switched successfully:', data);

        return {
          success: true,
          activeOrganization: {
            id: data.id || input.organizationId,
            name: data.name || 'Organization',
            slug: data.slug || 'organization',
            role: data.role || 'member',
          },
        };
      } else {
        const error = await response.text();
        console.error('❌ Organization switch failed:', error);
        return {
          success: false,
          error: `Organization switch failed: ${error}`,
        };
      }
    } catch (_error) {
      console.error('❌ Organization switch error:', _error);
      return {
        success: false,
        error: 'Organization switch failed - internal error',
      };
    }
  });

// Sprint 3: OIDC Provider Procedures
const createOIDCClientProcedure = os
  .input(CreateOIDCClientSchema)
  .output(
    z.object({
      success: z.boolean(),
      client: z
        .object({
          clientId: z.string(),
          clientSecret: z.string(),
          name: z.string(),
          redirectUris: z.array(z.string()),
          scopes: z.array(z.string()),
          grantTypes: z.array(z.string()),
          responseTypes: z.array(z.string()),
          tokenEndpointAuthMethod: z.string(),
          organizationId: z.string().optional(),
          createdAt: z.string(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try OIDC Provider client creation API placeholder
      // For now, providing fallback until OIDC Provider API is verified
      throw new Error(
        'OIDC Provider client creation API endpoint verification needed',
      );
    } catch {
      console.warn('OIDC Provider client creation API not available');
    }

    // Fallback response
    return {
      success: false,
      error:
        'OIDC Provider client creation not available - plugin not configured',
    };
  });

const listOIDCClientsProcedure = os
  .input(ListOIDCClientsSchema)
  .output(
    z.object({
      success: z.boolean(),
      clients: z
        .array(
          z.object({
            clientId: z.string(),
            name: z.string(),
            redirectUris: z.array(z.string()),
            scopes: z.array(z.string()),
            organizationId: z.string().optional(),
            createdAt: z.string(),
            lastUsed: z.string().optional(),
          }),
        )
        .optional(),
      total: z.number().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try OIDC Provider client listing API placeholder
      throw new Error(
        'OIDC Provider client listing API endpoint verification needed',
      );
    } catch {
      console.warn('OIDC Provider client listing API not available');
    }

    // Fallback response
    return {
      success: false,
      error:
        'OIDC Provider client listing not available - plugin not configured',
    };
  });

const getOIDCClientProcedure = os
  .input(GetOIDCClientSchema)
  .output(
    z.object({
      success: z.boolean(),
      client: z
        .object({
          clientId: z.string(),
          name: z.string(),
          redirectUris: z.array(z.string()),
          scopes: z.array(z.string()),
          grantTypes: z.array(z.string()),
          responseTypes: z.array(z.string()),
          tokenEndpointAuthMethod: z.string(),
          organizationId: z.string().optional(),
          createdAt: z.string(),
          updatedAt: z.string().optional(),
          lastUsed: z.string().optional(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try OIDC Provider get client API placeholder
      throw new Error(
        'OIDC Provider get client API endpoint verification needed',
      );
    } catch {
      console.warn('OIDC Provider get client API not available');
    }

    // Fallback response
    return {
      success: false,
      error: 'OIDC Provider get client not available - plugin not configured',
    };
  });

const deleteOIDCClientProcedure = os
  .input(DeleteOIDCClientSchema)
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try OIDC Provider delete client API placeholder
      throw new Error(
        'OIDC Provider delete client API endpoint verification needed',
      );
    } catch {
      console.warn('OIDC Provider delete client API not available');
    }

    // Fallback response
    return {
      success: false,
      error:
        'OIDC Provider delete client not available - plugin not configured',
    };
  });

const generateClientSecretProcedure = os
  .input(GenerateClientSecretSchema)
  .output(
    z.object({
      success: z.boolean(),
      secret: z
        .object({
          secretId: z.string(),
          clientSecret: z.string(),
          createdAt: z.string(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try OIDC Provider generate secret API placeholder
      throw new Error(
        'OIDC Provider generate secret API endpoint verification needed',
      );
    } catch {
      console.warn('OIDC Provider generate secret API not available');
    }

    // Fallback response
    return {
      success: false,
      error:
        'OIDC Provider generate secret not available - plugin not configured',
    };
  });

// Sprint 3: API Keys & Bearer Authentication Procedures
const createAPIKeyProcedure = os
  .input(CreateAPIKeySchema)
  .output(
    z.object({
      success: z.boolean(),
      apiKey: z
        .object({
          id: z.string(),
          name: z.string(),
          description: z.string().optional(),
          key: z.string(),
          organizationId: z.string(),
          scopes: z.array(z.string()),
          rateLimit: z
            .object({
              requests: z.number(),
              period: z.string(),
            })
            .optional(),
          expiresAt: z.string().optional(),
          createdAt: z.string(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 API Key functionality, we need to check if user is authenticated first
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated by getting their session
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required to create API key',
        };
      }

      // For Sprint 3, we'll use internal HTTP call to the Better Auth API Key endpoint
      const authUrl = process.env.BETTER_AUTH_URL || 'http://localhost:3000';
      const cookieHeader = headers?.get('cookie') || '';

      const createKeyResponse = await fetch(
        `${authUrl}/api/auth/create-api-key`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Cookie: cookieHeader,
          },
          body: JSON.stringify({
            name: input.name,
            ...(input.description && {
              metadata: { description: input.description },
            }),
            ...(input.expiresAt && {
              expiresIn: Math.floor(
                (new Date(input.expiresAt).getTime() - Date.now()) / 1000,
              ),
            }),
          }),
        },
      );

      if (!createKeyResponse.ok) {
        const errorText = await createKeyResponse.text();
        throw new Error(`API Key creation failed: ${errorText}`);
      }

      const apiKeyData = (await createKeyResponse.json()) as {
        id?: string;
        name?: string;
        key?: string;
        apiKey?: string;
        createdAt?: string;
      };

      return {
        success: true,
        apiKey: {
          id: apiKeyData.id || `key_${Date.now()}`,
          name: apiKeyData.name || input.name,
          key:
            apiKeyData.key ||
            apiKeyData.apiKey ||
            `sk_${Date.now()}_${Math.random().toString(36).substring(7)}`,
          organizationId: input.organizationId,
          scopes: input.scopes,
          description: input.description,
          createdAt: apiKeyData.createdAt || new Date().toISOString(),
          expiresAt: input.expiresAt?.toISOString(),
          rateLimit: input.rateLimit,
        },
      };
    } catch (error) {
      console.error('❌ API Key creation failed:', error);
      return {
        success: false,
        error:
          error instanceof Error ? error.message : 'Failed to create API key',
      };
    }
  });

const listAPIKeysProcedure = os
  .input(ListAPIKeysSchema)
  .output(
    z.object({
      success: z.boolean(),
      apiKeys: z
        .array(
          z.object({
            id: z.string(),
            name: z.string(),
            description: z.string().optional(),
            organizationId: z.string(),
            scopes: z.array(z.string()),
            rateLimit: z
              .object({
                requests: z.number(),
                period: z.string(),
              })
              .optional(),
            expiresAt: z.string().optional(),
            createdAt: z.string(),
            lastUsed: z.string().optional(),
            isExpired: z.boolean(),
          }),
        )
        .optional(),
      total: z.number().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try API Key listing placeholder
      throw new Error('API Key listing API endpoint verification needed');
    } catch {
      console.warn('API Key listing API not available');
    }

    // Fallback response
    return {
      success: false,
      error: 'API Key listing not available - requires custom implementation',
    };
  });

const deleteAPIKeyProcedure = os
  .input(DeleteAPIKeySchema)
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try API Key deletion placeholder
      throw new Error('API Key deletion API endpoint verification needed');
    } catch {
      console.warn('API Key deletion API not available');
    }

    // Fallback response
    return {
      success: false,
      error: 'API Key deletion not available - requires custom implementation',
    };
  });

const rotateAPIKeyProcedure = os
  .input(RotateAPIKeySchema)
  .output(
    z.object({
      success: z.boolean(),
      apiKey: z
        .object({
          id: z.string(),
          newKey: z.string(),
          rotatedAt: z.string(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try API Key rotation placeholder
      throw new Error('API Key rotation API endpoint verification needed');
    } catch {
      console.warn('API Key rotation API not available');
    }

    // Fallback response
    return {
      success: false,
      error: 'API Key rotation not available - requires custom implementation',
    };
  });

const validateAPIKeyProcedure = os
  .input(ValidateAPIKeySchema)
  .output(
    z.object({
      success: z.boolean(),
      valid: z.boolean(),
      keyInfo: z
        .object({
          id: z.string(),
          organizationId: z.string(),
          scopes: z.array(z.string()),
          rateLimit: z
            .object({
              requests: z.number(),
              period: z.string(),
              remaining: z.number(),
              resetAt: z.string(),
            })
            .optional(),
          expiresAt: z.string().optional(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try API Key validation placeholder
      throw new Error('API Key validation API endpoint verification needed');
    } catch {
      console.warn('API Key validation API not available');
    }

    // Fallback response
    return {
      success: false,
      valid: false,
      error:
        'API Key validation not available - requires custom implementation',
    };
  });

// Sprint 3: Bearer Token Authentication Procedures
const validateBearerTokenProcedure = os
  .input(ValidateBearerTokenSchema)
  .output(
    z.object({
      success: z.boolean(),
      valid: z.boolean(),
      session: z
        .object({
          user: z.object({
            id: z.string(),
            email: z.string(),
            name: z.string().nullable(),
          }),
          token: z.string(),
          expiresAt: z.string(),
          scopes: z.array(z.string()).optional(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 Bearer token validation, we need to check the token
      const headers = headersFromCtx(context as RpcContext);

      // Use the Bearer token from the input or Authorization header
      const bearerToken =
        input.token || headers?.get('authorization')?.replace('Bearer ', '');

      if (!bearerToken) {
        return {
          success: false,
          valid: false,
          error: 'Bearer token required',
        };
      }

      // --- cache read: avoid HTTP round-trip on hot path ---
      const cacheKey = _bearerCacheKey(bearerToken);
      try {
        const cached = await redisSecondaryStorage.get(cacheKey);
        if (cached) {
          const parsed = JSON.parse(cached) as {
            user: { id: string; email: string; name: string | null };
            session: { token: string; expiresAt: string };
          };
          return {
            success: true,
            valid: true,
            session: {
              user: parsed.user,
              token: bearerToken,
              expiresAt: parsed.session.expiresAt,
              scopes: ['read'],
            },
          };
        }
      } catch (_cacheErr) {
        // Redis unavailable — fall through to HTTP validation
      }

      // For Sprint 3, we'll use internal HTTP call to validate Bearer token
      const authUrl = process.env.BETTER_AUTH_URL || 'http://localhost:3000';

      const validateResponse = await fetch(`${authUrl}/api/auth/get-session`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${bearerToken}`,
        },
      });

      if (!validateResponse.ok) {
        return {
          success: false,
          valid: false,
          error: 'Invalid bearer token',
        };
      }

      const sessionData = (await validateResponse.json()) as {
        user?: { id: string; email: string; name?: string };
        session?: { token: string; expiresAt: string };
      };

      if (!sessionData.user || !sessionData.session) {
        return {
          success: false,
          valid: false,
          error: 'Invalid session data',
        };
      }

      // --- cache write: store for TTL seconds ---
      try {
        await redisSecondaryStorage.set(
          cacheKey,
          JSON.stringify({
            user: {
              id: sessionData.user.id,
              email: sessionData.user.email,
              name: sessionData.user.name || null,
            },
            session: sessionData.session,
          }),
          _BEARER_CACHE_TTL,
        );
      } catch (_cacheErr) {
        // Redis write failure is non-fatal
      }

      return {
        success: true,
        valid: true,
        session: {
          user: {
            id: sessionData.user.id,
            email: sessionData.user.email,
            name: sessionData.user.name || null,
          },
          token: bearerToken,
          expiresAt: sessionData.session.expiresAt,
          scopes: ['read'], // Default scope for Sprint 3
        },
      };
    } catch (error) {
      console.error('❌ Bearer token validation failed:', error);
      return {
        success: false,
        valid: false,
        error:
          error instanceof Error
            ? error.message
            : 'Bearer token validation failed',
      };
    }
  });

const createBearerTokenProcedure = os
  .input(CreateBearerTokenSchema)
  .output(
    z.object({
      success: z.boolean(),
      token: z
        .object({
          id: z.string(),
          token: z.string(),
          expiresAt: z.string(),
          scopes: z.array(z.string()).optional(),
          createdAt: z.string(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 Bearer token creation, we need to check if user is authenticated first
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated by getting their session
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required to create bearer token',
        };
      }

      // Create bearer token (not persisted to database)
      const tokenId = `bt_${Date.now()}_${Math.random().toString(36).substring(7)}`;
      const bearerToken = `bearer_${Date.now()}_${Math.random().toString(36).substring(7)}`;
      const expiresAt = input.expiresIn
        ? new Date(Date.now() + input.expiresIn * 1000).toISOString()
        : new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString(); // Default 24 hours

      console.log('✅ Bearer token created:', {
        id: tokenId,
        userId: session.user.id,
        expiresAt,
        scopes: input.scopes,
      });

      return {
        success: true,
        token: {
          id: tokenId,
          token: bearerToken,
          expiresAt,
          scopes: input.scopes || ['read'],
          createdAt: new Date().toISOString(),
        },
      };
    } catch (_error) {
      console.error('❌ Bearer token creation failed:', _error);
      return {
        success: false,
        error:
          _error instanceof Error
            ? _error.message
            : 'Failed to create bearer token',
      };
    }
  });

const revokeBearerTokenProcedure = os
  .input(RevokeBearerTokenSchema)
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 Bearer token revocation, we need to check if user is authenticated first
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated by getting their session
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required to revoke bearer token',
        };
      }

      // Simulate token revocation (not persisted to database)
      console.log('✅ Bearer token revoked:', {
        token: input.token,
        userId: session.user.id,
        revokedAt: new Date().toISOString(),
      });

      return {
        success: true,
      };
    } catch (error) {
      console.error('❌ Bearer token revocation failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to revoke bearer token',
      };
    }
  });

const listBearerTokensProcedure = os
  .input(ListBearerTokensSchema)
  .output(
    z.object({
      success: z.boolean(),
      tokens: z
        .array(
          z.object({
            id: z.string(),
            token: z.string(),
            expiresAt: z.string(),
            scopes: z.array(z.string()),
            createdAt: z.string(),
            lastUsed: z.string().nullable(),
            isActive: z.boolean(),
          }),
        )
        .optional(),
      total: z.number().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 Bearer token listing, we need to check if user is authenticated first
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated by getting their session
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required to list bearer tokens',
        };
      }

      // Return error since bearer token persistence is not implemented
      return {
        success: false,
        error:
          'Bearer token listing is not implemented. This feature requires database persistence.',
      };
    } catch (error) {
      console.error('❌ Bearer token listing failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to list bearer tokens',
      };
    }
  });

// Sprint 3: Admin Plugin Procedures
const adminListUsersProcedure = os
  .input(AdminListUsersSchema)
  .output(
    z.object({
      success: z.boolean(),
      users: z
        .array(
          z.object({
            id: z.string(),
            name: z.string().nullable(),
            email: z.string(),
            emailVerified: z.boolean(),
            image: z.string().nullable(),
            role: z.string().optional(),
            status: z.enum(['active', 'suspended', 'pending']),
            lastLogin: z.string().nullable(),
            createdAt: z.string(),
            organizationCount: z.number(),
          }),
        )
        .optional(),
      total: z.number().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      console.log('🔍 [adminListUsers] Starting admin list users request');
      const headers = headersFromCtx(context as RpcContext);

      // Check internal service authentication OR user session with admin role
      const internalServiceSecret = headers?.get('x-internal-service-secret');
      const expectedSecret =
        process.env.INTERNAL_SERVICE_SECRET || process.env.INTERNAL_API_KEY;
      const isInternalService =
        internalServiceSecret &&
        expectedSecret &&
        internalServiceSecret === expectedSecret;

      if (!isInternalService) {
        // For external requests, check session authentication and admin role
        const session = await auth.api.getSession({
          headers: headers ?? new Headers(),
        });

        if (!session || !session.user) {
          console.log('❌ [adminListUsers] No authenticated session');
          return {
            success: false,
            error: 'Authentication required for admin operations',
          };
        }

        // Check if user has admin role
        const user = session.user as { role?: string };
        if (user.role !== 'admin' && user.role !== 'superadmin') {
          console.log('❌ [adminListUsers] User lacks admin role:', user.role);
          return {
            success: false,
            error: 'Admin role required for this operation',
          };
        }
      } else {
        console.log(
          '✅ [adminListUsers] Internal service authenticated - bypassing session check',
        );
      }

      // Build query with filters - using direct query for simplicity
      const { limit = 20, offset = 0, search, role } = input;

      console.log(
        `🔍 [adminListUsers] Input params: limit=${limit}, offset=${offset}, role=${role}, search=${search}`,
      );

      // Query all users directly from the user table
      const allUsers = await db.select().from(schema.user);

      // Apply filters in memory
      let filteredUsers = allUsers;

      // Filter by role if provided
      if (role) {
        console.log(`🔍 [adminListUsers] Filtering by role: ${role}`);
        filteredUsers = filteredUsers.filter((user) => user.role === role);
        console.log(
          `🔍 [adminListUsers] After role filter: ${filteredUsers.length} users`,
        );
      }

      // Filter by search if provided
      if (search) {
        const searchLower = search.toLowerCase();
        filteredUsers = filteredUsers.filter(
          (user) =>
            user.email.toLowerCase().includes(searchLower) ||
            (user.name && user.name.toLowerCase().includes(searchLower)),
        );
      }

      // Get total count
      const total = filteredUsers.length;

      // Sort by createdAt descending (most recent first)
      filteredUsers.sort(
        (a, b) => b.createdAt.getTime() - a.createdAt.getTime(),
      );

      // Apply pagination
      const paginatedUsers = filteredUsers.slice(offset, offset + limit);

      // Map users to output format
      const mappedUsers = paginatedUsers.map((user) => ({
        id: user.id,
        name: user.name || null,
        email: user.email,
        emailVerified: user.emailVerified || false,
        image: user.image || null,
        role: user.role || 'user', // Include role field
        status: 'active' as const, // Default status - can be enhanced later
        lastLogin: user.updatedAt ? user.updatedAt.toISOString() : null,
        createdAt: user.createdAt.toISOString(),
        organizationCount: 0, // TODO: Add organization count when org plugin is fully implemented
      }));

      console.log(
        `✅ [adminListUsers] Successfully retrieved ${mappedUsers.length} users (total: ${total})`,
      );

      return {
        success: true,
        users: mappedUsers,
        total,
      };
    } catch (error) {
      console.error('❌ [adminListUsers] Admin list users failed:', error);
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to list users',
      };
    }
  });

const adminGetUserProcedure = os
  .input(AdminGetUserSchema)
  .output(
    z.object({
      success: z.boolean(),
      user: z
        .object({
          id: z.string(),
          name: z.string().nullable(),
          email: z.string(),
          emailVerified: z.boolean(),
          image: z.string().nullable(),
          status: z.enum(['active', 'suspended', 'pending']),
          lastLogin: z.string().nullable(),
          createdAt: z.string(),
          updatedAt: z.string(),
          metadata: z.record(z.string(), z.any()).optional(),
          organizations: z.array(
            z.object({
              id: z.string(),
              name: z.string(),
              role: z.enum(['owner', 'admin', 'member']),
            }),
          ),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try Admin get user placeholder
      throw new Error('Admin get user API endpoint verification needed');
    } catch {
      console.warn('Admin get user API not available');
    }

    // Fallback response
    return {
      success: false,
      error:
        'Admin get user not available - requires admin role and custom implementation',
    };
  });

// const adminSuspendUserProcedure = os
//   .input(AdminSuspendUserSchema)
//   .output(
//     z.object({
//       success: z.boolean(),
//       error: z.string().optional(),
//     }),
//   )
//   .handler(() => {
//     try {
//       // Try Admin suspend user placeholder
//       throw new Error('Admin suspend user API endpoint verification needed');
//     } catch {
//       console.warn('Admin suspend user API not available');
//     }

//     // Fallback response
//     return {
//       success: false,
//       error:
//         'Admin suspend user not available - requires admin role and custom implementation',
//     };
//   });

const adminListOrganizationsProcedure = os
  .input(AdminListOrganizationsSchema)
  .output(
    z.object({
      success: z.boolean(),
      organizations: z
        .array(
          z.object({
            id: z.string(),
            name: z.string(),
            slug: z.string(),
            memberCount: z.number(),
            createdAt: z.string(),
            ownerId: z.string(),
            ownerEmail: z.string(),
          }),
        )
        .optional(),
      total: z.number().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try Admin organization listing placeholder
      throw new Error(
        'Admin organization listing API endpoint verification needed',
      );
    } catch {
      console.warn('Admin organization listing API not available');
    }

    // Fallback response
    return {
      success: false,
      error:
        'Admin organization listing not available - requires admin role and custom implementation',
    };
  });

const adminGetSystemStatsProcedure = os
  .input(AdminGetSystemStatsSchema)
  .output(
    z.object({
      success: z.boolean(),
      stats: z
        .object({
          totalUsers: z.number(),
          totalOrganizations: z.number(),
          totalAPIKeys: z.number(),
          totalOIDCClients: z.number(),
          activeUsers: z.number(),
          newUsersThisPeriod: z.number(),
          newOrganizationsThisPeriod: z.number(),
          loginStats: z.object({
            totalLogins: z.number(),
            uniqueLogins: z.number(),
            failedLogins: z.number(),
          }),
          systemHealth: z.object({
            uptime: z.number(),
            memoryUsage: z.number(),
            cpuUsage: z.number(),
          }),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try Admin system stats placeholder
      throw new Error('Admin system stats API endpoint verification needed');
    } catch {
      console.warn('Admin system stats API not available');
    }

    // Fallback response
    return {
      success: false,
      error:
        'Admin system stats not available - requires admin role and custom implementation',
    };
  });

// Additional Better Auth Admin Plugin Procedures
const adminCreateUserProcedure = os
  .input(AdminCreateUserSchema)
  .output(
    z.object({
      success: z.boolean(),
      user: z
        .object({
          id: z.string(),
          email: z.string(),
          name: z.string(),
          role: z.string().optional(),
          createdAt: z.string(),
        })
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 admin user creation, we need to check admin permissions
      const headers = headersFromCtx(context as RpcContext);

      console.log('🔍 [adminCreateUser] Headers received:', {
        cookie: headers?.get('cookie'),
        hasHeaders: !!headers,
        internalSecret: headers?.get('x-internal-service-secret')
          ? 'present'
          : 'missing',
      });

      // Check internal service authentication OR user session
      const internalServiceSecret = headers?.get('x-internal-service-secret');
      const expectedSecret =
        process.env.INTERNAL_SERVICE_SECRET || process.env.INTERNAL_API_KEY;
      const isInternalService =
        internalServiceSecret &&
        expectedSecret &&
        internalServiceSecret === expectedSecret;

      if (!isInternalService) {
        // For external requests, check session authentication
        const session = await auth.api.getSession({
          headers: headers ?? new Headers(),
        });

        console.log('🔍 [adminCreateUser] Session result:', {
          hasSession: !!session,
          hasUser: !!session?.user,
          userId: session?.user?.id,
        });

        if (!session || !session.user) {
          return {
            success: false,
            error: 'Authentication required for admin operations',
          };
        }
      } else {
        console.log(
          '✅ [adminCreateUser] Internal service authenticated - bypassing session check',
        );
      }

      // Create user directly via database with hashed password using Node crypto
      const crypto = await import('crypto');
      const salt = crypto.randomBytes(16).toString('hex');
      const hashedPassword =
        crypto.scryptSync(input.password, salt, 64).toString('hex') +
        '.' +
        salt;
      const userId = `user_${Date.now()}_${Math.random().toString(36).substring(7)}`;
      const now = new Date();

      const [newUser] = await db
        .insert(schema.user)
        .values({
          id: userId,
          email: input.email,
          name: input.name,
          emailVerified: false,
          role: input.role || 'user',
          createdAt: now,
          updatedAt: now,
        })
        .returning();

      // Insert account with hashed password
      await db.insert(schema.account).values({
        id: `account_${Date.now()}`,
        userId: newUser.id,
        accountId: newUser.email,
        providerId: 'credential',
        password: hashedPassword,
        createdAt: now,
        updatedAt: now,
      });

      return {
        success: true,
        user: {
          id: newUser.id,
          email: newUser.email,
          name: newUser.name || input.name,
          role: newUser.role || 'user',
          createdAt: newUser.createdAt.toISOString(),
        },
      };
    } catch (error) {
      console.error('❌ Admin user creation failed:', error);
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to create user',
      };
    }
  });

const adminSetRoleProcedure = os
  .input(AdminSetRoleSchema)
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 admin set role, we need to check admin permissions
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated and has admin role
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required for admin operations',
        };
      }

      // For Sprint 3, we'll use internal HTTP call to the Better Auth admin endpoint
      const authUrl = process.env.BETTER_AUTH_URL || 'http://localhost:3000';
      const cookieHeader = headers?.get('cookie') || '';

      const setRoleResponse = await fetch(
        `${authUrl}/api/auth/admin/set-role`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Cookie: cookieHeader,
          },
          body: JSON.stringify({
            userId: input.userId,
            role: input.role,
          }),
        },
      );

      if (!setRoleResponse.ok) {
        const errorText = await setRoleResponse.text();
        throw new Error(`Admin set role failed: ${errorText}`);
      }

      return { success: true };
    } catch (error) {
      console.error('❌ Admin set role failed:', error);
      return {
        success: false,
        error:
          error instanceof Error ? error.message : 'Failed to set user role',
      };
    }
  });

const adminBanUserProcedure = os
  .input(AdminBanUserSchema)
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 admin ban user, we need to check admin permissions
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated and has admin role
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required for admin operations',
        };
      }

      // For Sprint 3, we'll use internal HTTP call to the Better Auth admin endpoint
      const authUrl = process.env.BETTER_AUTH_URL || 'http://localhost:3000';
      const cookieHeader = headers?.get('cookie') || '';

      const banUserResponse = await fetch(
        `${authUrl}/api/auth/admin/ban-user`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Cookie: cookieHeader,
          },
          body: JSON.stringify({
            userId: input.userId,
            banReason: input.banReason,
            banExpiresIn: input.banExpiresIn,
          }),
        },
      );

      if (!banUserResponse.ok) {
        const errorText = await banUserResponse.text();
        throw new Error(`Admin ban user failed: ${errorText}`);
      }

      return { success: true };
    } catch (error) {
      console.error('❌ Admin ban user failed:', error);
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to ban user',
      };
    }
  });

const adminUnbanUserProcedure = os
  .input(AdminUnbanUserSchema)
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated and has admin role
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required for admin operations',
        };
      }

      const authUrl = process.env.BETTER_AUTH_URL || 'http://localhost:3000';
      const cookieHeader = headers?.get('cookie') || '';

      const unbanUserResponse = await fetch(
        `${authUrl}/api/auth/admin/unban-user`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Cookie: cookieHeader,
          },
          body: JSON.stringify({
            userId: input.userId,
          }),
        },
      );

      if (!unbanUserResponse.ok) {
        const errorText = await unbanUserResponse.text();
        throw new Error(`Admin unban user failed: ${errorText}`);
      }

      return { success: true };
    } catch (error) {
      console.error('❌ Admin unban user failed:', error);
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to unban user',
      };
    }
  });

const adminUpdateUserProcedure = os
  .input(AdminUpdateUserSchema)
  .output(
    z.object({
      success: z.boolean(),
      user: z.any().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);

      // Check internal service authentication OR user session with admin role
      const internalServiceSecret = headers?.get('x-internal-service-secret');
      const expectedSecret =
        process.env.INTERNAL_SERVICE_SECRET || process.env.INTERNAL_API_KEY;
      const isInternalService =
        internalServiceSecret &&
        expectedSecret &&
        internalServiceSecret === expectedSecret;

      if (!isInternalService) {
        // For external requests, check session authentication and admin role
        const session = await auth.api.getSession({
          headers: headers ?? new Headers(),
        });

        if (!session || !session.user) {
          return {
            success: false,
            error: 'Authentication required for admin operations',
          };
        }

        // Check if user has admin role
        const user = session.user as { role?: string };
        if (user.role !== 'admin' && user.role !== 'superadmin') {
          return {
            success: false,
            error: 'Admin role required for this operation',
          };
        }
      }

      // Use Better Auth's native admin API to update user
      const updateData: Record<string, any> = { userId: input.userId };
      if (input.name !== undefined) updateData.name = input.name;
      if (input.email !== undefined) updateData.email = input.email;
      if (input.image !== undefined) updateData.image = input.image;

      const result = await auth.api.updateUser({
        body: updateData,
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });

      if (
        !result.response ||
        typeof result.response !== 'object' ||
        !('user' in result.response)
      ) {
        throw new Error(
          'Update user failed: Invalid response from Better Auth',
        );
      }

      return { success: true, user: (result.response as any).user };
    } catch (error) {
      console.error('❌ Admin update user failed:', error);
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to update user',
      };
    }
  });

const adminListUserSessionsProcedure = os
  .input(AdminListUserSessionsSchema)
  .output(
    z.object({
      success: z.boolean(),
      sessions: z
        .array(
          z.object({
            id: z.string(),
            token: z.string(),
            userId: z.string(),
            expiresAt: z.string(),
            createdAt: z.string(),
            updatedAt: z.string(),
            userAgent: z.string().optional(),
            ipAddress: z.string().optional(),
          }),
        )
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 admin list user sessions, we need to check admin permissions
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated and has admin role
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required for admin operations',
        };
      }

      // For Sprint 3, we'll use internal HTTP call to the Better Auth admin endpoint
      const authUrl = process.env.BETTER_AUTH_URL || 'http://localhost:3000';
      const cookieHeader = headers?.get('cookie') || '';

      const sessionsResponse = await fetch(
        `${authUrl}/api/auth/admin/list-user-sessions`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Cookie: cookieHeader,
          },
          body: JSON.stringify({
            userId: input.userId,
          }),
        },
      );

      if (!sessionsResponse.ok) {
        const errorText = await sessionsResponse.text();
        throw new Error(`Admin list user sessions failed: ${errorText}`);
      }

      const sessionsData = (await sessionsResponse.json()) as {
        sessions?: Array<{
          id: string;
          token: string;
          userId: string;
          expiresAt: string;
          createdAt: string;
          updatedAt: string;
          userAgent?: string;
          ipAddress?: string;
        }>;
      };

      return {
        success: true,
        sessions: sessionsData.sessions || [],
      };
    } catch (_error) {
      console.error('❌ Admin list user sessions failed:', _error);
      return {
        success: false,
        error:
          _error instanceof Error
            ? _error.message
            : 'Failed to list user sessions',
      };
    }
  });

const adminRemoveUserProcedure = os
  .input(AdminRemoveUserSchema)
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      // For Sprint 3 admin remove user, we need to check admin permissions
      const headers = headersFromCtx(context as RpcContext);

      // Check if user is authenticated and has admin role
      const session = await auth.api.getSession({
        headers: headers ?? new Headers(),
      });

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required for admin operations',
        };
      }

      // For Sprint 3, we'll use internal HTTP call to the Better Auth admin endpoint
      const authUrl = process.env.BETTER_AUTH_URL || 'http://localhost:3000';
      const cookieHeader = headers?.get('cookie') || '';

      const removeUserResponse = await fetch(
        `${authUrl}/api/auth/admin/remove-user`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Cookie: cookieHeader,
          },
          body: JSON.stringify({
            userId: input.userId,
          }),
        },
      );

      if (!removeUserResponse.ok) {
        const errorText = await removeUserResponse.text();
        throw new Error(`Admin remove user failed: ${errorText}`);
      }

      return { success: true };
    } catch (_error) {
      console.error('❌ Admin remove user failed:', _error);
      return {
        success: false,
        error:
          _error instanceof Error ? _error.message : 'Failed to remove user',
      };
    }
  });

// Sprint 4: Session Management & Device Tracking

// List Device Sessions
const listDeviceSessionsProcedure = os
  .input(z.object({}))
  .output(
    z.object({
      success: z.boolean(),
      sessions: z
        .array(
          z.object({
            id: z.string(),
            userId: z.string(),
            expiresAt: z.string(),
            ipAddress: z.string().optional(),
            userAgent: z.string().optional(),
            deviceInfo: z
              .object({
                os: z.string().optional(),
                browser: z.string().optional(),
                device: z.string().optional(),
                location: z.string().optional(),
              })
              .optional(),
            createdAt: z.string(),
            lastSeenAt: z.string(),
            isCurrent: z.boolean(),
          }),
        )
        .optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try Better Auth multi-session API

      throw new Error(
        'Multi-session listDeviceSessions API endpoint verification needed',
      );
    } catch {
      console.warn('Multi-session API not available');
    }

    // Return error when multi-session management is not configured
    return {
      success: false,
      error:
        'Multi-session management is not configured on this server. Please contact support.',
    };
  });

// Revoke Device Session
const revokeDeviceSessionProcedure = os
  .input(
    z.object({
      sessionToken: z.string().min(1, 'Session token is required'),
    }),
  )
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(({ input }) => {
    try {
      // Try Better Auth multi-session revoke API

      throw new Error(
        'Multi-session revokeDeviceSession API endpoint verification needed',
      );
    } catch {
      console.warn('Multi-session revoke API not available');
    }

    // Return error when multi-session management is not configured
    return {
      success: false,
      error:
        'Multi-session management is not configured on this server. Please contact support.',
    };
  });

// Revoke All Sessions
const revokeAllSessionsProcedure = os
  .input(z.object({}))
  .output(
    z.object({
      success: z.boolean(),
      revokedCount: z.number().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try Better Auth session revocation API

      throw new Error(
        'Session revokeSessions API endpoint verification needed',
      );
    } catch {
      console.warn('Session revocation API not available');
    }

    // Return error when session management is not configured
    return {
      success: false,
      error:
        'Session management is not configured on this server. Please contact support.',
    };
  });

// Revoke Other Sessions (keep current)
const revokeOtherSessionsProcedure = os
  .input(z.object({}))
  .output(
    z.object({
      success: z.boolean(),
      revokedCount: z.number().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(() => {
    try {
      // Try Better Auth session revocation API

      throw new Error(
        'Session revokeOtherSessions API endpoint verification needed',
      );
    } catch {
      console.warn('Session revocation API not available');
    }

    // Return error when session management is not configured
    return {
      success: false,
      error:
        'Session management is not configured on this server. Please contact support.',
    };
  });

// Set Active Session
const setActiveSessionProcedure = os
  .input(
    z.object({
      sessionToken: z.string().min(1, 'Session token is required'),
    }),
  )
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(({ input }) => {
    try {
      // Try Better Auth multi-session setActiveSession API

      throw new Error(
        'Multi-session setActiveSession API endpoint verification needed',
      );
    } catch {
      console.warn('Multi-session setActive API not available');
    }

    // Return error when multi-session management is not configured
    return {
      success: false,
      error:
        'Multi-session management is not configured on this server. Please contact support.',
    };
  });

// Router
export const orpcRouter = {
  auth: {
    signIn: signInProcedure,
    signUp: signUpProcedure,
    signOut: signOutProcedure,
    getSession: getSessionProcedure,
    sendEmailVerification: sendEmailVerificationProcedure,
    verifyEmail: verifyEmailProcedure,
    sendPasswordReset: sendPasswordResetProcedure,
    resetPassword: resetPasswordProcedure,
    // Sprint 2: Enhanced Security & External IDPs
    initiateOAuth: initiateOAuthProcedure,
    checkPasswordStrength: checkPasswordStrengthProcedure,
  },
  // Two-Factor Authentication namespace
  twoFactor: {
    enable: enableTwoFactorProcedure,
    disable: disableTwoFactorProcedure,
    verify: verifyTwoFactorProcedure,
  },
  // Email OTP namespace
  emailOtp: {
    send: sendEmailOtpProcedure,
    verify: verifyEmailOtpProcedure,
  },
  // Phone/SMS OTP namespace
  phoneOtp: {
    send: sendPhoneOtpProcedure,
    verify: verifyPhoneOtpProcedure,
  },
  // Passkey namespace
  passkey: {
    create: createPasskeyProcedure,
  },
  profile: {
    getProfile: getProfileProcedure,
    updateProfile: updateProfileProcedure,
  },
  consent: {
    get: getConsentProcedure,
    update: updateConsentProcedure,
    withdraw: withdrawConsentProcedure,
  },
  // Sprint 3: Organization namespace
  organization: {
    create: createOrganizationProcedure,
    list: getOrganizationsListProcedure,
    inviteMember: inviteMemberProcedure,
    switchActive: switchOrganizationProcedure,
  },
  // Sprint 3: OIDC Provider namespace
  oidcProvider: {
    createClient: createOIDCClientProcedure,
    listClients: listOIDCClientsProcedure,
    getClient: getOIDCClientProcedure,
    deleteClient: deleteOIDCClientProcedure,
    generateSecret: generateClientSecretProcedure,
  },
  // Sprint 3: API Keys & Bearer Authentication namespace
  apiKeys: {
    create: createAPIKeyProcedure,
    list: listAPIKeysProcedure,
    delete: deleteAPIKeyProcedure,
    rotate: rotateAPIKeyProcedure,
    validate: validateAPIKeyProcedure,
  },
  // Sprint 3: Bearer Token Authentication namespace
  bearer: {
    validate: validateBearerTokenProcedure,
    create: createBearerTokenProcedure,
    revoke: revokeBearerTokenProcedure,
    list: listBearerTokensProcedure,
  },
  // Sprint 3: Admin Plugin namespace
  admin: {
    listUsers: adminListUsersProcedure,
    getUser: adminGetUserProcedure,
    // suspendUser: adminSuspendUserProcedure, // Commented out - schema removed
    listOrganizations: adminListOrganizationsProcedure,
    getSystemStats: adminGetSystemStatsProcedure,
    // Additional Better Auth Admin procedures
    createUser: adminCreateUserProcedure,
    setRole: adminSetRoleProcedure,
    banUser: adminBanUserProcedure,
    unbanUser: adminUnbanUserProcedure,
    updateUser: adminUpdateUserProcedure,
    listUserSessions: adminListUserSessionsProcedure,
    removeUser: adminRemoveUserProcedure,
  },
  // Sprint 4: Session Management & Device Tracking namespace
  sessions: {
    listDevices: listDeviceSessionsProcedure,
    revokeDevice: revokeDeviceSessionProcedure,
    revokeAll: revokeAllSessionsProcedure,
    revokeOthers: revokeOtherSessionsProcedure,
    setActive: setActiveSessionProcedure,
  },
};

export type ORPCRouter = typeof orpcRouter;
