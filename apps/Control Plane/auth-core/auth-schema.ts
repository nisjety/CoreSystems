import {
  pgTable,
  text,
  bigint,
  timestamp,
  boolean,
  integer,
  uuid,
  unique,
  check,
} from 'drizzle-orm/pg-core';
import { sql } from 'drizzle-orm';

export const user = pgTable('user', {
  id: text('id').primaryKey(),
  name: text('name').notNull(),
  email: text('email').notNull().unique(),
  emailVerified: boolean('email_verified').default(false).notNull(),
  image: text('image'),
  createdAt: timestamp('created_at').defaultNow().notNull(),
  updatedAt: timestamp('updated_at')
    .defaultNow()
    .$onUpdate(() => /* @__PURE__ */ new Date())
    .notNull(),
  role: text('role'),
  banned: boolean('banned').default(false),
  banReason: text('ban_reason'),
  banExpires: timestamp('ban_expires'),
  twoFactorEnabled: boolean('two_factor_enabled').default(false),
  phoneNumber: text('phone_number').unique(),
  phoneNumberVerified: boolean('phone_number_verified'),
});

export const account = pgTable('account', {
  id: text('id').primaryKey(),
  accountId: text('account_id').notNull(),
  providerId: text('provider_id').notNull(),
  userId: text('user_id')
    .notNull()
    .references(() => user.id, { onDelete: 'cascade' }),
  accessToken: text('access_token'),
  refreshToken: text('refresh_token'),
  idToken: text('id_token'),
  accessTokenExpiresAt: timestamp('access_token_expires_at'),
  refreshTokenExpiresAt: timestamp('refresh_token_expires_at'),
  scope: text('scope'),
  password: text('password'),
  createdAt: timestamp('created_at').defaultNow().notNull(),
  updatedAt: timestamp('updated_at')
    .$onUpdate(() => /* @__PURE__ */ new Date())
    .notNull(),
});

export const verification = pgTable('verification', {
  id: text('id').primaryKey(),
  identifier: text('identifier').notNull(),
  value: text('value').notNull(),
  expiresAt: timestamp('expires_at').notNull(),
  createdAt: timestamp('created_at').defaultNow().notNull(),
  updatedAt: timestamp('updated_at')
    .defaultNow()
    .$onUpdate(() => /* @__PURE__ */ new Date())
    .notNull(),
});

export const organization = pgTable('organization', {
  id: text('id').primaryKey(),
  name: text('name').notNull(),
  slug: text('slug').unique(),
  logo: text('logo'),
  createdAt: timestamp('created_at').notNull(),
  metadata: text('metadata'),
});

export const member = pgTable('member', {
  id: text('id').primaryKey(),
  organizationId: text('organization_id')
    .notNull()
    .references(() => organization.id, { onDelete: 'cascade' }),
  userId: text('user_id')
    .notNull()
    .references(() => user.id, { onDelete: 'cascade' }),
  role: text('role').default('member').notNull(),
  createdAt: timestamp('created_at').notNull(),
});

export const invitation = pgTable('invitation', {
  id: text('id').primaryKey(),
  organizationId: text('organization_id')
    .notNull()
    .references(() => organization.id, { onDelete: 'cascade' }),
  email: text('email').notNull(),
  role: text('role'),
  status: text('status').default('pending').notNull(),
  expiresAt: timestamp('expires_at').notNull(),
  inviterId: text('inviter_id')
    .notNull()
    .references(() => user.id, { onDelete: 'cascade' }),
});

export const ssoProvider = pgTable('sso_provider', {
  id: text('id').primaryKey(),
  issuer: text('issuer').notNull(),
  oidcConfig: text('oidc_config'),
  samlConfig: text('saml_config'),
  userId: text('user_id').references(() => user.id, { onDelete: 'cascade' }),
  providerId: text('provider_id').notNull().unique(),
  organizationId: text('organization_id'),
  domain: text('domain').notNull(),
});

export const oauthApplication = pgTable('oauth_application', {
  id: text('id').primaryKey(),
  name: text('name'),
  icon: text('icon'),
  metadata: text('metadata'),
  clientId: text('client_id').unique(),
  clientSecret: text('client_secret'),
  redirectUrls: text('redirect_ur_ls'),
  authenticationScheme: text('authentication_scheme'),
  type: text('type'),
  disabled: boolean('disabled').default(false),
  userId: text('user_id').references(() => user.id, { onDelete: 'cascade' }),
  createdAt: timestamp('created_at'),
  updatedAt: timestamp('updated_at'),
});

export const oauthAccessToken = pgTable('oauth_access_token', {
  id: text('id').primaryKey(),
  accessToken: text('access_token').unique(),
  refreshToken: text('refresh_token').unique(),
  accessTokenExpiresAt: timestamp('access_token_expires_at'),
  refreshTokenExpiresAt: timestamp('refresh_token_expires_at'),
  clientId: text('client_id').references(() => oauthApplication.clientId, {
    onDelete: 'cascade',
  }),
  userId: text('user_id').references(() => user.id, { onDelete: 'cascade' }),
  scopes: text('scopes'),
  createdAt: timestamp('created_at'),
  updatedAt: timestamp('updated_at'),
});

export const oauthConsent = pgTable('oauth_consent', {
  id: text('id').primaryKey(),
  clientId: text('client_id').references(() => oauthApplication.clientId, {
    onDelete: 'cascade',
  }),
  userId: text('user_id').references(() => user.id, { onDelete: 'cascade' }),
  scopes: text('scopes'),
  createdAt: timestamp('created_at'),
  updatedAt: timestamp('updated_at'),
  consentGiven: boolean('consent_given'),
});

