import { os } from '@orpc/server';
import { z } from 'zod';
import { auth } from './auth';
import { db } from '../db';
import * as schema from '../db/schema';
import { and, asc, count, desc, eq, ilike, or, type SQL } from 'drizzle-orm';
import { redisSecondaryStorage } from '../db/redis';
import { createHash, randomBytes } from 'crypto';
import {
  authorizeAuthInternalServiceToken,
  loadAuthInternalServiceCredentials,
} from '../internal/internal-service-auth';

const _BEARER_CACHE_TTL = 90; // seconds
function _bearerCacheKey(token: string): string {
  return `bearer:val:${createHash('sha256').update(token).digest('hex')}`;
}

function hashBearerToken(token: string): string {
  return createHash('sha256').update(token).digest('hex');
}

function generateBearerToken(): string {
  return `velion_bt_${randomBytes(32).toString('base64url')}`;
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

type SessionUser = {
  id: string;
  email?: string;
  name?: string | null;
  role?: string | string[] | null;
};

type AuthSession = {
  user: SessionUser;
  session?: unknown;
};

type AdminAuthorization = {
  headers: Headers;
  internal: boolean;
  session?: AuthSession;
};

function hasInternalAdminServicePrincipal(
  headers: Headers | undefined,
): boolean {
  try {
    authorizeAuthInternalServiceToken(
      headers?.get('x-internal-service-secret') ?? undefined,
      loadAuthInternalServiceCredentials(),
      'auth:admin',
      'user-core',
    );
    return true;
  } catch {
    return false;
  }
}

function adminRoleNames(): Set<string> {
  const roles = (process.env.ADMIN_ROLES || 'admin,superadmin')
    .split(',')
    .map((role) => role.trim())
    .filter(Boolean);
  return new Set(roles.length > 0 ? roles : ['admin']);
}

function userRoleSet(role: unknown): Set<string> {
  if (Array.isArray(role)) {
    return new Set(role.map((value) => String(value).trim()).filter(Boolean));
  }
  if (typeof role === 'string') {
    return new Set(
      role
        .split(',')
        .map((value) => value.trim())
        .filter(Boolean),
    );
  }
  return new Set();
}

function isAdminSession(session: AuthSession | null | undefined): boolean {
  if (!session?.user) return false;
  const adminRoles = adminRoleNames();
  for (const role of userRoleSet(session.user.role)) {
    if (adminRoles.has(role)) return true;
  }
  const adminUserIds = (process.env.ADMIN_USER_IDS || '')
    .split(',')
    .map((id) => id.trim())
    .filter(Boolean);
  return adminUserIds.includes(session.user.id);
}

async function getAuthenticatedSession(
  headers: Headers | undefined,
): Promise<AuthSession | null> {
  return (await auth.api.getSession({
    headers: headers ?? new Headers(),
  })) as AuthSession | null;
}

async function authorizeAdminContext(
  context: RpcContext | undefined,
  options: { allowInternal?: boolean } = {},
): Promise<AdminAuthorization> {
  const headers = headersFromCtx(context);
  const allowInternal = options.allowInternal ?? true;
  if (allowInternal && hasInternalAdminServicePrincipal(headers)) {
    return { headers: headers ?? new Headers(), internal: true };
  }

  const session = await getAuthenticatedSession(headers);
  if (!session?.user) {
    throw new Error('Authentication required for admin operations');
  }
  if (!isAdminSession(session)) {
    throw new Error('Admin role required for this operation');
  }
  return { headers: headers ?? new Headers(), internal: false, session };
}

function getAuthApiMethod<TResult = unknown>(
  name: string,
): (options: Record<string, unknown>) => Promise<TResult> {
  const method = (auth.api as Record<string, unknown>)[name];
  if (typeof method !== 'function') {
    throw new Error(
      `${name} is not available. Enable the required Better Auth plugin.`,
    );
  }
  return async (options) => {
    const result: unknown = Reflect.apply(method, auth.api, [options]);
    return (await result) as TResult;
  };
}

function toIsoString(
  value: Date | string | number | null | undefined,
): string | undefined {
  if (!value) return undefined;
  if (value instanceof Date) return value.toISOString();
  return new Date(value).toISOString();
}

function apiKeyScopesFrom(value: unknown): string[] {
  if (!value || typeof value !== 'object') return [];
  const permissions = value as Record<string, unknown>;
  const apiScopes = permissions.api;
  if (Array.isArray(apiScopes)) {
    return apiScopes.map((scope) => String(scope));
  }
  return Object.values(permissions)
    .filter(Array.isArray)
    .flatMap((scopes) => (scopes as unknown[]).map((scope) => String(scope)));
}

function rateLimitPeriodToMs(period: 'minute' | 'hour' | 'day'): number {
  if (period === 'minute') return 60_000;
  if (period === 'hour') return 3_600_000;
  return 86_400_000;
}

// Narrowing helpers
function isRecord(v: unknown): v is Record<string, unknown> {
  return !!v && typeof v === 'object' && !Array.isArray(v);
}

function pickUser(obj: unknown): z.infer<typeof UserOut> | undefined {
  if (!isRecord(obj)) return undefined;
  if (typeof obj.id !== 'string' || typeof obj.email !== 'string') {
    return undefined;
  }
  return {
    id: obj.id,
    name: (obj.name as string | null | undefined) ?? null,
    email: obj.email,
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

function parseJsonRecord(value: unknown): Record<string, unknown> {
  if (isRecord(value)) return value;
  if (typeof value !== 'string' || !value.trim()) return {};
  try {
    const parsed = JSON.parse(value) as unknown;
    return isRecord(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

function stringListValue(value: unknown): string[] | undefined {
  if (Array.isArray(value)) {
    const items = value
      .map((item) => (typeof item === 'string' ? item.trim() : ''))
      .filter(Boolean);
    return items.length > 0 ? items : undefined;
  }

  if (typeof value !== 'string') return undefined;
  const trimmed = value.trim();
  if (!trimmed) return undefined;

  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (Array.isArray(parsed)) {
      const items = parsed
        .map((item) => (typeof item === 'string' ? item.trim() : ''))
        .filter(Boolean);
      return items.length > 0 ? items : undefined;
    }
  } catch {
    // Fall back to delimited strings below.
  }

  const delimiter = trimmed.includes(',') ? ',' : ' ';
  const items = trimmed
    .split(delimiter)
    .map((item) => item.trim())
    .filter(Boolean);
  return items.length > 0 ? items : undefined;
}

function redirectUrisFrom(value: unknown): string[] {
  return stringListValue(value) ?? [];
}

function oidcVelionMetadata(
  metadata: Record<string, unknown>,
): Record<string, unknown> {
  return isRecord(metadata.velion) ? metadata.velion : {};
}

function oidcMetadataString(
  metadata: Record<string, unknown>,
  key: string,
): string | undefined {
  const velion = oidcVelionMetadata(metadata);
  return stringValue(velion[key]) ?? stringValue(metadata[key]);
}

function oidcMetadataStringList(
  metadata: Record<string, unknown>,
  key: string,
  fallback: string[],
): string[] {
  const velion = oidcVelionMetadata(metadata);
  return (
    stringListValue(velion[key]) ?? stringListValue(metadata[key]) ?? fallback
  );
}

function buildOIDCClientMetadata(
  input: z.infer<typeof CreateOIDCClientSchema>,
): Record<string, unknown> {
  const metadata = input.metadata ?? {};
  const existingVelion = isRecord(metadata.velion) ? metadata.velion : {};
  return {
    ...metadata,
    velion: {
      ...existingVelion,
      organizationId: input.organizationId,
      scopes: input.scopes,
      grantTypes: input.grantTypes,
      responseTypes: input.responseTypes,
      tokenEndpointAuthMethod: input.tokenEndpointAuthMethod,
    },
  };
}

type OAuthApplicationRow = typeof schema.oauthApplication.$inferSelect;

function oidcClientOrganizationId(
  row: OAuthApplicationRow,
): string | undefined {
  return oidcMetadataString(parseJsonRecord(row.metadata), 'organizationId');
}

function mapOIDCClientListItem(row: OAuthApplicationRow) {
  const metadata = parseJsonRecord(row.metadata);
  return {
    clientId: row.clientId ?? row.id,
    name: row.name ?? row.clientId ?? 'Unnamed OIDC client',
    redirectUris: redirectUrisFrom(row.redirectUrls),
    scopes: oidcMetadataStringList(metadata, 'scopes', [
      'openid',
      'profile',
      'email',
    ]),
    organizationId: oidcClientOrganizationId(row),
    createdAt: toIsoString(row.createdAt) ?? new Date(0).toISOString(),
  };
}

function mapOIDCClientDetail(row: OAuthApplicationRow) {
  const metadata = parseJsonRecord(row.metadata);
  return {
    ...mapOIDCClientListItem(row),
    grantTypes: oidcMetadataStringList(metadata, 'grantTypes', [
      'authorization_code',
      'refresh_token',
    ]),
    responseTypes: oidcMetadataStringList(metadata, 'responseTypes', ['code']),
    tokenEndpointAuthMethod:
      row.authenticationScheme ??
      oidcMetadataString(metadata, 'tokenEndpointAuthMethod') ??
      'client_secret_basic',
    updatedAt: toIsoString(row.updatedAt),
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

const DEFAULT_CONSENT: z.infer<typeof ConsentSchema> = {
  analytics: false,
  marketing: false,
  necessary: true,
};

const CONSENT_SUBJECT_COOKIE = 'velion_consent_subject';

type ConsentSubject = {
  userId?: string;
  sessionId?: string;
};

type PrivacyConsentRow = typeof schema.privacyConsent.$inferSelect;

function normalizeConsent(
  consent: z.infer<typeof ConsentSchema>,
): z.infer<typeof ConsentSchema> {
  return {
    analytics: consent.analytics,
    marketing: consent.marketing,
    necessary: true,
  };
}

function cookieValue(
  headers: Headers | undefined,
  name: string,
): string | null {
  const cookieHeader = headers?.get('cookie');
  if (!cookieHeader) return null;
  const cookies = cookieHeader.split(';');
  for (const cookie of cookies) {
    const [rawName, ...rawValue] = cookie.trim().split('=');
    if (rawName === name) return decodeURIComponent(rawValue.join('='));
  }
  return null;
}

function setConsentSubjectCookie(
  context: RpcContext | undefined,
  subject: string,
) {
  const secure = process.env.NODE_ENV === 'production' ? '; Secure' : '';
  context?.setHeader?.(
    'set-cookie',
    `${CONSENT_SUBJECT_COOKIE}=${encodeURIComponent(
      subject,
    )}; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly${secure}`,
  );
}

async function resolveConsentSubject(
  context: RpcContext | undefined,
): Promise<ConsentSubject> {
  const headers = headersFromCtx(context);
  const session = await getAuthenticatedSession(headers).catch(() => null);
  if (session?.user?.id) return { userId: session.user.id };

  const existingSubject = cookieValue(headers, CONSENT_SUBJECT_COOKIE);
  if (existingSubject) return { sessionId: existingSubject };

  const subject = `anon_${randomBytes(16).toString('base64url')}`;
  setConsentSubjectCookie(context, subject);
  return { sessionId: subject };
}

function privacyConsentWhere(subject: ConsentSubject): SQL | undefined {
  if (subject.userId) {
    return eq(schema.privacyConsent.userId, subject.userId);
  }
  if (subject.sessionId) {
    return eq(schema.privacyConsent.sessionId, subject.sessionId);
  }
  return undefined;
}

function consentFromRow(
  row: PrivacyConsentRow | null | undefined,
): z.infer<typeof ConsentSchema> {
  if (!row) return DEFAULT_CONSENT;
  return normalizeConsent({
    analytics: Boolean(row.analytics),
    marketing: Boolean(row.marketing),
    necessary: Boolean(row.necessary),
  });
}

async function readConsent(
  subject: ConsentSubject,
): Promise<z.infer<typeof ConsentSchema>> {
  const where = privacyConsentWhere(subject);
  if (!where) return DEFAULT_CONSENT;
  const [row] = await db
    .select()
    .from(schema.privacyConsent)
    .where(where)
    .limit(1);
  return consentFromRow(row);
}

async function persistConsent(
  subject: ConsentSubject,
  consent: z.infer<typeof ConsentSchema>,
): Promise<z.infer<typeof ConsentSchema>> {
  const normalized = normalizeConsent(consent);
  const where = privacyConsentWhere(subject);
  if (!where) return normalized;

  const now = new Date();
  const [existing] = await db
    .select({ id: schema.privacyConsent.id })
    .from(schema.privacyConsent)
    .where(where)
    .limit(1);

  if (existing) {
    await db
      .update(schema.privacyConsent)
      .set({
        analytics: normalized.analytics,
        marketing: normalized.marketing,
        necessary: normalized.necessary,
        updatedAt: now,
      })
      .where(eq(schema.privacyConsent.id, existing.id));
    return normalized;
  }

  await db.insert(schema.privacyConsent).values({
    id: `pc_${randomBytes(16).toString('base64url')}`,
    userId: subject.userId,
    sessionId: subject.sessionId,
    analytics: normalized.analytics,
    marketing: normalized.marketing,
    necessary: normalized.necessary,
    createdAt: now,
    updatedAt: now,
  });

  return normalized;
}

async function isPasswordCompromised(password: string): Promise<boolean> {
  const sha1Hash = createHash('sha1')
    .update(password)
    .digest('hex')
    .toUpperCase();
  const prefix = sha1Hash.slice(0, 5);
  const suffix = sha1Hash.slice(5);
  const response = await fetch(
    `https://api.pwnedpasswords.com/range/${prefix}`,
    {
      headers: {
        'Add-Padding': 'true',
        'User-Agent': 'Velion Auth Password Checker',
      },
    },
  );
  if (!response.ok) {
    throw new Error(`HIBP range check failed with status ${response.status}`);
  }
  const body = await response.text();
  return body
    .split('\n')
    .some((line) => line.split(':')[0]?.trim().toUpperCase() === suffix);
}

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

const OrganizationApiRecordSchema = z.object({
  id: z.string(),
  name: z.string(),
  slug: z.string(),
  logo: z.string().nullable().optional(),
  createdAt: z.union([z.date(), z.string(), z.number()]),
});

const OrganizationListApiRecordSchema = OrganizationApiRecordSchema.extend({
  role: z.enum(['owner', 'admin', 'member']),
  memberCount: z.number().optional(),
});

const InviteMemberSchema = z.object({
  organizationId: z.string().min(1),
  email: z.string().email(),
  role: z.enum(['owner', 'admin', 'member']),
  expiresAt: z.coerce.date().optional(),
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
      const data = response;
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
      const data = response;
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
  .handler(async ({ context }) => {
    const subject = await resolveConsentSubject(context as RpcContext);
    return readConsent(subject);
  });

const updateConsentProcedure = os
  .input(ConsentSchema)
  .output(
    z.object({
      success: z.boolean(),
      consent: ConsentSchema,
    }),
  )
  .handler(async ({ input, context }) => {
    const subject = await resolveConsentSubject(context as RpcContext);
    const consent = await persistConsent(subject, input);
    return { success: true, consent };
  });

const withdrawConsentProcedure = os
  .input(z.object({}))
  .output(
    z.object({
      success: z.boolean(),
      consent: ConsentSchema,
    }),
  )
  .handler(async ({ context }) => {
    const subject = await resolveConsentSubject(context as RpcContext);
    const consent = await persistConsent(subject, DEFAULT_CONSENT);
    return {
      success: true,
      consent,
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
      await auth.api.requestPasswordReset({
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
      const enableTwoFactor = getAuthApiMethod<{
        headers: Headers;
        response: unknown;
      }>('enableTwoFactor');
      const { headers: respHeaders, response } = await enableTwoFactor({
        body: input,
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });
      forwardSetCookie(context as RpcContext, respHeaders);
      const data = response;
      const totpURI = isRecord(data) ? stringValue(data.totpURI) : undefined;
      return {
        success: true,
        qrCode: totpURI,
        backupCodes: isRecord(data)
          ? (stringListValue(data.backupCodes) ?? [])
          : [],
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
      const disableTwoFactor = getAuthApiMethod('disableTwoFactor');
      const result = await disableTwoFactor({
        body: input,
        headers: headers ?? new Headers(),
      });
      return {
        success: isRecord(result) ? Boolean(result.status) : true,
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
      code: z.string().min(6).max(64),
      type: z.enum(['totp', 'backup-code']).default('totp'),
      trustDevice: z.boolean().optional(),
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
      const methodName =
        input.type === 'totp' ? 'verifyTOTP' : 'verifyBackupCode';
      const verifyTwoFactor = getAuthApiMethod<{
        headers: Headers;
        response: unknown;
      }>(methodName);
      const { headers: respHeaders, response } = await verifyTwoFactor({
        body: {
          code: input.code,
          trustDevice: input.trustDevice,
        },
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });
      forwardSetCookie(context as RpcContext, respHeaders);
      const data = response;
      return {
        success: Boolean(isRecord(data) && (data.token || data.user)),
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

      const sendVerificationOTP = getAuthApiMethod('sendVerificationOTP');
      await sendVerificationOTP({
        body: input,
        headers: headers ?? new Headers(),
      });
      return { success: true };
    } catch (_error) {
      console.error('Send email OTP error:', _error);
      return {
        success: false,
        error:
          _error instanceof Error ? _error.message : 'Failed to send email OTP',
      };
    }
  });

const verifyEmailOtpProcedure = os
  .input(
    z.object({
      email: z.string().email(),
      otp: z.string().length(6),
      type: z
        .enum(['sign-in', 'email-verification', 'forget-password'])
        .default('sign-in'),
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

      if (input.type === 'email-verification') {
        const verifyEmailOTP = getAuthApiMethod('verifyEmailOTP');
        const result = await verifyEmailOTP({
          body: { email: input.email, otp: input.otp },
          headers: headers ?? new Headers(),
        });
        const user = isRecord(result) ? pickUser(result.user) : undefined;
        return { success: Boolean(isRecord(result) && result.status), user };
      }

      if (input.type === 'forget-password') {
        const checkVerificationOTP = getAuthApiMethod('checkVerificationOTP');
        const result = await checkVerificationOTP({
          body: {
            email: input.email,
            otp: input.otp,
            type: input.type,
          },
          headers: headers ?? new Headers(),
        });
        return { success: Boolean(isRecord(result) && result.success) };
      }

      const signInEmailOTP = getAuthApiMethod<{
        headers: Headers;
        response: unknown;
      }>('signInEmailOTP');
      const { headers: respHeaders, response } = await signInEmailOTP({
        body: { email: input.email, otp: input.otp },
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });
      forwardSetCookie(context as RpcContext, respHeaders);
      const data = response;
      const user = isRecord(data) ? pickUser(data.user) : undefined;
      if (!user) {
        return { success: false, error: 'Invalid or expired OTP' };
      }
      return {
        success: true,
        user,
        session: isRecord(data) ? data.session : undefined,
      };
    } catch (_error) {
      console.error('Verify email OTP error:', _error);
      return {
        success: false,
        error:
          _error instanceof Error ? _error.message : 'Invalid or expired OTP',
      };
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
      const [existingUser] = await db
        .select({ id: schema.user.id })
        .from(schema.user)
        .where(eq(schema.user.phoneNumber, input.phoneNumber))
        .limit(1);

      if (!existingUser) {
        return {
          success: false,
          error: 'No account is registered with this phone number',
        };
      }

      const sendPhoneNumberOTP = getAuthApiMethod('sendPhoneNumberOTP');
      await sendPhoneNumberOTP({
        body: input,
        headers: headers ?? new Headers(),
      });
      return { success: true };
    } catch (_error) {
      console.error('Send phone OTP error:', _error);
      return {
        success: false,
        error:
          _error instanceof Error ? _error.message : 'Failed to send SMS OTP',
      };
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
      const [existingUser] = await db
        .select({ id: schema.user.id })
        .from(schema.user)
        .where(eq(schema.user.phoneNumber, input.phoneNumber))
        .limit(1);

      if (!existingUser) {
        return {
          success: false,
          error: 'No account is registered with this phone number',
        };
      }

      const verifyPhoneNumber = getAuthApiMethod<{
        headers: Headers;
        response: unknown;
      }>('verifyPhoneNumber');
      const { headers: respHeaders, response } = await verifyPhoneNumber({
        body: {
          phoneNumber: input.phoneNumber,
          code: input.otp,
        },
        headers: headers ?? new Headers(),
        returnHeaders: true,
      });
      forwardSetCookie(context as RpcContext, respHeaders);
      const data = response;
      const user = isRecord(data) ? pickUser(data.user) : undefined;
      if (!isRecord(data) || !data.status) {
        return { success: false, error: 'Invalid or expired SMS OTP' };
      }
      return {
        success: true,
        user,
      };
    } catch (_error) {
      console.error('Verify phone OTP error:', _error);
      return {
        success: false,
        error:
          _error instanceof Error
            ? _error.message
            : 'Invalid or expired SMS OTP',
      };
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
      const session = await getAuthenticatedSession(headers);
      if (!session?.user) {
        return {
          success: false,
          error: 'Authentication required to create passkey options',
        };
      }

      const generatePasskeyRegistrationOptions = getAuthApiMethod(
        'generatePasskeyRegistrationOptions',
      );
      const options = await generatePasskeyRegistrationOptions({
        query: {
          name: input.name,
        },
        headers: headers ?? new Headers(),
      });

      return {
        success: true,
        options,
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
  .handler(async ({ input }) => {
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

      // Check if the provider is configured without trusting an untyped plugin
      // options object.
      const authOptions: unknown = auth.options;
      const socialProviders = isRecord(authOptions)
        ? authOptions.socialProviders
        : undefined;
      const socialConfig = isRecord(socialProviders)
        ? socialProviders[provider]
        : undefined;
      if (!socialConfig) {
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
        const data: unknown = await result.json();
        console.log(`📊 Response body:`, data);
        const redirectUrl = isRecord(data) ? stringValue(data.url) : undefined;
        if (redirectUrl) {
          console.log(`✅ Found OAuth URL in response body`);
          return {
            success: true,
            url: redirectUrl,
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
  .handler(async ({ input }) => {
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

      let isCompromised = false;
      if (process.env.HIBP_ENABLED !== 'false') {
        isCompromised = await isPasswordCompromised(password);
      }
      if (isCompromised) {
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
        feedback: ['Unable to complete the password breach check'],
        error:
          _error instanceof Error
            ? _error.message
            : 'Failed to check password strength',
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

      const createOrganization = getAuthApiMethod('createOrganization');
      const rawResult = await createOrganization({
        body: {
          name: input.name,
          slug: input.slug || input.name.toLowerCase().replace(/\s+/g, '-'),
          logo: input.logo,
          metadata: input.metadata,
        },
        headers: headers ?? new Headers(),
      });
      const result = OrganizationApiRecordSchema.parse(rawResult);

      console.log('✅ Organization created:', result);

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
      const listOrganizations = getAuthApiMethod('listOrganizations');
      const rawResult = await listOrganizations({
        query: {},
        headers: headers ?? new Headers(),
      });
      const result = z.array(OrganizationListApiRecordSchema).parse(rawResult);

      const organizations = result.map((org) => ({
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
  .handler(async ({ input, context }) => {
    try {
      const authorization = await authorizeAdminContext(context as RpcContext);
      if (input.responseTypes.includes('id_token')) {
        return {
          success: false,
          error:
            'The configured Better Auth OIDC provider supports code and token response types, not id_token implicit response type.',
        };
      }

      const registerOAuthApplication = getAuthApiMethod(
        'registerOAuthApplication',
      );
      const registration = (await registerOAuthApplication({
        body: {
          redirect_uris: input.redirectUris,
          token_endpoint_auth_method: input.tokenEndpointAuthMethod,
          grant_types: input.grantTypes,
          response_types: input.responseTypes,
          client_name: input.name,
          scope: input.scopes.join(' '),
          metadata: buildOIDCClientMetadata(input),
        },
        headers: authorization.headers,
      })) as Record<string, unknown>;

      const clientId = stringValue(registration.client_id) ?? '';
      const clientSecret = stringValue(registration.client_secret) ?? '';
      if (!clientId || !clientSecret) {
        return {
          success: false,
          error: 'OIDC Provider did not return a usable client credential',
        };
      }

      return {
        success: true,
        client: {
          clientId,
          clientSecret,
          name: stringValue(registration.client_name) ?? input.name,
          redirectUris:
            stringListValue(registration.redirect_uris) ?? input.redirectUris,
          scopes:
            stringListValue(registration.scope) ??
            stringListValue(parseJsonRecord(registration.metadata).scopes) ??
            input.scopes,
          grantTypes:
            stringListValue(registration.grant_types) ?? input.grantTypes,
          responseTypes:
            stringListValue(registration.response_types) ?? input.responseTypes,
          tokenEndpointAuthMethod:
            stringValue(registration.token_endpoint_auth_method) ??
            input.tokenEndpointAuthMethod,
          organizationId: input.organizationId,
          createdAt: registration.client_id_issued_at
            ? new Date(
                Number(registration.client_id_issued_at) * 1000,
              ).toISOString()
            : new Date().toISOString(),
        },
      };
    } catch (error) {
      console.error('OIDC Provider client creation failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'OIDC Provider client creation failed',
      };
    }
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
  .handler(async ({ input, context }) => {
    try {
      await authorizeAdminContext(context as RpcContext);

      if (input.organizationId) {
        const allClients = await db
          .select()
          .from(schema.oauthApplication)
          .orderBy(desc(schema.oauthApplication.createdAt));
        const filtered = allClients.filter(
          (client) => oidcClientOrganizationId(client) === input.organizationId,
        );
        return {
          success: true,
          clients: filtered
            .slice(input.offset, input.offset + input.limit)
            .map(mapOIDCClientListItem),
          total: filtered.length,
        };
      }

      const [totalRow] = await db
        .select({ value: count() })
        .from(schema.oauthApplication);
      const clients = await db
        .select()
        .from(schema.oauthApplication)
        .orderBy(desc(schema.oauthApplication.createdAt))
        .limit(input.limit)
        .offset(input.offset);

      return {
        success: true,
        clients: clients.map(mapOIDCClientListItem),
        total: totalRow?.value ?? clients.length,
      };
    } catch (error) {
      console.error('OIDC Provider client listing failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'OIDC Provider client listing failed',
      };
    }
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
  .handler(async ({ input, context }) => {
    try {
      await authorizeAdminContext(context as RpcContext);
      const [client] = await db
        .select()
        .from(schema.oauthApplication)
        .where(eq(schema.oauthApplication.clientId, input.clientId))
        .limit(1);

      if (!client) {
        return { success: false, error: 'OIDC client not found' };
      }

      return {
        success: true,
        client: mapOIDCClientDetail(client),
      };
    } catch (error) {
      console.error('OIDC Provider get client failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'OIDC Provider get client failed',
      };
    }
  });

const deleteOIDCClientProcedure = os
  .input(DeleteOIDCClientSchema)
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      await authorizeAdminContext(context as RpcContext);
      const [client] = await db
        .select({ id: schema.oauthApplication.id })
        .from(schema.oauthApplication)
        .where(eq(schema.oauthApplication.clientId, input.clientId))
        .limit(1);

      if (!client) {
        return { success: false, error: 'OIDC client not found' };
      }

      await db
        .delete(schema.oauthApplication)
        .where(eq(schema.oauthApplication.clientId, input.clientId));

      return { success: true };
    } catch (error) {
      console.error('OIDC Provider delete client failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'OIDC Provider delete client failed',
      };
    }
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
  .handler(async ({ input, context }) => {
    try {
      await authorizeAdminContext(context as RpcContext);
      const [client] = await db
        .select({ id: schema.oauthApplication.id })
        .from(schema.oauthApplication)
        .where(eq(schema.oauthApplication.clientId, input.clientId))
        .limit(1);

      if (!client) {
        return { success: false, error: 'OIDC client not found' };
      }

      const createdAt = new Date();
      const clientSecret = randomBytes(32).toString('base64url');
      await db
        .update(schema.oauthApplication)
        .set({
          clientSecret,
          updatedAt: createdAt,
        })
        .where(eq(schema.oauthApplication.clientId, input.clientId));

      return {
        success: true,
        secret: {
          secretId: `${input.clientId}:${createdAt.getTime()}`,
          clientSecret,
          createdAt: createdAt.toISOString(),
        },
      };
    } catch (error) {
      console.error('OIDC Provider client secret generation failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'OIDC Provider client secret generation failed',
      };
    }
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
      const headers = headersFromCtx(context as RpcContext);
      const session = await getAuthenticatedSession(headers);

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required to create API key',
        };
      }

      const createApiKey = getAuthApiMethod('createApiKey');
      const expiresIn = input.expiresAt
        ? Math.max(
            60,
            Math.floor((input.expiresAt.getTime() - Date.now()) / 1000),
          )
        : undefined;
      const rateLimit = input.rateLimit
        ? {
            rateLimitEnabled: true,
            rateLimitTimeWindow: rateLimitPeriodToMs(input.rateLimit.period),
            rateLimitMax: input.rateLimit.requests,
          }
        : {};

      const apiKeyData = await createApiKey({
        body: {
          configId: 'org-keys',
          name: input.name,
          organizationId: input.organizationId,
          expiresIn,
          metadata: {
            description: input.description,
            scopes: input.scopes,
            organizationId: input.organizationId,
          },
          permissions: {
            api: input.scopes,
          },
          ...rateLimit,
        },
        headers: headers ?? new Headers(),
      });

      const created = apiKeyData as {
        id?: string;
        name?: string | null;
        key?: string;
        createdAt?: Date | string;
        expiresAt?: Date | string | null;
        permissions?: unknown;
        metadata?: { description?: string; scopes?: string[] } | null;
        rateLimitMax?: number | null;
        rateLimitTimeWindow?: number | null;
      };

      if (!created.key) {
        throw new Error('API key creation did not return a key');
      }

      return {
        success: true,
        apiKey: {
          id: created.id ?? '',
          name: created.name || input.name,
          key: created.key,
          organizationId: input.organizationId,
          scopes:
            apiKeyScopesFrom(created.permissions).length > 0
              ? apiKeyScopesFrom(created.permissions)
              : input.scopes,
          description: created.metadata?.description ?? input.description,
          createdAt: toIsoString(created.createdAt) ?? new Date().toISOString(),
          expiresAt: toIsoString(created.expiresAt),
          rateLimit:
            created.rateLimitMax && created.rateLimitTimeWindow
              ? {
                  requests: created.rateLimitMax,
                  period:
                    created.rateLimitTimeWindow <= 60_000
                      ? 'minute'
                      : created.rateLimitTimeWindow <= 3_600_000
                        ? 'hour'
                        : 'day',
                }
              : input.rateLimit,
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
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const session = await getAuthenticatedSession(headers);

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required to list API keys',
        };
      }

      const listApiKeys = getAuthApiMethod('listApiKeys');
      const configId = input.organizationId ? 'org-keys' : 'user-keys';
      const result = (await listApiKeys({
        query: {
          configId,
          organizationId: input.organizationId,
          limit: input.limit,
          offset: input.offset,
          sortBy: 'createdAt',
          sortDirection: 'desc',
        },
        headers: headers ?? new Headers(),
      })) as {
        apiKeys?: Array<{
          id: string;
          name?: string | null;
          referenceId?: string;
          permissions?: unknown;
          metadata?: { description?: string; scopes?: string[] } | null;
          rateLimitMax?: number | null;
          rateLimitTimeWindow?: number | null;
          expiresAt?: Date | string | null;
          createdAt?: Date | string;
          lastRequest?: Date | string | null;
        }>;
        total?: number;
      };

      const apiKeys = (result.apiKeys ?? [])
        .filter(
          (key) =>
            input.includeExpired ||
            !key.expiresAt ||
            new Date(key.expiresAt) > new Date(),
        )
        .map((key) => {
          const scopes = apiKeyScopesFrom(key.permissions);
          return {
            id: key.id,
            name: key.name || 'API key',
            description: key.metadata?.description,
            organizationId:
              input.organizationId ||
              (configId === 'org-keys' ? key.referenceId || '' : ''),
            scopes:
              scopes.length > 0
                ? scopes
                : Array.isArray(key.metadata?.scopes)
                  ? key.metadata.scopes
                  : [],
            rateLimit:
              key.rateLimitMax && key.rateLimitTimeWindow
                ? {
                    requests: key.rateLimitMax,
                    period:
                      key.rateLimitTimeWindow <= 60_000
                        ? 'minute'
                        : key.rateLimitTimeWindow <= 3_600_000
                          ? 'hour'
                          : 'day',
                  }
                : undefined,
            expiresAt: toIsoString(key.expiresAt),
            createdAt: toIsoString(key.createdAt) ?? new Date().toISOString(),
            lastUsed: toIsoString(key.lastRequest),
            isExpired: Boolean(
              key.expiresAt && new Date(key.expiresAt) <= new Date(),
            ),
          };
        });

      return {
        success: true,
        apiKeys,
        total: result.total ?? apiKeys.length,
      };
    } catch (error) {
      console.error('❌ API Key listing failed:', error);
      return {
        success: false,
        error:
          error instanceof Error ? error.message : 'Failed to list API keys',
      };
    }
  });

const deleteAPIKeyProcedure = os
  .input(DeleteAPIKeySchema)
  .output(
    z.object({
      success: z.boolean(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const session = await getAuthenticatedSession(headers);

      if (!session || !session.user) {
        return {
          success: false,
          error: 'Authentication required to delete API key',
        };
      }

      const deleteApiKey = getAuthApiMethod('deleteApiKey');
      let lastError: unknown;
      for (const configId of ['org-keys', 'user-keys']) {
        try {
          await deleteApiKey({
            body: {
              configId,
              keyId: input.keyId,
            },
            headers: headers ?? new Headers(),
          });
          return { success: true };
        } catch (error) {
          lastError = error;
        }
      }
      throw lastError instanceof Error
        ? lastError
        : new Error('API key deletion failed');
    } catch (error) {
      console.error('❌ API Key deletion failed:', error);
      return {
        success: false,
        error:
          error instanceof Error ? error.message : 'Failed to delete API key',
      };
    }
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
    return {
      success: false,
      error:
        'API key rotation is not supported by Better Auth for existing key IDs. Create a replacement key, then delete the old key.',
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
  .handler(async ({ input }) => {
    try {
      const verifyApiKey = getAuthApiMethod('verifyApiKey');
      const result = (await verifyApiKey({
        body: {
          key: input.apiKey,
          permissions: input.scope ? { api: [input.scope] } : undefined,
        },
      })) as {
        valid?: boolean;
        error?: { message?: string } | null;
        key?: {
          id: string;
          configId?: string;
          referenceId?: string;
          permissions?: unknown;
          expiresAt?: Date | string | null;
          rateLimitMax?: number | null;
          rateLimitTimeWindow?: number | null;
          remaining?: number | null;
        } | null;
      };

      if (!result.valid || !result.key) {
        return {
          success: true,
          valid: false,
          error: result.error?.message,
        };
      }

      return {
        success: true,
        valid: true,
        keyInfo: {
          id: result.key.id,
          organizationId:
            result.key.configId === 'org-keys'
              ? result.key.referenceId || ''
              : '',
          scopes: apiKeyScopesFrom(result.key.permissions),
          rateLimit:
            result.key.rateLimitMax && result.key.rateLimitTimeWindow
              ? {
                  requests: result.key.rateLimitMax,
                  period:
                    result.key.rateLimitTimeWindow <= 60_000
                      ? 'minute'
                      : result.key.rateLimitTimeWindow <= 3_600_000
                        ? 'hour'
                        : 'day',
                  remaining: result.key.remaining ?? result.key.rateLimitMax,
                  resetAt: new Date(
                    Date.now() + result.key.rateLimitTimeWindow,
                  ).toISOString(),
                }
              : undefined,
          expiresAt: toIsoString(result.key.expiresAt),
        },
      };
    } catch (error) {
      console.error('❌ API Key validation failed:', error);
      return {
        success: false,
        valid: false,
        error:
          error instanceof Error ? error.message : 'API key validation failed',
      };
    }
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
      } catch {
        // Redis unavailable — fall through to HTTP validation
      }

      const tokenHash = hashBearerToken(bearerToken);
      const [persistedToken] = await db
        .select()
        .from(schema.bearerToken)
        .where(eq(schema.bearerToken.token, tokenHash))
        .limit(1);

      if (persistedToken) {
        if (persistedToken.expiresAt <= new Date()) {
          await db
            .delete(schema.bearerToken)
            .where(eq(schema.bearerToken.id, persistedToken.id));
          return {
            success: false,
            valid: false,
            error: 'Bearer token expired',
          };
        }

        const [tokenUser] = await db
          .select()
          .from(schema.user)
          .where(eq(schema.user.id, persistedToken.userId))
          .limit(1);

        if (!tokenUser) {
          return {
            success: false,
            valid: false,
            error: 'Bearer token user not found',
          };
        }

        const persistedSession = {
          user: {
            id: tokenUser.id,
            email: tokenUser.email,
            name: tokenUser.name || null,
          },
          session: {
            token: bearerToken,
            expiresAt: persistedToken.expiresAt.toISOString(),
          },
        };

        try {
          await redisSecondaryStorage.set(
            cacheKey,
            JSON.stringify(persistedSession),
            Math.min(
              _BEARER_CACHE_TTL,
              Math.max(
                1,
                Math.floor(
                  (persistedToken.expiresAt.getTime() - Date.now()) / 1000,
                ),
              ),
            ),
          );
        } catch {
          // Redis write failure is non-fatal
        }

        return {
          success: true,
          valid: true,
          session: {
            user: persistedSession.user,
            token: bearerToken,
            expiresAt: persistedSession.session.expiresAt,
            scopes: ['read'],
          },
        };
      }

      const sessionHeaders = new Headers();
      sessionHeaders.set('authorization', `Bearer ${bearerToken}`);
      const sessionData = (await auth.api.getSession({
        headers: sessionHeaders,
      })) as {
        user?: { id: string; email: string; name?: string | null };
        session?: { token?: string; expiresAt: Date | string };
      } | null;

      if (!sessionData?.user || !sessionData.session) {
        return {
          success: false,
          valid: false,
          error: 'Invalid bearer token',
        };
      }
      const sessionExpiresAt = toIsoString(sessionData.session.expiresAt);
      if (!sessionExpiresAt) {
        throw new Error('Invalid session expiration');
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
            session: {
              token: sessionData.session.token || bearerToken,
              expiresAt: sessionExpiresAt,
            },
          }),
          _BEARER_CACHE_TTL,
        );
      } catch {
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
          expiresAt: sessionExpiresAt,
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

      const bearerToken = generateBearerToken();
      const expiresAt = input.expiresIn
        ? new Date(Date.now() + input.expiresIn * 1000).toISOString()
        : new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString(); // Default 24 hours
      const now = new Date();
      const [createdToken] = await db
        .insert(schema.bearerToken)
        .values({
          id: `bt_${randomBytes(16).toString('hex')}`,
          token: hashBearerToken(bearerToken),
          userId: session.user.id,
          expiresAt: new Date(expiresAt),
          createdAt: now,
          updatedAt: now,
        })
        .returning();

      console.log('✅ Bearer token created:', {
        id: createdToken.id,
        userId: session.user.id,
        expiresAt,
        scopes: input.scopes,
      });

      return {
        success: true,
        token: {
          id: createdToken.id,
          token: bearerToken,
          expiresAt,
          scopes: input.scopes || ['read'],
          createdAt: createdToken.createdAt.toISOString(),
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

      const tokenHash = hashBearerToken(input.token);
      await db
        .delete(schema.bearerToken)
        .where(
          and(
            eq(schema.bearerToken.token, tokenHash),
            eq(schema.bearerToken.userId, session.user.id),
          ),
        );

      try {
        await redisSecondaryStorage.delete(_bearerCacheKey(input.token));
      } catch {
        // Redis delete failure is non-fatal
      }

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

      const rows = await db
        .select()
        .from(schema.bearerToken)
        .where(eq(schema.bearerToken.userId, session.user.id))
        .orderBy(desc(schema.bearerToken.createdAt))
        .limit(input.limit)
        .offset(input.offset);

      const now = new Date();
      const tokens = rows
        .filter((token) => input.includeExpired || token.expiresAt > now)
        .map((token) => ({
          id: token.id,
          token: `${token.token.slice(0, 12)}...`,
          expiresAt: token.expiresAt.toISOString(),
          scopes: ['read'],
          createdAt: token.createdAt.toISOString(),
          lastUsed: null,
          isActive: token.expiresAt > now,
        }));

      return {
        success: true,
        tokens,
        total: tokens.length,
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
      await authorizeAdminContext(context as RpcContext);
      const { limit = 20, offset = 0, search, role } = input;
      const conditions: SQL[] = [];
      if (role) {
        conditions.push(eq(schema.user.role, role));
      }
      if (search) {
        conditions.push(
          or(
            ilike(schema.user.email, `%${search}%`),
            ilike(schema.user.name, `%${search}%`),
          )!,
        );
      }

      const whereClause =
        conditions.length > 0 ? and(...conditions) : undefined;
      const orderColumn =
        input.sortBy === 'email' ? schema.user.email : schema.user.createdAt;
      const orderBy =
        input.sortOrder === 'asc' ? asc(orderColumn) : desc(orderColumn);
      const [totalRow] = whereClause
        ? await db
            .select({ value: count() })
            .from(schema.user)
            .where(whereClause)
        : await db.select({ value: count() }).from(schema.user);

      let usersQuery = db.select().from(schema.user).$dynamic();
      if (whereClause) {
        usersQuery = usersQuery.where(whereClause);
      }
      const paginatedUsers = await usersQuery
        .orderBy(orderBy)
        .limit(limit)
        .offset(offset);

      const mappedUsers = paginatedUsers.map((user) => ({
        id: user.id,
        name: user.name || null,
        email: user.email,
        emailVerified: user.emailVerified || false,
        image: user.image || null,
        role: user.role || 'user',
        status: user.banned
          ? ('suspended' as const)
          : user.emailVerified
            ? ('active' as const)
            : ('pending' as const),
        lastLogin: user.updatedAt ? user.updatedAt.toISOString() : null,
        createdAt: user.createdAt.toISOString(),
        organizationCount: 0,
      }));

      return {
        success: true,
        users: mappedUsers,
        total: totalRow?.value ?? mappedUsers.length,
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
  .handler(async ({ input, context }) => {
    try {
      await authorizeAdminContext(context as RpcContext);

      const [user] = await db
        .select()
        .from(schema.user)
        .where(eq(schema.user.id, input.userId))
        .limit(1);

      if (!user) {
        return {
          success: false,
          error: 'User not found',
        };
      }

      const memberships = await db
        .select({
          id: schema.organization.id,
          name: schema.organization.name,
          role: schema.member.role,
        })
        .from(schema.member)
        .innerJoin(
          schema.organization,
          eq(schema.member.organizationId, schema.organization.id),
        )
        .where(eq(schema.member.userId, user.id));

      return {
        success: true,
        user: {
          id: user.id,
          name: user.name || null,
          email: user.email,
          emailVerified: user.emailVerified || false,
          image: user.image || null,
          status: user.banned
            ? ('suspended' as const)
            : user.emailVerified
              ? ('active' as const)
              : ('pending' as const),
          lastLogin: user.updatedAt ? user.updatedAt.toISOString() : null,
          createdAt: user.createdAt.toISOString(),
          updatedAt: user.updatedAt.toISOString(),
          metadata: {
            phoneNumber: user.phoneNumber,
            phoneNumberVerified: user.phoneNumberVerified,
            twoFactorEnabled: user.twoFactorEnabled,
            role: user.role || 'user',
            banned: user.banned,
            banReason: user.banReason,
            banExpires: user.banExpires?.toISOString(),
          },
          organizations: memberships.map((membership) => ({
            id: membership.id,
            name: membership.name,
            role:
              membership.role === 'owner' || membership.role === 'admin'
                ? membership.role
                : 'member',
          })),
        },
      };
    } catch (error) {
      console.error('❌ Admin get user failed:', error);
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to get user',
      };
    }
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
  .handler(async ({ input, context }) => {
    try {
      await authorizeAdminContext(context as RpcContext);
      let organizationsQuery = db.select().from(schema.organization).$dynamic();
      if (input.search) {
        organizationsQuery = organizationsQuery.where(
          ilike(schema.organization.name, `%${input.search}%`),
        );
      }
      const orderColumn =
        input.sortBy === 'name'
          ? schema.organization.name
          : schema.organization.createdAt;
      const organizations = await organizationsQuery
        .orderBy(
          input.sortOrder === 'asc' ? asc(orderColumn) : desc(orderColumn),
        )
        .limit(input.limit)
        .offset(input.offset);

      const mappedOrganizations = await Promise.all(
        organizations.map(async (organization) => {
          const [memberCountRow] = await db
            .select({ value: count() })
            .from(schema.member)
            .where(eq(schema.member.organizationId, organization.id));
          const [ownerMember] = await db
            .select({
              userId: schema.member.userId,
              email: schema.user.email,
            })
            .from(schema.member)
            .innerJoin(schema.user, eq(schema.member.userId, schema.user.id))
            .where(
              and(
                eq(schema.member.organizationId, organization.id),
                eq(schema.member.role, 'owner'),
              ),
            )
            .limit(1);

          return {
            id: organization.id,
            name: organization.name,
            slug: organization.slug || '',
            memberCount: memberCountRow?.value ?? 0,
            createdAt: organization.createdAt.toISOString(),
            ownerId: ownerMember?.userId ?? '',
            ownerEmail: ownerMember?.email ?? '',
          };
        }),
      );

      const [totalRow] = await db
        .select({ value: count() })
        .from(schema.organization);
      return {
        success: true,
        organizations: mappedOrganizations,
        total: totalRow?.value ?? mappedOrganizations.length,
      };
    } catch (error) {
      console.error('❌ Admin organization listing failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to list organizations',
      };
    }
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
  .handler(async ({ context }) => {
    try {
      await authorizeAdminContext(context as RpcContext);
      const [userCount] = await db.select({ value: count() }).from(schema.user);
      const [organizationCount] = await db
        .select({ value: count() })
        .from(schema.organization);
      const [apiKeyCount] = await db
        .select({ value: count() })
        .from(schema.apikey);
      const [oidcClientCount] = await db
        .select({ value: count() })
        .from(schema.oauthApplication);
      const activeUsersRow = await db
        .select({ value: count() })
        .from(schema.user)
        .where(eq(schema.user.banned, false));

      const memoryUsage = process.memoryUsage();
      return {
        success: true,
        stats: {
          totalUsers: userCount?.value ?? 0,
          totalOrganizations: organizationCount?.value ?? 0,
          totalAPIKeys: apiKeyCount?.value ?? 0,
          totalOIDCClients: oidcClientCount?.value ?? 0,
          activeUsers: activeUsersRow[0]?.value ?? 0,
          newUsersThisPeriod: 0,
          newOrganizationsThisPeriod: 0,
          loginStats: {
            totalLogins: 0,
            uniqueLogins: 0,
            failedLogins: 0,
          },
          systemHealth: {
            uptime: Math.floor(process.uptime()),
            memoryUsage: memoryUsage.rss,
            cpuUsage: 0,
          },
        },
      };
    } catch (error) {
      console.error('❌ Admin system stats failed:', error);
      return {
        success: false,
        error:
          error instanceof Error ? error.message : 'Failed to get system stats',
      };
    }
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
      const authorization = await authorizeAdminContext(context as RpcContext);
      const createUser = getAuthApiMethod('createUser');
      const result = (await createUser({
        body: {
          email: input.email,
          password: input.password,
          name: input.name,
          role: input.role || 'user',
          data: input.data,
        },
        ...(authorization.internal ? {} : { headers: authorization.headers }),
      })) as {
        user?: {
          id: string;
          email: string;
          name?: string | null;
          role?: string;
          createdAt?: Date | string;
        };
      };

      const newUser = result.user;
      if (!newUser) {
        throw new Error('Better Auth createUser did not return a user');
      }

      return {
        success: true,
        user: {
          id: newUser.id,
          email: newUser.email,
          name: newUser.name || input.name,
          role: newUser.role || 'user',
          createdAt: toIsoString(newUser.createdAt) ?? new Date().toISOString(),
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
      const authorization = await authorizeAdminContext(context as RpcContext, {
        allowInternal: false,
      });
      const setRole = getAuthApiMethod('setRole');
      await setRole({
        body: {
          userId: input.userId,
          role: input.role,
        },
        headers: authorization.headers,
      });

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
      const authorization = await authorizeAdminContext(context as RpcContext, {
        allowInternal: false,
      });
      const banUser = getAuthApiMethod('banUser');
      await banUser({
        body: {
          userId: input.userId,
          banReason: input.banReason,
          banExpiresIn: input.banExpiresIn,
        },
        headers: authorization.headers,
      });

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
      const authorization = await authorizeAdminContext(context as RpcContext, {
        allowInternal: false,
      });
      const unbanUser = getAuthApiMethod('unbanUser');
      await unbanUser({
        body: {
          userId: input.userId,
        },
        headers: authorization.headers,
      });

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
      user: z.unknown().optional(),
      error: z.string().optional(),
    }),
  )
  .handler(async ({ input, context }) => {
    try {
      const authorization = await authorizeAdminContext(context as RpcContext, {
        allowInternal: false,
      });
      const data: Record<string, unknown> = {};
      if (input.name !== undefined) data.name = input.name;
      if (input.email !== undefined) data.email = input.email;
      if (input.image !== undefined) data.image = input.image;

      const adminUpdateUser = getAuthApiMethod('adminUpdateUser');
      const result: unknown = await adminUpdateUser({
        body: {
          userId: input.userId,
          data,
        },
        headers: authorization.headers,
      });

      return {
        success: true,
        user: isRecord(result) ? result.user : undefined,
      };
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
      const authorization = await authorizeAdminContext(context as RpcContext, {
        allowInternal: false,
      });
      const listUserSessions = getAuthApiMethod('listUserSessions');
      const sessionsData = (await listUserSessions({
        body: {
          userId: input.userId,
        },
        headers: authorization.headers,
      })) as {
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
      const authorization = await authorizeAdminContext(context as RpcContext, {
        allowInternal: false,
      });
      const removeUser = getAuthApiMethod('removeUser');
      await removeUser({
        body: {
          userId: input.userId,
        },
        headers: authorization.headers,
      });

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
  .handler(async ({ context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const listDeviceSessions = getAuthApiMethod('listDeviceSessions');
      const sessions = (await listDeviceSessions({
        headers: headers ?? new Headers(),
      })) as Array<{
        session: {
          id: string;
          token: string;
          userId: string;
          expiresAt: Date | string;
          createdAt: Date | string;
          updatedAt: Date | string;
          userAgent?: string | null;
          ipAddress?: string | null;
        };
      }>;

      const currentSession = await getAuthenticatedSession(headers);
      const currentSessionData = isRecord(currentSession?.session)
        ? currentSession.session
        : undefined;
      const currentSessionToken = currentSessionData
        ? stringValue(currentSessionData.token)
        : undefined;
      const mappedSessions = sessions.map((entry) => ({
        id: entry.session.id,
        userId: entry.session.userId,
        expiresAt:
          toIsoString(entry.session.expiresAt) ?? new Date().toISOString(),
        ipAddress: entry.session.ipAddress || undefined,
        userAgent: entry.session.userAgent || undefined,
        deviceInfo: {
          browser: entry.session.userAgent || undefined,
        },
        createdAt:
          toIsoString(entry.session.createdAt) ?? new Date().toISOString(),
        lastSeenAt:
          toIsoString(entry.session.updatedAt) ?? new Date().toISOString(),
        isCurrent: currentSessionToken === entry.session.token,
      }));

      return {
        success: true,
        sessions: mappedSessions,
      };
    } catch (error) {
      console.error('❌ Multi-session list devices failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to list device sessions',
      };
    }
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
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const revokeDeviceSession = getAuthApiMethod('revokeDeviceSession');
      await revokeDeviceSession({
        body: {
          sessionToken: input.sessionToken,
        },
        headers: headers ?? new Headers(),
      });

      return { success: true };
    } catch (error) {
      console.error('❌ Multi-session revoke device failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to revoke device session',
      };
    }
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
  .handler(async ({ context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const revokeSessions = getAuthApiMethod('revokeSessions');
      await revokeSessions({
        headers: headers ?? new Headers(),
      });

      return { success: true };
    } catch (error) {
      console.error('❌ Revoke all sessions failed:', error);
      return {
        success: false,
        error:
          error instanceof Error ? error.message : 'Failed to revoke sessions',
      };
    }
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
  .handler(async ({ context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const revokeOtherSessions = getAuthApiMethod('revokeOtherSessions');
      await revokeOtherSessions({
        headers: headers ?? new Headers(),
      });

      return { success: true };
    } catch (error) {
      console.error('❌ Revoke other sessions failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to revoke other sessions',
      };
    }
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
  .handler(async ({ input, context }) => {
    try {
      const headers = headersFromCtx(context as RpcContext);
      const setActiveSession = getAuthApiMethod('setActiveSession');
      await setActiveSession({
        body: {
          sessionToken: input.sessionToken,
        },
        headers: headers ?? new Headers(),
      });

      return { success: true };
    } catch (error) {
      console.error('❌ Multi-session set active failed:', error);
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to set active session',
      };
    }
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