export const privacyConsent = pgTable('privacy_consent', {
  id: text('id').primaryKey(),
  userId: text('user_id').references(() => user.id, { onDelete: 'cascade' }),
  sessionId: text('session_id'),
  analytics: boolean('analytics').default(false).notNull(),
  marketing: boolean('marketing').default(false).notNull(),
  necessary: boolean('necessary').default(true).notNull(),
  createdAt: timestamp('created_at').defaultNow().notNull(),
  updatedAt: timestamp('updated_at')
    .defaultNow()
    .$onUpdate(() => /* @__PURE__ */ new Date())
    .notNull(),
});

export const apikey = pgTable('apikey', {
  id: text('id').primaryKey(),
  name: text('name'),
  start: text('start'),
  prefix: text('prefix'),
  key: text('key').notNull(),
  configId: text('config_id').notNull().default('user-keys'),
  referenceId: text('reference_id').notNull(),
  userId: text('user_id').references(() => user.id, { onDelete: 'cascade' }),
  refillInterval: integer('refill_interval'),
  refillAmount: integer('refill_amount'),
  lastRefillAt: timestamp('last_refill_at'),
  enabled: boolean('enabled').default(true),
  rateLimitEnabled: boolean('rate_limit_enabled').default(true),
  rateLimitTimeWindow: integer('rate_limit_time_window').default(86400000),
  rateLimitMax: integer('rate_limit_max').default(10),
  requestCount: integer('request_count').default(0),
  remaining: integer('remaining'),
  lastRequest: timestamp('last_request'),
  expiresAt: timestamp('expires_at'),
  createdAt: timestamp('created_at').notNull(),
  updatedAt: timestamp('updated_at').notNull(),
  permissions: text('permissions'),
  metadata: text('metadata'),
});

export const twoFactor = pgTable('two_factor', {
  id: text('id').primaryKey(),
  secret: text('secret').notNull(),
  backupCodes: text('backup_codes').notNull(),
  userId: text('user_id')
    .notNull()
    .references(() => user.id, { onDelete: 'cascade' }),
});

export const passkey = pgTable('passkey', {
  id: text('id').primaryKey(),
  name: text('name'),
  publicKey: text('public_key').notNull(),
  userId: text('user_id')
    .notNull()
    .references(() => user.id, { onDelete: 'cascade' }),
  credentialID: text('credential_id').notNull(),
  counter: integer('counter').notNull(),
  deviceType: text('device_type').notNull(),
  backedUp: boolean('backed_up').notNull(),
  transports: text('transports'),
  createdAt: timestamp('created_at'),
  aaguid: text('aaguid'),
});

export const session = pgTable('session', {
  id: text('id').primaryKey(),
  expiresAt: timestamp('expires_at').notNull(),
  token: text('token').notNull().unique(),
  createdAt: timestamp('created_at').notNull(),
  updatedAt: timestamp('updated_at').notNull(),
  ipAddress: text('ip_address'),
  userAgent: text('user_agent'),
  userId: text('user_id')
    .notNull()
    .references(() => user.id, { onDelete: 'cascade' }),
  activeOrganizationId: text('active_organization_id'),
  activeTeamId: text('active_team_id'),
  impersonatedBy: text('impersonated_by'),
});

export const jwks = pgTable('jwks', {
  id: text('id').primaryKey(),
  publicKey: text('public_key').notNull(),
  privateKey: text('private_key').notNull(),
  createdAt: timestamp('created_at').notNull(),
});

// D-A: an "account" grouping axis above org. Named org_group, not "account" —
// the `account` table above (Better Auth's per-user OAuth-provider rows)
// already uses that name for something unrelated. See migrations/028_org_group_grants.sql
// for the full rationale.
export const orgGroup = pgTable(
  'org_group',
  {
    id: uuid('id').primaryKey(),
    name: text('name').notNull(),
    hostOrganizationId: text('host_organization_id')
      .notNull()
      .references(() => organization.id, { onDelete: 'cascade' }),
    createdAt: timestamp('created_at', { withTimezone: true })
      .defaultNow()
      .notNull(),
    updatedAt: timestamp('updated_at', { withTimezone: true })
      .defaultNow()
      .$onUpdate(() => /* @__PURE__ */ new Date())
      .notNull(),
  },
  (table) => [unique('org_group_host_unique').on(table.hostOrganizationId)],
);

// A row's existence represents an active grant of some kind; revoking the
// last of the two flags should delete the row (see org_group_grant_has_a_grant).
export const orgGroupGrant = pgTable(
  'org_group_grant',
  {
    id: uuid('id').primaryKey(),
    orgGroupId: uuid('org_group_id')
      .notNull()
      .references(() => orgGroup.id, { onDelete: 'cascade' }),
    organizationId: text('organization_id')
      .notNull()
      .references(() => organization.id, { onDelete: 'cascade' }),
    dataAccess: boolean('data_access').default(false).notNull(),
    billingConsolidation: boolean('billing_consolidation')
      .default(false)
      .notNull(),
    grantedBy: text('granted_by')
      .notNull()
      .references(() => user.id),
    createdAt: timestamp('created_at', { withTimezone: true })
      .defaultNow()
      .notNull(),
    updatedAt: timestamp('updated_at', { withTimezone: true })
      .defaultNow()
      .$onUpdate(() => /* @__PURE__ */ new Date())
      .notNull(),
  },
  (table) => [
    unique('org_group_grant_unique').on(table.orgGroupId, table.organizationId),
    check(
      'org_group_grant_has_a_grant',
      sql`${table.dataAccess} OR ${table.billingConsolidation}`,
    ),
  ],
);

export const rateLimit = pgTable('rate_limit', {
  id: text('id').primaryKey(),
  key: text('key'),
  count: integer('count'),
  lastRequest: bigint('last_request', { mode: 'number' }),
});
