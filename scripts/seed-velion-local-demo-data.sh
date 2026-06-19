#!/usr/bin/env bash
set -euo pipefail

# Local-only demo seed for Velion v3.
# This seeds product data into the running Docker databases so the UI can use
# database-backed mock data instead of hardcoded frontend fallback records.

ORG_ID="${ORG_ID:-org_1781699307874}"
ORG_NAME="${ORG_NAME:-Velion AS}"
ORG_SLUG="${ORG_SLUG:-velion}"

ADMIN_USER_ID="${ADMIN_USER_ID:-mxDNys1MNRyda7LqdQzUtqx0wFRKxMIc}"
NORMAL_USER_ID="${NORMAL_USER_ID:-user_velion_normal}"
EDITOR_USER_ID="${EDITOR_USER_ID:-user_velion_editor}"

ADMIN_EMAIL="${ADMIN_EMAIL:-local@velion.dev}"
NORMAL_EMAIL="${NORMAL_EMAIL:-normal@velion.dev}"
EDITOR_EMAIL="${EDITOR_EMAIL:-editor@velion.dev}"

ADMIN_NAME="${ADMIN_NAME:-Velion Admin}"
NORMAL_NAME="${NORMAL_NAME:-Velion Normal}"
EDITOR_NAME="${EDITOR_NAME:-Velion Editor}"

require_container() {
  local container="$1"
  if ! docker inspect "$container" >/dev/null 2>&1; then
    echo "Missing Docker container: $container" >&2
    echo "Start the local Velion stack before running this seed." >&2
    exit 1
  fi
}

run_psql() {
  local container="$1"
  local user="$2"
  local database="$3"
  shift 3
  docker exec -i "$container" psql \
    -v ON_ERROR_STOP=1 \
    -v org_id="$ORG_ID" \
    -v org_name="$ORG_NAME" \
    -v org_slug="$ORG_SLUG" \
    -v admin_user_id="$ADMIN_USER_ID" \
    -v normal_user_id="$NORMAL_USER_ID" \
    -v editor_user_id="$EDITOR_USER_ID" \
    -v admin_email="$ADMIN_EMAIL" \
    -v normal_email="$NORMAL_EMAIL" \
    -v editor_email="$EDITOR_EMAIL" \
    -v admin_name="$ADMIN_NAME" \
    -v normal_name="$NORMAL_NAME" \
    -v editor_name="$EDITOR_NAME" \
    -U "$user" \
    -d "$database" \
    "$@"
}

seed_auth_service() {
  echo "Seeding auth_service..."
  run_psql controlplane-postgres aquatiq auth_service <<'SQL'
BEGIN;

UPDATE organization
SET slug = slug || '-archived-' || substr(id, 1, 8)
WHERE slug = :'org_slug'
  AND id <> :'org_id';

INSERT INTO "user" (id, name, email, email_verified, image, role, banned, created_at, updated_at)
VALUES
  (:'admin_user_id', :'admin_name', :'admin_email', true, null, 'superadmin', false, now() - interval '14 days', now()),
  (:'normal_user_id', :'normal_name', :'normal_email', true, null, 'user', false, now() - interval '10 days', now()),
  (:'editor_user_id', :'editor_name', :'editor_email', true, null, 'editor', false, now() - interval '9 days', now())
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    email = EXCLUDED.email,
    email_verified = true,
    role = EXCLUDED.role,
    banned = false,
    updated_at = now();

WITH local_password AS (
  SELECT password
  FROM account
  WHERE user_id = :'admin_user_id'
    AND provider_id = 'credential'
    AND password IS NOT NULL
  ORDER BY created_at DESC
  LIMIT 1
)
INSERT INTO account (id, account_id, provider_id, user_id, password, created_at, updated_at)
SELECT id, account_id, 'credential', user_id, COALESCE((SELECT password FROM local_password), ''), now() - interval '9 days', now()
FROM (
  VALUES
    ('acct_velion_normal', :'normal_user_id', :'normal_user_id'),
    ('acct_velion_editor', :'editor_user_id', :'editor_user_id')
) AS seed(id, account_id, user_id)
ON CONFLICT (id) DO UPDATE
SET account_id = EXCLUDED.account_id,
    provider_id = EXCLUDED.provider_id,
    user_id = EXCLUDED.user_id,
    password = EXCLUDED.password,
    updated_at = now();

INSERT INTO organization (id, name, slug, logo, metadata, created_at)
VALUES (
  :'org_id',
  :'org_name',
  :'org_slug',
  null,
  jsonb_build_object(
    'seed', 'velion-local-demo',
    'plan', 'standard',
    'plan_label', 'Velion Advanced',
    'locale', 'nb-NO'
  )::text,
  now() - interval '14 days'
)
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    slug = EXCLUDED.slug,
    metadata = EXCLUDED.metadata;

INSERT INTO member (id, organization_id, user_id, role, created_at)
VALUES
  ('member_velion_admin', :'org_id', :'admin_user_id', 'admin', now() - interval '14 days'),
  ('member_velion_normal', :'org_id', :'normal_user_id', 'member', now() - interval '9 days'),
  ('member_velion_editor', :'org_id', :'editor_user_id', 'editor', now() - interval '8 days')
ON CONFLICT (id) DO UPDATE
SET organization_id = EXCLUDED.organization_id,
    user_id = EXCLUDED.user_id,
    role = EXCLUDED.role;

COMMIT;
SQL
}

seed_org_core() {
  echo "Seeding org_core..."
  run_psql controlplane-postgres aquatiq org_core <<'SQL'
BEGIN;

UPDATE organizations
SET slug = slug || '-archived-' || substr(id, 1, 8),
    updated_at = now()
WHERE slug = :'org_slug'
  AND id <> :'org_id';

INSERT INTO organizations (
  id,
  name,
  slug,
  plan,
  status,
  metadata,
  org_number,
  verification_status,
  primary_domain,
  region,
  default_locale,
  created_at,
  updated_at
)
VALUES (
  :'org_id',
  :'org_name',
  :'org_slug',
  'standard',
  'active',
  jsonb_build_object(
    'seed', 'velion-local-demo',
    'plan_label', 'Velion Advanced',
    'mock_data', true,
    'support_timezone', 'Europe/Oslo'
  ),
  '999888777',
  'verified',
  'velion.dev',
  'eu',
  'nb-NO',
  now() - interval '14 days',
  now()
)
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    slug = EXCLUDED.slug,
    plan = EXCLUDED.plan,
    status = EXCLUDED.status,
    metadata = EXCLUDED.metadata,
    org_number = EXCLUDED.org_number,
    verification_status = EXCLUDED.verification_status,
    primary_domain = EXCLUDED.primary_domain,
    region = EXCLUDED.region,
    default_locale = EXCLUDED.default_locale,
    deleted_at = null,
    updated_at = now();

INSERT INTO organization_members (
  id,
  org_id,
  user_id,
  role,
  status,
  invited_by,
  invited_at,
  joined_at,
  created_at,
  updated_at,
  invited_email
)
VALUES
  ('orgmem_velion_admin', :'org_id', :'admin_user_id', 'admin', 'active', null, null, now() - interval '14 days', now() - interval '14 days', now(), :'admin_email'),
  ('orgmem_velion_normal', :'org_id', :'normal_user_id', 'member', 'active', :'admin_user_id', now() - interval '9 days', now() - interval '9 days', now() - interval '9 days', now(), :'normal_email'),
  ('orgmem_velion_editor', :'org_id', :'editor_user_id', 'editor', 'active', :'admin_user_id', now() - interval '8 days', now() - interval '8 days', now() - interval '8 days', now(), :'editor_email')
ON CONFLICT (org_id, user_id) DO UPDATE
SET role = EXCLUDED.role,
    status = EXCLUDED.status,
    invited_by = EXCLUDED.invited_by,
    invited_at = EXCLUDED.invited_at,
    joined_at = EXCLUDED.joined_at,
    invited_email = EXCLUDED.invited_email,
    updated_at = now();

INSERT INTO org_billing (
  org_id,
  billing_email,
  subscription_id,
  subscription_status,
  trial_ends_at,
  current_period_start,
  current_period_end,
  auto_renew,
  billing_address,
  tax_id,
  created_at,
  updated_at
)
VALUES (
  :'org_id',
  'billing@velion.dev',
  'sub_velion_advanced_demo',
  'active',
  null,
  date_trunc('month', now()),
  date_trunc('month', now()) + interval '1 month',
  true,
  jsonb_build_object('country', 'NO', 'city', 'Oslo', 'line1', 'Demo gate 1'),
  'NO999888777MVA',
  now() - interval '14 days',
  now()
)
ON CONFLICT (org_id) DO UPDATE
SET billing_email = EXCLUDED.billing_email,
    subscription_id = EXCLUDED.subscription_id,
    subscription_status = EXCLUDED.subscription_status,
    trial_ends_at = EXCLUDED.trial_ends_at,
    current_period_start = EXCLUDED.current_period_start,
    current_period_end = EXCLUDED.current_period_end,
    auto_renew = EXCLUDED.auto_renew,
    billing_address = EXCLUDED.billing_address,
    tax_id = EXCLUDED.tax_id,
    updated_at = now();

INSERT INTO org_compliance (
  org_id,
  data_residency,
  gdpr_compliant,
  soc2_compliant,
  data_retention_days,
  require_mfa,
  ip_allowlist,
  audit_log_retention_days,
  encryption_at_rest,
  encryption_in_transit,
  created_at,
  updated_at
)
VALUES (
  :'org_id',
  'eu',
  true,
  true,
  365,
  false,
  '[]'::jsonb,
  365,
  true,
  true,
  now() - interval '14 days',
  now()
)
ON CONFLICT (org_id) DO UPDATE
SET data_residency = EXCLUDED.data_residency,
    gdpr_compliant = EXCLUDED.gdpr_compliant,
    soc2_compliant = EXCLUDED.soc2_compliant,
    data_retention_days = EXCLUDED.data_retention_days,
    require_mfa = EXCLUDED.require_mfa,
    ip_allowlist = EXCLUDED.ip_allowlist,
    audit_log_retention_days = EXCLUDED.audit_log_retention_days,
    encryption_at_rest = EXCLUDED.encryption_at_rest,
    encryption_in_transit = EXCLUDED.encryption_in_transit,
    updated_at = now();

INSERT INTO org_quotas (org_id, quota_key, quota_value, quota_limit, reset_period, last_reset_at, updated_at)
VALUES
  (:'org_id', 'users', 3, 25, 'monthly', date_trunc('month', now()), now()),
  (:'org_id', 'api_calls', 1284, 100000, 'monthly', date_trunc('month', now()), now()),
  (:'org_id', 'storage_mb', 8192, 102400, 'monthly', date_trunc('month', now()), now()),
  (:'org_id', 'tickets', 6, 5000, 'monthly', date_trunc('month', now()), now()),
  (:'org_id', 'documents', 3, 20000, 'monthly', date_trunc('month', now()), now())
ON CONFLICT (org_id, quota_key) DO UPDATE
SET quota_value = EXCLUDED.quota_value,
    quota_limit = EXCLUDED.quota_limit,
    reset_period = EXCLUDED.reset_period,
    last_reset_at = EXCLUDED.last_reset_at,
    updated_at = now();

INSERT INTO org_entitlements (org_id, entitlement_key, enabled, updated_at)
VALUES
  (:'org_id', 'feature.chat', true, now()),
  (:'org_id', 'feature.audit_logs', true, now()),
  (:'org_id', 'feature.api_keys', true, now()),
  (:'org_id', 'feature.sso', true, now()),
  (:'org_id', 'feature.integrations', true, now()),
  (:'org_id', 'feature.advanced_ticketing', true, now()),
  (:'org_id', 'feature.knowledge_base', true, now()),
  (:'org_id', 'feature.social_planner', true, now())
ON CONFLICT (org_id, entitlement_key) DO UPDATE
SET enabled = EXCLUDED.enabled,
    updated_at = now();

INSERT INTO org_plan_history (id, org_id, previous_plan, new_plan, changed_by, change_reason, changed_at, metadata)
VALUES (
  'planhist_velion_advanced_seed',
  :'org_id',
  'trial',
  'standard',
  :'admin_user_id',
  'Local demo seed sets Velion AS to the Advanced product tier.',
  now() - interval '14 days',
  jsonb_build_object('seed', 'velion-local-demo', 'plan_label', 'Velion Advanced')
)
ON CONFLICT (id) DO UPDATE
SET previous_plan = EXCLUDED.previous_plan,
    new_plan = EXCLUDED.new_plan,
    changed_by = EXCLUDED.changed_by,
    change_reason = EXCLUDED.change_reason,
    metadata = EXCLUDED.metadata;

COMMIT;
SQL
}

seed_user_service() {
  echo "Seeding user_service..."
  run_psql controlplane-postgres aquatiq user_service <<'SQL'
BEGIN;

INSERT INTO users (
  id,
  email,
  name,
  avatar,
  status,
  email_verified,
  created_at,
  updated_at,
  last_login_at,
  onboarding_complete,
  onboarding_step,
  onboarding_state
)
VALUES
  (:'admin_user_id', :'admin_email', :'admin_name', '', 'active', true, now() - interval '14 days', now(), now() - interval '1 hour', true, 'completed', jsonb_build_object('seed', 'velion-local-demo', 'role', 'admin')),
  (:'normal_user_id', :'normal_email', :'normal_name', '', 'active', true, now() - interval '9 days', now(), now() - interval '3 hours', true, 'completed', jsonb_build_object('seed', 'velion-local-demo', 'role', 'member')),
  (:'editor_user_id', :'editor_email', :'editor_name', '', 'active', true, now() - interval '8 days', now(), now() - interval '2 hours', true, 'completed', jsonb_build_object('seed', 'velion-local-demo', 'role', 'editor'))
ON CONFLICT (id) DO UPDATE
SET email = EXCLUDED.email,
    name = EXCLUDED.name,
    avatar = EXCLUDED.avatar,
    status = EXCLUDED.status,
    email_verified = EXCLUDED.email_verified,
    last_login_at = EXCLUDED.last_login_at,
    onboarding_complete = EXCLUDED.onboarding_complete,
    onboarding_step = EXCLUDED.onboarding_step,
    onboarding_state = EXCLUDED.onboarding_state,
    updated_at = now();

INSERT INTO user_profiles (user_id, bio, phone, location, timezone, language, metadata, updated_at)
VALUES
  (:'admin_user_id', 'Local Velion administrator for testing organization, billing, and support workflows.', '+47 400 00 001', 'Oslo, Norway', 'Europe/Oslo', 'nb', jsonb_build_object('department', 'Operations', 'seed', 'velion-local-demo'), now()),
  (:'normal_user_id', 'Support specialist with customer ticket and knowledge-base test data.', '+47 400 00 002', 'Bergen, Norway', 'Europe/Oslo', 'nb', jsonb_build_object('department', 'Support', 'seed', 'velion-local-demo'), now()),
  (:'editor_user_id', 'Content editor for social planning and approval workflow testing.', '+47 400 00 003', 'Trondheim, Norway', 'Europe/Oslo', 'nb', jsonb_build_object('department', 'Content', 'seed', 'velion-local-demo'), now())
ON CONFLICT (user_id) DO UPDATE
SET bio = EXCLUDED.bio,
    phone = EXCLUDED.phone,
    location = EXCLUDED.location,
    timezone = EXCLUDED.timezone,
    language = EXCLUDED.language,
    metadata = EXCLUDED.metadata,
    updated_at = now();

INSERT INTO user_org_memberships (id, user_id, org_id, role, status, invited_by, created_at, updated_at)
VALUES
  ('uom_velion_admin', :'admin_user_id', :'org_id', 'admin', 'active', null, now() - interval '14 days', now()),
  ('uom_velion_normal', :'normal_user_id', :'org_id', 'member', 'active', :'admin_user_id', now() - interval '9 days', now()),
  ('uom_velion_editor', :'editor_user_id', :'org_id', 'editor', 'active', :'admin_user_id', now() - interval '8 days', now())
ON CONFLICT (user_id, org_id) DO UPDATE
SET role = EXCLUDED.role,
    status = EXCLUDED.status,
    invited_by = EXCLUDED.invited_by,
    updated_at = now();

INSERT INTO roles (id, name, description, permissions, created_at, updated_at)
VALUES
  ('role_velion_admin', 'velion_admin', 'Velion local demo administrator', jsonb_build_array('org:manage', 'billing:manage', 'tickets:manage', 'knowledge:manage', 'social:approve'), now(), now()),
  ('role_velion_member', 'velion_member', 'Velion local demo normal user', jsonb_build_array('tickets:read', 'tickets:reply', 'knowledge:read'), now(), now()),
  ('role_velion_editor', 'velion_editor', 'Velion local demo editor', jsonb_build_array('tickets:read', 'knowledge:write', 'social:write'), now(), now())
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    description = EXCLUDED.description,
    permissions = EXCLUDED.permissions,
    updated_at = now();

INSERT INTO user_roles (user_id, role_id, assigned_at)
VALUES
  (:'admin_user_id', 'role_velion_admin', now() - interval '14 days'),
  (:'normal_user_id', 'role_velion_member', now() - interval '9 days'),
  (:'editor_user_id', 'role_velion_editor', now() - interval '8 days')
ON CONFLICT (user_id, role_id) DO UPDATE
SET assigned_at = EXCLUDED.assigned_at;

INSERT INTO user_settings (id, user_id, category, settings, created_at, updated_at)
VALUES
  ('settings_velion_admin_ui', :'admin_user_id', 'ui', jsonb_build_object('theme', 'system', 'locale', 'nb-NO', 'ticket_queue', 'my'), now(), now()),
  ('settings_velion_normal_ui', :'normal_user_id', 'ui', jsonb_build_object('theme', 'light', 'locale', 'nb-NO', 'ticket_queue', 'waiting-customer'), now(), now()),
  ('settings_velion_editor_ui', :'editor_user_id', 'ui', jsonb_build_object('theme', 'light', 'locale', 'nb-NO', 'ticket_queue', 'suggested'), now(), now())
ON CONFLICT (id) DO UPDATE
SET settings = EXCLUDED.settings,
    updated_at = now();

COMMIT;
SQL
}

seed_billing_service() {
  echo "Seeding billing_service..."
  run_psql controlplane-postgres aquatiq billing_service <<'SQL'
BEGIN;

INSERT INTO billing_accounts (
  org_id,
  plan,
  subscription_state,
  credits,
  products,
  feature_flags,
  entitlements,
  quota_limits,
  provider_customer_id,
  metadata,
  created_at,
  updated_at,
  trial_ends_at
)
VALUES (
  :'org_id',
  'standard',
  'active',
  250000,
  jsonb_build_object('suite', 'velion', 'tier', 'advanced', 'seats', 25),
  jsonb_build_object('advanced_ticketing', true, 'knowledge_base', true, 'social_planner', true, 'web_search', true),
  jsonb_build_object(
    'feature.chat', true,
    'feature.audit_logs', true,
    'feature.api_keys', true,
    'feature.sso', true,
    'feature.integrations', true,
    'feature.advanced_ticketing', true,
    'feature.knowledge_base', true,
    'feature.social_planner', true
  ),
  jsonb_build_object('users', 25, 'api_calls', 100000, 'storage_mb', 102400, 'tickets', 5000, 'documents', 20000),
  jsonb_build_object('local_demo', 'cus_velion_advanced_demo'),
  jsonb_build_object('seed', 'velion-local-demo', 'org_name', :'org_name', 'plan_label', 'Velion Advanced'),
  now() - interval '14 days',
  now(),
  null
)
ON CONFLICT (org_id) DO UPDATE
SET plan = EXCLUDED.plan,
    subscription_state = EXCLUDED.subscription_state,
    credits = EXCLUDED.credits,
    products = EXCLUDED.products,
    feature_flags = EXCLUDED.feature_flags,
    entitlements = EXCLUDED.entitlements,
    quota_limits = EXCLUDED.quota_limits,
    provider_customer_id = EXCLUDED.provider_customer_id,
    metadata = EXCLUDED.metadata,
    trial_ends_at = EXCLUDED.trial_ends_at,
    updated_at = now();

INSERT INTO billing_invoices (invoice_id, org_id, provider, amount_cents, currency, status, issued_at, due_at, metadata, created_at, last_modified)
VALUES
  ('inv_velion_advanced_202606', :'org_id', 'local_demo', 249900, 'NOK', 'paid', date_trunc('month', now()), date_trunc('month', now()) + interval '14 days', jsonb_build_object('plan_label', 'Velion Advanced', 'seats', 3), now() - interval '14 days', now()),
  ('inv_velion_advanced_202605', :'org_id', 'local_demo', 249900, 'NOK', 'paid', date_trunc('month', now()) - interval '1 month', date_trunc('month', now()) - interval '1 month' + interval '14 days', jsonb_build_object('plan_label', 'Velion Advanced', 'seats', 3), now() - interval '1 month', now())
ON CONFLICT (invoice_id) DO UPDATE
SET org_id = EXCLUDED.org_id,
    provider = EXCLUDED.provider,
    amount_cents = EXCLUDED.amount_cents,
    currency = EXCLUDED.currency,
    status = EXCLUDED.status,
    issued_at = EXCLUDED.issued_at,
    due_at = EXCLUDED.due_at,
    metadata = EXCLUDED.metadata,
    last_modified = now();

INSERT INTO billing_usage_events (org_id, metric, quantity, source, occurred_at, metadata, created_at)
SELECT :'org_id', metric, quantity, 'velion-local-demo', occurred_at, metadata, now()
FROM (
  VALUES
    ('tickets.created', 6::double precision, now() - interval '2 hours', jsonb_build_object('seed', 'velion-local-demo')),
    ('messages.processed', 18::double precision, now() - interval '90 minutes', jsonb_build_object('seed', 'velion-local-demo')),
    ('documents.indexed', 3::double precision, now() - interval '80 minutes', jsonb_build_object('seed', 'velion-local-demo')),
    ('model.tokens', 7842::double precision, now() - interval '45 minutes', jsonb_build_object('seed', 'velion-local-demo'))
) AS seed(metric, quantity, occurred_at, metadata)
WHERE NOT EXISTS (
  SELECT 1
  FROM billing_usage_events e
  WHERE e.org_id = :'org_id'
    AND e.source = 'velion-local-demo'
    AND e.metric = seed.metric
);

COMMIT;
SQL
}

seed_integration_core() {
  echo "Seeding integration..."
  run_psql ingestion-postgres ingestion_user integration <<'SQL'
BEGIN;

INSERT INTO integration_connections (
  id,
  provider_key,
  connector_type,
  organization_id,
  workspace_id,
  user_id,
  user_email,
  status,
  display_name,
  provider_account_id,
  tenant_id,
  provider_context,
  capabilities,
  scopes,
  encrypted_access_token,
  encrypted_refresh_token,
  access_token_expires_at,
  last_refreshed_at,
  last_sync_status,
  created_at,
  updated_at,
  deleted_at
)
VALUES
  ('conn_velion_microsoft', 'microsoft', 'microsoft-graph', :'org_id', 'workspace_velion_demo', :'admin_user_id', :'admin_email', 'connected', 'Velion Microsoft 365', 'velion-as.onmicrosoft.com', 'tenant_velion_demo', jsonb_build_object('tenant_name', 'Velion AS', 'primary_domain', 'velion.dev'), ARRAY['profile.read','sharepoint.read','teams.read','mail.read','mail.send','calendar.read']::text[], ARRAY['openid','profile','email','offline_access','User.Read','Files.Read.All','Sites.Read.All','Mail.Read','Mail.Send','Calendars.Read']::text[], 'local-demo-token-microsoft', 'local-demo-refresh-microsoft', now() + interval '30 days', now() - interval '35 minutes', 'completed', now() - interval '10 days', now(), null),
  ('conn_velion_google', 'google', 'google-workspace', :'org_id', 'workspace_velion_demo', :'admin_user_id', :'admin_email', 'connected', 'Velion Google Workspace', 'admin@velion.dev', 'google_workspace_velion_demo', jsonb_build_object('workspace_domain', 'velion.dev'), ARRAY['profile.read','drive.metadata','drive.read','gmail.read','gmail.send','calendar.read']::text[], ARRAY['openid','email','profile','https://www.googleapis.com/auth/drive.readonly','https://www.googleapis.com/auth/gmail.readonly','https://www.googleapis.com/auth/gmail.send','https://www.googleapis.com/auth/calendar.readonly']::text[], 'local-demo-token-google', 'local-demo-refresh-google', now() + interval '30 days', now() - interval '32 minutes', 'completed', now() - interval '9 days', now(), null),
  ('conn_velion_slack', 'slack', 'slack', :'org_id', 'workspace_velion_demo', :'normal_user_id', :'normal_email', 'connected', 'Velion Slack', 'T-VELION-DEMO', 'slack_team_velion_demo', jsonb_build_object('team_name', 'Velion AS', 'workspace_url', 'velion-demo.slack.com'), ARRAY['workspace.read','users.read','channels.read','channels.history','files.read','messages.write']::text[], ARRAY['team:read','users:read','channels:read','channels:history','files:read','chat:write']::text[], 'local-demo-token-slack', 'local-demo-refresh-slack', now() + interval '30 days', now() - interval '28 minutes', 'completed', now() - interval '8 days', now(), null),
  ('conn_velion_github', 'github', 'github', :'org_id', 'workspace_velion_demo', :'admin_user_id', :'admin_email', 'connected', 'Velion GitHub', 'triodelab/velion-demo', 'github_install_velion_demo', jsonb_build_object('installation_id', '1234567', 'org', 'triodelab'), ARRAY['profile.read','org.read','repo.public.read','repo.private.read','issues.write']::text[], ARRAY['read:org','repo','write:discussion']::text[], 'local-demo-token-github', 'local-demo-refresh-github', now() + interval '30 days', now() - interval '25 minutes', 'completed', now() - interval '8 days', now(), null),
  ('conn_velion_linkedin', 'linkedin', 'linkedin', :'org_id', 'workspace_velion_demo', :'editor_user_id', :'editor_email', 'connected', 'Velion LinkedIn', 'urn:li:organization:997711', 'linkedin_org_velion_demo', jsonb_build_object('handle', 'Velion AS', 'author_urn', 'urn:li:organization:997711', 'organization_urn', 'urn:li:organization:997711'), ARRAY['social.profile.read','social.post.write','social.media.upload','social.analytics.read']::text[], ARRAY['openid','profile','w_member_social','r_organization_social','rw_organization_admin']::text[], 'local-demo-token-linkedin', 'local-demo-refresh-linkedin', now() + interval '30 days', now() - interval '22 minutes', 'completed', now() - interval '7 days', now(), null),
  ('conn_velion_x', 'x', 'x', :'org_id', 'workspace_velion_demo', :'editor_user_id', :'editor_email', 'connected', 'Velion X', '@veliondemo', 'x_account_velion_demo', jsonb_build_object('handle', '@veliondemo', 'user_id', '900100200'), ARRAY['social.profile.read','social.post.write','social.media.upload','social.analytics.read']::text[], ARRAY['tweet.read','tweet.write','users.read','offline.access']::text[], 'local-demo-token-x', 'local-demo-refresh-x', now() + interval '30 days', now() - interval '20 minutes', 'completed', now() - interval '7 days', now(), null),
  ('conn_velion_instagram', 'instagram', 'instagram', :'org_id', 'workspace_velion_demo', :'editor_user_id', :'editor_email', 'connected', 'Velion Instagram', '@veliondemo', 'ig_business_velion_demo', jsonb_build_object('handle', '@veliondemo', 'ig_user_id', '17841400000000000', 'page_id', '1122334455'), ARRAY['social.profile.read','social.post.write','social.media.upload','social.analytics.read']::text[], ARRAY['instagram_basic','instagram_content_publish','pages_read_engagement','pages_show_list']::text[], 'local-demo-token-instagram', 'local-demo-refresh-instagram', now() + interval '30 days', now() - interval '18 minutes', 'completed', now() - interval '7 days', now(), null),
  ('conn_velion_facebook', 'facebook', 'facebook', :'org_id', 'workspace_velion_demo', :'editor_user_id', :'editor_email', 'connected', 'Velion Facebook', 'Velion AS', 'fb_page_velion_demo', jsonb_build_object('handle', 'Velion AS', 'page_id', '1122334455', 'page_access_token_ref', 'local-demo'), ARRAY['social.profile.read','social.post.write','social.media.upload','social.inbox.read','social.analytics.read']::text[], ARRAY['pages_read_engagement','pages_manage_posts','pages_messaging','pages_show_list']::text[], 'local-demo-token-facebook', 'local-demo-refresh-facebook', now() + interval '30 days', now() - interval '16 minutes', 'completed', now() - interval '7 days', now(), null),
  ('conn_velion_tiktok', 'tiktok', 'tiktok', :'org_id', 'workspace_velion_demo', :'editor_user_id', :'editor_email', 'connected', 'Velion TikTok', '@veliondemo', 'tiktok_business_velion_demo', jsonb_build_object('handle', '@veliondemo', 'open_id', 'tt_velion_demo'), ARRAY['social.profile.read','social.post.write','social.media.upload','social.analytics.read']::text[], ARRAY['user.info.basic','video.publish','video.upload']::text[], 'local-demo-token-tiktok', 'local-demo-refresh-tiktok', now() + interval '30 days', now() - interval '14 minutes', 'completed', now() - interval '6 days', now(), null),
  ('conn_velion_snapchat', 'snapchat', 'snapchat', :'org_id', 'workspace_velion_demo', :'editor_user_id', :'editor_email', 'connected', 'Velion Snapchat', 'veliondemo', 'snap_ad_account_velion_demo', jsonb_build_object('handle', 'veliondemo', 'ad_account_id', 'snap-ad-velion-demo'), ARRAY['social.profile.read','social.ads.manage','social.analytics.read']::text[], ARRAY['snapchat-marketing-api']::text[], 'local-demo-token-snapchat', 'local-demo-refresh-snapchat', now() + interval '30 days', now() - interval '12 minutes', 'completed', now() - interval '6 days', now(), null)
ON CONFLICT (id) DO UPDATE
SET status = EXCLUDED.status,
    display_name = EXCLUDED.display_name,
    provider_account_id = EXCLUDED.provider_account_id,
    tenant_id = EXCLUDED.tenant_id,
    provider_context = EXCLUDED.provider_context,
    capabilities = EXCLUDED.capabilities,
    scopes = EXCLUDED.scopes,
    encrypted_access_token = EXCLUDED.encrypted_access_token,
    encrypted_refresh_token = EXCLUDED.encrypted_refresh_token,
    access_token_expires_at = EXCLUDED.access_token_expires_at,
    last_refreshed_at = EXCLUDED.last_refreshed_at,
    last_sync_status = EXCLUDED.last_sync_status,
    updated_at = now(),
    deleted_at = null;

INSERT INTO integration_sync_jobs (
  id,
  organization_id,
  connection_id,
  user_id,
  provider_key,
  status,
  reason,
  mode,
  checkpoint,
  metadata,
  created_at,
  updated_at,
  started_at,
  completed_at
)
VALUES
  ('sync_velion_microsoft_initial', :'org_id', 'conn_velion_microsoft', :'admin_user_id', 'microsoft', 'completed', 'local_demo_seed', 'incremental', jsonb_build_object('documents', 42, 'mailboxes', 2), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '8 days', now() - interval '35 minutes', now() - interval '8 days', now() - interval '35 minutes'),
  ('sync_velion_google_initial', :'org_id', 'conn_velion_google', :'admin_user_id', 'google', 'completed', 'local_demo_seed', 'incremental', jsonb_build_object('drive_files', 31, 'gmail_threads', 9), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '8 days', now() - interval '32 minutes', now() - interval '8 days', now() - interval '32 minutes'),
  ('sync_velion_slack_initial', :'org_id', 'conn_velion_slack', :'normal_user_id', 'slack', 'completed', 'local_demo_seed', 'incremental', jsonb_build_object('channels', 6, 'messages', 128), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '7 days', now() - interval '28 minutes', now() - interval '7 days', now() - interval '28 minutes'),
  ('sync_velion_social_daily', :'org_id', 'conn_velion_instagram', :'editor_user_id', 'instagram', 'completed', 'local_demo_seed', 'analytics_refresh', jsonb_build_object('accounts', 6, 'posts', 7), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '6 hours', now() - interval '18 minutes', now() - interval '6 hours', now() - interval '18 minutes'),
  ('sync_velion_social_x', :'org_id', 'conn_velion_x', :'editor_user_id', 'x', 'completed', 'local_demo_seed', 'analytics_refresh', jsonb_build_object('tweets', 4), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '5 hours', now() - interval '20 minutes', now() - interval '5 hours', now() - interval '20 minutes'),
  ('sync_velion_social_linkedin', :'org_id', 'conn_velion_linkedin', :'editor_user_id', 'linkedin', 'completed', 'local_demo_seed', 'analytics_refresh', jsonb_build_object('organization_posts', 5), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '5 hours', now() - interval '22 minutes', now() - interval '5 hours', now() - interval '22 minutes')
ON CONFLICT (id) DO UPDATE
SET status = EXCLUDED.status,
    reason = EXCLUDED.reason,
    mode = EXCLUDED.mode,
    checkpoint = EXCLUDED.checkpoint,
    metadata = EXCLUDED.metadata,
    updated_at = EXCLUDED.updated_at,
    started_at = EXCLUDED.started_at,
    completed_at = EXCLUDED.completed_at;

INSERT INTO integration_sync_events (id, job_id, type, message, metadata, created_at)
VALUES
  ('syncevent_velion_microsoft_done', 'sync_velion_microsoft_initial', 'completed', 'Microsoft demo sync completed.', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '35 minutes'),
  ('syncevent_velion_google_done', 'sync_velion_google_initial', 'completed', 'Google demo sync completed.', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '32 minutes'),
  ('syncevent_velion_slack_done', 'sync_velion_slack_initial', 'completed', 'Slack demo sync completed.', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '28 minutes'),
  ('syncevent_velion_social_done', 'sync_velion_social_daily', 'completed', 'Social analytics demo sync completed.', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '18 minutes')
ON CONFLICT (id) DO UPDATE
SET type = EXCLUDED.type,
    message = EXCLUDED.message,
    metadata = EXCLUDED.metadata,
    created_at = EXCLUDED.created_at;

INSERT INTO integration_connection_consents (
  id,
  organization_id,
  connection_id,
  user_id,
  provider_key,
  source,
  purpose,
  granted,
  metadata,
  expires_at,
  created_at,
  updated_at,
  revoked_at
)
VALUES
  ('consent_velion_linkedin_publish', :'org_id', 'conn_velion_linkedin', :'editor_user_id', 'linkedin', 'organization_page', 'social_publish', true, jsonb_build_object('seed', 'velion-local-demo'), now() + interval '365 days', now() - interval '7 days', now(), null),
  ('consent_velion_instagram_publish', :'org_id', 'conn_velion_instagram', :'editor_user_id', 'instagram', 'business_account', 'social_publish', true, jsonb_build_object('seed', 'velion-local-demo'), now() + interval '365 days', now() - interval '7 days', now(), null),
  ('consent_velion_facebook_inbox', :'org_id', 'conn_velion_facebook', :'editor_user_id', 'facebook', 'page_inbox', 'social_inbox_triage', true, jsonb_build_object('seed', 'velion-local-demo'), now() + interval '365 days', now() - interval '7 days', now(), null),
  ('consent_velion_x_publish', :'org_id', 'conn_velion_x', :'editor_user_id', 'x', 'profile', 'social_publish', true, jsonb_build_object('seed', 'velion-local-demo'), now() + interval '365 days', now() - interval '7 days', now(), null),
  ('consent_velion_microsoft_knowledge', :'org_id', 'conn_velion_microsoft', :'admin_user_id', 'microsoft', 'sharepoint', 'knowledge_sync', true, jsonb_build_object('seed', 'velion-local-demo'), now() + interval '365 days', now() - interval '10 days', now(), null),
  ('consent_velion_google_knowledge', :'org_id', 'conn_velion_google', :'admin_user_id', 'google', 'drive', 'knowledge_sync', true, jsonb_build_object('seed', 'velion-local-demo'), now() + interval '365 days', now() - interval '9 days', now(), null)
ON CONFLICT (connection_id, source, purpose) DO UPDATE
SET granted = EXCLUDED.granted,
    metadata = EXCLUDED.metadata,
    expires_at = EXCLUDED.expires_at,
    updated_at = now(),
    revoked_at = EXCLUDED.revoked_at;

INSERT INTO integration_token_leases (id, organization_id, connection_id, user_id, provider_key, connector_type, consumer, expires_at, created_at)
VALUES
  ('lease_velion_social_core_linkedin', :'org_id', 'conn_velion_linkedin', :'editor_user_id', 'linkedin', 'linkedin', 'social-core', now() + interval '15 minutes', now() - interval '5 minutes'),
  ('lease_velion_social_core_instagram', :'org_id', 'conn_velion_instagram', :'editor_user_id', 'instagram', 'instagram', 'social-core', now() + interval '15 minutes', now() - interval '5 minutes'),
  ('lease_velion_social_core_x', :'org_id', 'conn_velion_x', :'editor_user_id', 'x', 'x', 'social-core', now() + interval '15 minutes', now() - interval '5 minutes')
ON CONFLICT (id) DO UPDATE
SET expires_at = EXCLUDED.expires_at,
    created_at = EXCLUDED.created_at;

INSERT INTO integration_audit_events (id, organization_id, user_id, connection_id, event_type, provider_key, metadata, created_at)
VALUES
  ('intaudit_velion_linkedin_connected', :'org_id', :'editor_user_id', 'conn_velion_linkedin', 'velion.ingestion.integration.connected', 'linkedin', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '7 days'),
  ('intaudit_velion_instagram_connected', :'org_id', :'editor_user_id', 'conn_velion_instagram', 'velion.ingestion.integration.connected', 'instagram', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '7 days'),
  ('intaudit_velion_google_connected', :'org_id', :'admin_user_id', 'conn_velion_google', 'velion.ingestion.integration.connected', 'google', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '9 days'),
  ('intaudit_velion_slack_connected', :'org_id', :'normal_user_id', 'conn_velion_slack', 'velion.ingestion.integration.connected', 'slack', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '8 days')
ON CONFLICT (id) DO UPDATE
SET metadata = EXCLUDED.metadata,
    created_at = EXCLUDED.created_at;

COMMIT;
SQL
}

seed_application_plane() {
  echo "Seeding application_plane..."
  run_psql application-postgres appuser application_plane <<'SQL'
BEGIN;

INSERT INTO conversation_inboxes (id, org_id, name, channel, created_at, updated_at)
VALUES
  ('inbox_velion_email', :'org_id', 'Support email', 'email', now() - interval '14 days', now()),
  ('inbox_velion_whatsapp', :'org_id', 'WhatsApp support', 'whatsapp', now() - interval '13 days', now()),
  ('inbox_velion_instagram', :'org_id', 'Instagram DMs', 'instagram', now() - interval '12 days', now())
ON CONFLICT (org_id, channel) DO UPDATE
SET name = EXCLUDED.name,
    updated_at = now();

INSERT INTO conversation_contacts (id, org_id, name, email, phone, external_ref, created_at, updated_at)
VALUES
  ('contact_velion_anne', :'org_id', 'Anne Larsen', 'anne.larsen@example.test', '+47 410 00 101', 'demo:anne', now() - interval '7 days', now()),
  ('contact_velion_marius', :'org_id', 'Marius Berg', 'marius.berg@example.test', '+47 410 00 102', 'demo:marius', now() - interval '6 days', now()),
  ('contact_velion_sara', :'org_id', 'Sara Nilsen', 'sara.nilsen@example.test', '+47 410 00 103', 'demo:sara', now() - interval '5 days', now()),
  ('contact_velion_emil', :'org_id', 'Emil Johansen', 'emil.johansen@example.test', '+47 410 00 104', 'demo:emil', now() - interval '4 days', now()),
  ('contact_velion_linnea', :'org_id', 'Linnea Solberg', 'linnea.solberg@example.test', '+47 410 00 105', 'demo:linnea', now() - interval '3 days', now()),
  ('contact_velion_ole', :'org_id', 'Ole Haug', 'ole.haug@example.test', '+47 410 00 106', 'demo:ole', now() - interval '2 days', now())
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    email = EXCLUDED.email,
    phone = EXCLUDED.phone,
    external_ref = EXCLUDED.external_ref,
    updated_at = now();

INSERT INTO conversations (
  id,
  org_id,
  inbox_id,
  contact_id,
  title,
  status,
  priority,
  assignee_user_id,
  assignee_name,
  channel,
  provider,
  provider_thread_id,
  last_message_preview,
  last_message_at,
  created_at,
  updated_at
)
VALUES
  ('conv_refund_delivery', :'org_id', (SELECT id FROM conversation_inboxes WHERE org_id = :'org_id' AND channel = 'email'), 'contact_velion_anne', 'Refund blocked after delivery issue', 'open', 'high', :'admin_user_id', :'admin_name', 'email', 'local_demo', 'thread_refund_delivery', 'Customer needs delivery evidence before refund can be released.', now() - interval '28 minutes', now() - interval '2 days', now()),
  ('conv_security_login', :'org_id', (SELECT id FROM conversation_inboxes WHERE org_id = :'org_id' AND channel = 'email'), 'contact_velion_marius', 'Suspicious login on account', 'open', 'urgent', :'admin_user_id', :'admin_name', 'email', 'local_demo', 'thread_security_login', 'Customer reports login from unknown device.', now() - interval '18 minutes', now() - interval '1 day', now()),
  ('conv_whatsapp_setup', :'org_id', (SELECT id FROM conversation_inboxes WHERE org_id = :'org_id' AND channel = 'whatsapp'), 'contact_velion_sara', 'WhatsApp setup waiting on customer', 'waiting_customer', 'normal', :'normal_user_id', :'normal_name', 'whatsapp', 'local_demo', 'thread_whatsapp_setup', 'Waiting for business verification screenshot.', now() - interval '50 minutes', now() - interval '22 hours', now()),
  ('conv_invoice_copy', :'org_id', (SELECT id FROM conversation_inboxes WHERE org_id = :'org_id' AND channel = 'email'), 'contact_velion_emil', 'Invoice copy requested by finance', 'waiting_team', 'normal', :'normal_user_id', :'normal_name', 'email', 'local_demo', 'thread_invoice_copy', 'Finance needs the paid invoice for May.', now() - interval '75 minutes', now() - interval '20 hours', now()),
  ('conv_product_copy', :'org_id', (SELECT id FROM conversation_inboxes WHERE org_id = :'org_id' AND channel = 'email'), 'contact_velion_linnea', 'Product page copy needs approval', 'open', 'normal', :'editor_user_id', :'editor_name', 'email', 'local_demo', 'thread_product_copy', 'Editor needs approval on updated product wording.', now() - interval '2 hours', now() - interval '18 hours', now()),
  ('conv_social_schedule', :'org_id', (SELECT id FROM conversation_inboxes WHERE org_id = :'org_id' AND channel = 'instagram'), 'contact_velion_ole', 'Schedule campaign follow-up from Instagram', 'open', 'low', :'editor_user_id', :'editor_name', 'instagram', 'local_demo', 'thread_social_schedule', 'Customer asked when the campaign follow-up goes live.', now() - interval '3 hours', now() - interval '16 hours', now())
ON CONFLICT (id) DO UPDATE
SET inbox_id = EXCLUDED.inbox_id,
    contact_id = EXCLUDED.contact_id,
    title = EXCLUDED.title,
    status = EXCLUDED.status,
    priority = EXCLUDED.priority,
    assignee_user_id = EXCLUDED.assignee_user_id,
    assignee_name = EXCLUDED.assignee_name,
    channel = EXCLUDED.channel,
    provider = EXCLUDED.provider,
    provider_thread_id = EXCLUDED.provider_thread_id,
    last_message_preview = EXCLUDED.last_message_preview,
    last_message_at = EXCLUDED.last_message_at,
    updated_at = now();

INSERT INTO conversation_messages (
  id,
  org_id,
  conversation_id,
  direction,
  sender_type,
  sender_name,
  sender_email,
  body_text,
  internal,
  provider,
  occurred_at,
  created_at
)
VALUES
  ('msg_refund_customer_1', :'org_id', 'conv_refund_delivery', 'inbound', 'customer', 'Anne Larsen', 'anne.larsen@example.test', 'The package arrived damaged and the refund is still blocked. Can someone check the handoff?', false, 'local_demo', now() - interval '2 days', now() - interval '2 days'),
  ('msg_refund_agent_1', :'org_id', 'conv_refund_delivery', 'outbound', 'agent', :'admin_name', :'admin_email', 'We are checking the delivery evidence and billing handoff now.', false, 'local_demo', now() - interval '28 minutes', now() - interval '28 minutes'),
  ('msg_security_customer_1', :'org_id', 'conv_security_login', 'inbound', 'customer', 'Marius Berg', 'marius.berg@example.test', 'I saw a login from a device I do not recognize.', false, 'local_demo', now() - interval '1 day', now() - interval '1 day'),
  ('msg_security_agent_1', :'org_id', 'conv_security_login', 'internal', 'system', 'Velion', '', 'Security review created and escalated for manual verification.', true, 'local_demo', now() - interval '18 minutes', now() - interval '18 minutes'),
  ('msg_whatsapp_customer_1', :'org_id', 'conv_whatsapp_setup', 'inbound', 'customer', 'Sara Nilsen', 'sara.nilsen@example.test', 'I need help connecting WhatsApp. The verification screen is confusing.', false, 'local_demo', now() - interval '22 hours', now() - interval '22 hours'),
  ('msg_whatsapp_agent_1', :'org_id', 'conv_whatsapp_setup', 'outbound', 'agent', :'normal_name', :'normal_email', 'Please send the verification screenshot and we will finish the setup.', false, 'local_demo', now() - interval '50 minutes', now() - interval '50 minutes'),
  ('msg_invoice_customer_1', :'org_id', 'conv_invoice_copy', 'inbound', 'customer', 'Emil Johansen', 'emil.johansen@example.test', 'Can you resend the paid invoice for May to our finance team?', false, 'local_demo', now() - interval '20 hours', now() - interval '20 hours'),
  ('msg_product_customer_1', :'org_id', 'conv_product_copy', 'inbound', 'customer', 'Linnea Solberg', 'linnea.solberg@example.test', 'Please review the copy before we publish the updated product page.', false, 'local_demo', now() - interval '18 hours', now() - interval '18 hours'),
  ('msg_social_customer_1', :'org_id', 'conv_social_schedule', 'inbound', 'customer', 'Ole Haug', 'ole.haug@example.test', 'When will the Instagram follow-up post go live?', false, 'local_demo', now() - interval '16 hours', now() - interval '16 hours')
ON CONFLICT (id) DO UPDATE
SET body_text = EXCLUDED.body_text,
    sender_name = EXCLUDED.sender_name,
    sender_email = EXCLUDED.sender_email,
    internal = EXCLUDED.internal,
    occurred_at = EXCLUDED.occurred_at;

INSERT INTO conversation_sla_policies (
  id,
  org_id,
  name,
  active,
  conditions,
  calendar_ref,
  first_response_minutes,
  next_response_minutes,
  resolution_minutes,
  created_at,
  updated_at
)
VALUES
  ('sla_standard_support', :'org_id', 'Standard support', true, jsonb_build_object('priority', jsonb_build_array('low', 'normal')), 'business-hours-oslo', 240, 480, 2880, now() - interval '14 days', now()),
  ('sla_urgent_escalation', :'org_id', 'Urgent escalation', true, jsonb_build_object('priority', jsonb_build_array('high', 'urgent')), 'business-hours-oslo', 60, 120, 720, now() - interval '14 days', now())
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    active = EXCLUDED.active,
    conditions = EXCLUDED.conditions,
    calendar_ref = EXCLUDED.calendar_ref,
    first_response_minutes = EXCLUDED.first_response_minutes,
    next_response_minutes = EXCLUDED.next_response_minutes,
    resolution_minutes = EXCLUDED.resolution_minutes,
    updated_at = now();

INSERT INTO conversation_tickets (
  id,
  org_id,
  conversation_id,
  ticket_key,
  status,
  priority,
  severity,
  category,
  intent,
  assignee_user_id,
  assignee_name,
  team_id,
  team_name,
  due_at,
  source,
  ai_confidence,
  ai_reason,
  created_by,
  waiting_since,
  last_customer_reply_at,
  first_response_at,
  resolved_at,
  sla_policy_id,
  escalation_at,
  labels,
  created_at,
  updated_at
)
VALUES
  ('ticket_refund_delivery', :'org_id', 'conv_refund_delivery', 'TCK-1001', 'waiting_team', 'high', 'major', 'billing', 'refund_handoff', :'admin_user_id', :'admin_name', 'billing', 'Billing', now() + interval '6 hours', 'ai', 0.91, 'Refund and delivery issue needs billing handoff.', :'admin_user_id', now() - interval '28 minutes', now() - interval '2 days', now() - interval '28 minutes', null, 'sla_urgent_escalation', null, ARRAY['refund','delivery','handoff'], now() - interval '2 days', now()),
  ('ticket_security_login', :'org_id', 'conv_security_login', 'TCK-1002', 'escalated', 'urgent', 'critical', 'security', 'account_takeover_risk', :'admin_user_id', :'admin_name', 'security', 'Security', now() + interval '2 hours', 'manual', 0.0, 'Manual security review requested.', :'admin_user_id', null, now() - interval '1 day', now() - interval '18 minutes', null, 'sla_urgent_escalation', now() - interval '18 minutes', ARRAY['security','manual-review'], now() - interval '1 day', now()),
  ('ticket_whatsapp_setup', :'org_id', 'conv_whatsapp_setup', 'TCK-1003', 'waiting_customer', 'normal', 'medium', 'integrations', 'whatsapp_setup', :'normal_user_id', :'normal_name', 'support', 'Support', now() + interval '1 day', 'ai', 0.82, 'Customer needs setup guidance and has pending verification evidence.', :'normal_user_id', now() - interval '50 minutes', now() - interval '22 hours', now() - interval '50 minutes', null, 'sla_standard_support', null, ARRAY['whatsapp','setup'], now() - interval '22 hours', now()),
  ('ticket_invoice_copy', :'org_id', 'conv_invoice_copy', 'TCK-1004', 'waiting_team', 'normal', 'low', 'billing', 'invoice_copy', :'normal_user_id', :'normal_name', 'finance', 'Finance', now() + interval '2 days', 'manual', 0.0, 'Finance needs paid invoice copy.', :'normal_user_id', now() - interval '75 minutes', now() - interval '20 hours', null, null, 'sla_standard_support', null, ARRAY['invoice','finance'], now() - interval '20 hours', now()),
  ('ticket_product_copy', :'org_id', 'conv_product_copy', 'TCK-1005', 'open', 'normal', 'medium', 'content', 'copy_review', :'editor_user_id', :'editor_name', 'content', 'Content', now() + interval '20 hours', 'ai', 0.78, 'Content review can become a social/content task.', :'editor_user_id', null, now() - interval '18 hours', null, null, 'sla_standard_support', null, ARRAY['content','approval'], now() - interval '18 hours', now()),
  ('ticket_social_schedule', :'org_id', 'conv_social_schedule', 'TCK-1006', 'suggested', 'low', 'low', 'social', 'campaign_schedule', :'editor_user_id', :'editor_name', 'content', 'Content', now() + interval '3 days', 'ai', 0.86, 'Customer asks about campaign schedule; suggested ticket for editor follow-up.', :'editor_user_id', null, now() - interval '16 hours', null, null, 'sla_standard_support', null, ARRAY['social','instagram','campaign'], now() - interval '16 hours', now())
ON CONFLICT (id) DO UPDATE
SET conversation_id = EXCLUDED.conversation_id,
    ticket_key = EXCLUDED.ticket_key,
    status = EXCLUDED.status,
    priority = EXCLUDED.priority,
    severity = EXCLUDED.severity,
    category = EXCLUDED.category,
    intent = EXCLUDED.intent,
    assignee_user_id = EXCLUDED.assignee_user_id,
    assignee_name = EXCLUDED.assignee_name,
    team_id = EXCLUDED.team_id,
    team_name = EXCLUDED.team_name,
    due_at = EXCLUDED.due_at,
    source = EXCLUDED.source,
    ai_confidence = EXCLUDED.ai_confidence,
    ai_reason = EXCLUDED.ai_reason,
    created_by = EXCLUDED.created_by,
    waiting_since = EXCLUDED.waiting_since,
    last_customer_reply_at = EXCLUDED.last_customer_reply_at,
    first_response_at = EXCLUDED.first_response_at,
    resolved_at = EXCLUDED.resolved_at,
    sla_policy_id = EXCLUDED.sla_policy_id,
    escalation_at = EXCLUDED.escalation_at,
    labels = EXCLUDED.labels,
    updated_at = now();

INSERT INTO conversation_ticket_views (
  id,
  org_id,
  name,
  scope,
  owner_user_id,
  team_id,
  visibility,
  filter,
  sort,
  group_by,
  sidebar_order,
  created_at,
  updated_at
)
VALUES
  ('view_my_open', :'org_id', 'My open tickets', 'user', :'admin_user_id', '', 'sidebar', jsonb_build_object('status', jsonb_build_array('open','waiting_team','waiting_customer','escalated')), jsonb_build_object('due_at', 'asc'), 'status', 1, now() - interval '14 days', now()),
  ('view_refund_handoffs', :'org_id', 'Refund handoffs', 'team', '', 'billing', 'sidebar', jsonb_build_object('label', 'refund'), jsonb_build_object('priority', 'desc'), 'priority', 2, now() - interval '14 days', now()),
  ('view_breached_sla', :'org_id', 'Breached SLA', 'org', '', '', 'sidebar', jsonb_build_object('sla_state', 'breached'), jsonb_build_object('due_at', 'asc'), 'team', 3, now() - interval '14 days', now()),
  ('view_editor_reviews', :'org_id', 'Editor review queue', 'team', '', 'content', 'sidebar', jsonb_build_object('team', 'content'), jsonb_build_object('updated_at', 'desc'), 'category', 4, now() - interval '14 days', now())
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    scope = EXCLUDED.scope,
    owner_user_id = EXCLUDED.owner_user_id,
    team_id = EXCLUDED.team_id,
    visibility = EXCLUDED.visibility,
    filter = EXCLUDED.filter,
    sort = EXCLUDED.sort,
    group_by = EXCLUDED.group_by,
    sidebar_order = EXCLUDED.sidebar_order,
    updated_at = now();

INSERT INTO conversation_ticket_macros (
  id,
  org_id,
  name,
  description,
  visibility,
  team_id,
  active,
  actions,
  conditions,
  created_at,
  updated_at
)
VALUES
  ('macro_refund_handoff', :'org_id', 'Refund billing handoff', 'Escalate to billing and tag refund evidence.', 'team', 'billing', true, jsonb_build_object('status', 'waiting_team', 'priority', 'high', 'team_id', 'billing', 'team_name', 'Billing', 'labels', jsonb_build_array('refund','handoff')), jsonb_build_object('category', 'billing'), now() - interval '14 days', now()),
  ('macro_security_review', :'org_id', 'Security manual review', 'Escalate a suspected account takeover.', 'org', '', true, jsonb_build_object('status', 'escalated', 'priority', 'urgent', 'severity', 'critical', 'team_id', 'security', 'team_name', 'Security', 'labels', jsonb_build_array('security','manual-review')), jsonb_build_object('category', 'security'), now() - interval '14 days', now()),
  ('macro_waiting_customer_update', :'org_id', 'Waiting on customer', 'Move to waiting customer and keep owner assigned.', 'team', 'support', true, jsonb_build_object('status', 'waiting_customer', 'labels', jsonb_build_array('waiting-customer')), jsonb_build_object(), now() - interval '14 days', now()),
  ('macro_editor_followup', :'org_id', 'Editor follow-up', 'Route a content/social ticket to the editor queue.', 'team', 'content', true, jsonb_build_object('status', 'open', 'priority', 'normal', 'team_id', 'content', 'team_name', 'Content', 'assignee_user_id', :'editor_user_id', 'assignee_name', :'editor_name', 'labels', jsonb_build_array('content','editor-review')), jsonb_build_object(), now() - interval '14 days', now())
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    description = EXCLUDED.description,
    visibility = EXCLUDED.visibility,
    team_id = EXCLUDED.team_id,
    active = EXCLUDED.active,
    actions = EXCLUDED.actions,
    conditions = EXCLUDED.conditions,
    updated_at = now();

INSERT INTO conversation_ticket_automation_rules (
  id,
  org_id,
  name,
  event_name,
  active,
  conditions,
  actions,
  created_at,
  updated_at
)
VALUES
  ('rule_sla_risk_escalation', :'org_id', 'Escalate SLA risk', 'ticket.sla_risk', true, jsonb_build_object('minutes_to_due', '<=60'), jsonb_build_object('status', 'escalated', 'notify', jsonb_build_array('admin','team_lead')), now() - interval '14 days', now()),
  ('rule_security_manual_review', :'org_id', 'Never auto-close security cases', 'ticket.status_change', true, jsonb_build_object('category', 'security'), jsonb_build_object('require_manual_review', true), now() - interval '14 days', now()),
  ('rule_route_refunds', :'org_id', 'Route refund cases to Billing', 'message.received', true, jsonb_build_object('intent', 'refund'), jsonb_build_object('team_id', 'billing', 'macro_id', 'macro_refund_handoff'), now() - interval '14 days', now())
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    event_name = EXCLUDED.event_name,
    active = EXCLUDED.active,
    conditions = EXCLUDED.conditions,
    actions = EXCLUDED.actions,
    updated_at = now();

INSERT INTO conversation_ticket_checklists (id, org_id, ticket_id, name, template_id, created_by_user_id, created_at, updated_at)
VALUES
  ('checklist_refund_delivery', :'org_id', 'ticket_refund_delivery', 'Refund handoff checklist', 'template_refund', :'admin_user_id', now() - interval '2 days', now()),
  ('checklist_security_login', :'org_id', 'ticket_security_login', 'Security review checklist', 'template_security', :'admin_user_id', now() - interval '1 day', now()),
  ('checklist_whatsapp_setup', :'org_id', 'ticket_whatsapp_setup', 'WhatsApp setup checklist', 'template_integrations', :'normal_user_id', now() - interval '22 hours', now()),
  ('checklist_product_copy', :'org_id', 'ticket_product_copy', 'Content approval checklist', 'template_content', :'editor_user_id', now() - interval '18 hours', now())
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    template_id = EXCLUDED.template_id,
    created_by_user_id = EXCLUDED.created_by_user_id,
    updated_at = now();

INSERT INTO conversation_ticket_checklist_items (id, org_id, checklist_id, label, completed, position, created_at, updated_at)
VALUES
  ('cli_refund_1', :'org_id', 'checklist_refund_delivery', 'Collect delivery evidence', true, 1, now() - interval '2 days', now()),
  ('cli_refund_2', :'org_id', 'checklist_refund_delivery', 'Confirm refund amount with billing', false, 2, now() - interval '2 days', now()),
  ('cli_refund_3', :'org_id', 'checklist_refund_delivery', 'Send customer resolution summary', false, 3, now() - interval '2 days', now()),
  ('cli_security_1', :'org_id', 'checklist_security_login', 'Validate recent device fingerprint', true, 1, now() - interval '1 day', now()),
  ('cli_security_2', :'org_id', 'checklist_security_login', 'Force password reset if needed', false, 2, now() - interval '1 day', now()),
  ('cli_security_3', :'org_id', 'checklist_security_login', 'Document security decision', false, 3, now() - interval '1 day', now()),
  ('cli_whatsapp_1', :'org_id', 'checklist_whatsapp_setup', 'Request verification screenshot', true, 1, now() - interval '22 hours', now()),
  ('cli_whatsapp_2', :'org_id', 'checklist_whatsapp_setup', 'Confirm business display name', false, 2, now() - interval '22 hours', now()),
  ('cli_product_1', :'org_id', 'checklist_product_copy', 'Review product copy tone', false, 1, now() - interval '18 hours', now()),
  ('cli_product_2', :'org_id', 'checklist_product_copy', 'Prepare approval note', false, 2, now() - interval '18 hours', now())
ON CONFLICT (id) DO UPDATE
SET label = EXCLUDED.label,
    completed = EXCLUDED.completed,
    position = EXCLUDED.position,
    updated_at = now();

INSERT INTO conversation_linked_resources (
  id,
  org_id,
  ticket_id,
  conversation_id,
  link_type,
  resource_kind,
  resource_id,
  resource_url,
  label,
  metadata,
  created_by_user_id,
  created_at
)
VALUES
  ('link_refund_runbook', :'org_id', 'ticket_refund_delivery', 'conv_refund_delivery', 'normal', 'document', 'doc_velion_admin_runbook', '', 'Refund escalation runbook', jsonb_build_object('seed', 'velion-local-demo'), :'admin_user_id', now() - interval '2 days'),
  ('link_security_runbook', :'org_id', 'ticket_security_login', 'conv_security_login', 'normal', 'document', 'doc_velion_admin_runbook', '', 'Security review process', jsonb_build_object('seed', 'velion-local-demo'), :'admin_user_id', now() - interval '1 day'),
  ('link_whatsapp_guide', :'org_id', 'ticket_whatsapp_setup', 'conv_whatsapp_setup', 'normal', 'document', 'doc_velion_normal_support_guide', '', 'WhatsApp setup guide', jsonb_build_object('seed', 'velion-local-demo'), :'normal_user_id', now() - interval '22 hours'),
  ('link_social_plan', :'org_id', 'ticket_social_schedule', 'conv_social_schedule', 'normal', 'social_post', 'post_velion_campaign_followup', '', 'Campaign follow-up draft', jsonb_build_object('seed', 'velion-local-demo'), :'editor_user_id', now() - interval '16 hours')
ON CONFLICT (id) DO UPDATE
SET label = EXCLUDED.label,
    metadata = EXCLUDED.metadata,
    created_by_user_id = EXCLUDED.created_by_user_id;

DELETE FROM social_accounts
WHERE id IN ('social_account_instagram_velion', 'social_account_linkedin_velion');

INSERT INTO social_accounts (
  id,
  org_id,
  provider_key,
  connection_id,
  display_name,
  handle,
  status,
  capabilities,
  token_state,
  token_expires_at,
  metadata,
  created_at,
  updated_at
)
VALUES
  ('socacct_' || replace(replace(lower(:'org_id' || '_linkedin_conn_velion_linkedin'), ':', '_'), '/', '_'), :'org_id', 'linkedin', 'conn_velion_linkedin', 'Velion LinkedIn', 'Velion AS', 'connected', jsonb_build_array('publish','media_upload','insights'), 'available', now() + interval '30 days', jsonb_build_object('seed', 'velion-local-demo', 'provider_account_id', 'urn:li:organization:997711'), now() - interval '7 days', now()),
  ('socacct_' || replace(replace(lower(:'org_id' || '_x_conn_velion_x'), ':', '_'), '/', '_'), :'org_id', 'x', 'conn_velion_x', 'Velion X', '@veliondemo', 'connected', jsonb_build_array('publish','media_upload','insights'), 'available', now() + interval '30 days', jsonb_build_object('seed', 'velion-local-demo', 'provider_account_id', 'x_account_velion_demo'), now() - interval '7 days', now()),
  ('socacct_' || replace(replace(lower(:'org_id' || '_instagram_conn_velion_instagram'), ':', '_'), '/', '_'), :'org_id', 'instagram', 'conn_velion_instagram', 'Velion Instagram', '@veliondemo', 'connected', jsonb_build_array('publish','comments','insights','media_upload'), 'available', now() + interval '30 days', jsonb_build_object('seed', 'velion-local-demo', 'provider_account_id', 'ig_business_velion_demo'), now() - interval '7 days', now()),
  ('socacct_' || replace(replace(lower(:'org_id' || '_facebook_conn_velion_facebook'), ':', '_'), '/', '_'), :'org_id', 'facebook', 'conn_velion_facebook', 'Velion Facebook', 'Velion AS', 'connected', jsonb_build_array('publish','comments','inbox','insights','media_upload'), 'available', now() + interval '30 days', jsonb_build_object('seed', 'velion-local-demo', 'provider_account_id', 'fb_page_velion_demo'), now() - interval '7 days', now()),
  ('socacct_' || replace(replace(lower(:'org_id' || '_tiktok_conn_velion_tiktok'), ':', '_'), '/', '_'), :'org_id', 'tiktok', 'conn_velion_tiktok', 'Velion TikTok', '@veliondemo', 'connected', jsonb_build_array('publish','media_upload','insights'), 'available', now() + interval '30 days', jsonb_build_object('seed', 'velion-local-demo', 'provider_account_id', 'tiktok_business_velion_demo'), now() - interval '6 days', now()),
  ('socacct_' || replace(replace(lower(:'org_id' || '_snapchat_conn_velion_snapchat'), ':', '_'), '/', '_'), :'org_id', 'snapchat', 'conn_velion_snapchat', 'Velion Snapchat', 'veliondemo', 'connected', jsonb_build_array('ads_manage','insights'), 'available', now() + interval '30 days', jsonb_build_object('seed', 'velion-local-demo', 'provider_account_id', 'snap_ad_account_velion_demo'), now() - interval '6 days', now())
ON CONFLICT (id) DO UPDATE
SET connection_id = EXCLUDED.connection_id,
    display_name = EXCLUDED.display_name,
    handle = EXCLUDED.handle,
    status = EXCLUDED.status,
    capabilities = EXCLUDED.capabilities,
    token_state = EXCLUDED.token_state,
    token_expires_at = EXCLUDED.token_expires_at,
    metadata = EXCLUDED.metadata,
    updated_at = now();

INSERT INTO social_campaigns (
  id,
  org_id,
  name,
  brief,
  goal,
  status,
  platforms,
  starts_at,
  ends_at,
  source,
  metadata,
  owner_user_id,
  created_at,
  updated_at
)
VALUES
  ('campaign_velion_summer_support', :'org_id', 'Summer support readiness', 'Explain response times, support coverage, and self-service before summer.', 'Reduce repeated support questions during summer staffing.', 'active', jsonb_build_array('instagram','facebook','linkedin'), date_trunc('day', now()), date_trunc('day', now()) + interval '21 days', jsonb_build_object('kind', 'ticket_insight', 'label', 'Support queue analysis', 'href', '/tickets'), jsonb_build_object('seed', 'velion-local-demo', 'audience', 'customers'), :'editor_user_id', now() - interval '3 days', now()),
  ('campaign_velion_product_launch', :'org_id', 'Advanced workspace launch', 'Show how Velion Advanced combines integrations, ticketing, approvals, and reporting.', 'Create interest in the Advanced plan for operational teams.', 'active', jsonb_build_array('linkedin','x','snapchat'), date_trunc('day', now()) - interval '2 days', date_trunc('day', now()) + interval '28 days', jsonb_build_object('kind', 'billing_plan', 'label', 'Velion Advanced', 'href', '/settings/billing'), jsonb_build_object('seed', 'velion-local-demo', 'funnel', 'trial_to_paid'), :'admin_user_id', now() - interval '4 days', now()),
  ('campaign_velion_customer_story', :'org_id', 'Support operations customer story', 'Turn a resolved support workflow into reusable social proof and evergreen content.', 'Increase trust with teams evaluating Velion for support operations.', 'draft', jsonb_build_array('linkedin','facebook','instagram','tiktok'), date_trunc('day', now()) + interval '3 days', date_trunc('day', now()) + interval '45 days', jsonb_build_object('kind', 'case_study', 'label', 'Refund workflow resolution', 'href', '/tickets'), jsonb_build_object('seed', 'velion-local-demo', 'approval_lane', 'content'), :'editor_user_id', now() - interval '1 day', now())
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    brief = EXCLUDED.brief,
    goal = EXCLUDED.goal,
    status = EXCLUDED.status,
    platforms = EXCLUDED.platforms,
    starts_at = EXCLUDED.starts_at,
    ends_at = EXCLUDED.ends_at,
    source = EXCLUDED.source,
    metadata = EXCLUDED.metadata,
    owner_user_id = EXCLUDED.owner_user_id,
    updated_at = now();

INSERT INTO social_posts (
  id,
  org_id,
  title,
  body,
  status,
  platforms,
  media,
  source,
  previews,
  ai_context,
  approval_required,
  approval_state,
  scheduled_at,
  created_by_user_id,
  updated_by_user_id,
  created_at,
  updated_at
)
VALUES
  (
    'post_velion_campaign_followup',
    :'org_id',
    'Support hours summer reminder',
    'Summer support is ready. Customers can use self-service first, then reach us in the priority queue during staffed hours.',
    'scheduled',
    jsonb_build_array('instagram','facebook'),
    jsonb_build_array(jsonb_build_object('id', 'media_velion_support_hours', 'type', 'image', 'storage_ref', 'seed/social/support-hours.png', 'alt_text', 'Velion support hours summary card')),
    jsonb_build_object('kind', 'campaign', 'label', 'Summer support readiness', 'href', '/social/campaigns/campaign_velion_summer_support'),
    jsonb_build_array(
      jsonb_build_object('platform', 'instagram', 'mode', 'feed', 'content', 'Summer support is ready. Use self-service first, then reach us in priority support during staffed hours.', 'character_limit', 2200, 'warnings', jsonb_build_array(), 'media_required', true),
      jsonb_build_object('platform', 'facebook', 'mode', 'page_post', 'content', 'Summer support is ready. Customers can use self-service first, then reach us in the priority queue during staffed hours.', 'character_limit', 63206, 'warnings', jsonb_build_array(), 'media_required', false)
    ),
    jsonb_build_object('tone', 'clear and calm', 'seed', 'velion-local-demo', 'sourceTicketCount', 6),
    true,
    'approved',
    now() + interval '1 day',
    :'editor_user_id',
    :'editor_user_id',
    now() - interval '16 hours',
    now()
  ),
  (
    'post_velion_linkedin_update',
    :'org_id',
    'LinkedIn update: advanced support workflows',
    'Velion AS now has connected workflows for ticket triage, social approvals, integrations, and reporting in the Advanced workspace.',
    'draft',
    jsonb_build_array('linkedin'),
    jsonb_build_array(),
    jsonb_build_object('kind', 'campaign', 'label', 'Advanced workspace launch', 'href', '/social/campaigns/campaign_velion_product_launch'),
    jsonb_build_array(jsonb_build_object('platform', 'linkedin', 'mode', 'organization_post', 'content', 'Velion AS now has connected workflows for ticket triage, social approvals, integrations, and reporting in the Advanced workspace.', 'character_limit', 3000, 'warnings', jsonb_build_array(), 'media_required', false)),
    jsonb_build_object('tone', 'professional', 'seed', 'velion-local-demo', 'persona', 'operations_lead'),
    true,
    'pending',
    null,
    :'editor_user_id',
    :'editor_user_id',
    now() - interval '12 hours',
    now()
  ),
  (
    'post_velion_x_thread_support_metrics',
    :'org_id',
    'X thread: support metrics snapshot',
    'This week in Velion support: faster triage, fewer repeated questions, and clearer handoffs between support, billing, and content.',
    'published',
    jsonb_build_array('x'),
    jsonb_build_array(),
    jsonb_build_object('kind', 'report', 'label', 'Support operations dashboard', 'href', '/reports'),
    jsonb_build_array(jsonb_build_object('platform', 'x', 'mode', 'thread', 'content', 'This week in Velion support: faster triage, fewer repeated questions, and clearer handoffs between support, billing, and content.', 'character_limit', 280, 'warnings', jsonb_build_array(), 'media_required', false)),
    jsonb_build_object('tone', 'concise', 'seed', 'velion-local-demo', 'metricWindow', '7d'),
    false,
    'not_required',
    now() - interval '6 hours',
    :'admin_user_id',
    :'editor_user_id',
    now() - interval '9 hours',
    now() - interval '6 hours'
  ),
  (
    'post_velion_facebook_inbox_coverage',
    :'org_id',
    'Facebook reminder: inbox coverage',
    'Our social inbox is monitored alongside support tickets, so customer questions can move from comment to case without losing context.',
    'scheduled',
    jsonb_build_array('facebook'),
    jsonb_build_array(jsonb_build_object('id', 'media_velion_inbox_coverage', 'type', 'image', 'storage_ref', 'seed/social/inbox-coverage.png', 'alt_text', 'Connected social inbox workflow')),
    jsonb_build_object('kind', 'integration', 'label', 'Facebook page inbox', 'href', '/settings/integrations'),
    jsonb_build_array(jsonb_build_object('platform', 'facebook', 'mode', 'page_post', 'content', 'Our social inbox is monitored alongside support tickets, so customer questions can move from comment to case without losing context.', 'character_limit', 63206, 'warnings', jsonb_build_array(), 'media_required', false)),
    jsonb_build_object('tone', 'helpful', 'seed', 'velion-local-demo', 'source', 'facebook_inbox'),
    true,
    'pending',
    now() + interval '2 days',
    :'editor_user_id',
    :'editor_user_id',
    now() - interval '8 hours',
    now()
  ),
  (
    'post_velion_tiktok_support_tip',
    :'org_id',
    'TikTok short: where to find invoice help',
    'A quick support tip showing where customers can find invoice copies and how to contact finance when ownership is verified.',
    'blocked',
    jsonb_build_array('tiktok'),
    jsonb_build_array(),
    jsonb_build_object('kind', 'ticket_pattern', 'label', 'Invoice copy requests', 'href', '/tickets'),
    jsonb_build_array(jsonb_build_object('platform', 'tiktok', 'mode', 'video', 'content', 'A quick support tip showing where customers can find invoice copies and how to contact finance after verification.', 'character_limit', 2200, 'warnings', jsonb_build_array('TikTok requires a video asset before publishing.'), 'media_required', true)),
    jsonb_build_object('tone', 'short_video', 'seed', 'velion-local-demo', 'blockedReason', 'missing_video'),
    true,
    'rejected',
    null,
    :'editor_user_id',
    :'admin_user_id',
    now() - interval '6 hours',
    now()
  ),
  (
    'post_velion_snapchat_offer',
    :'org_id',
    'Snapchat awareness concept',
    'Short awareness concept for Velion Advanced teams that want connected support, social, and reporting data in one workspace.',
    'draft',
    jsonb_build_array('snapchat'),
    jsonb_build_array(jsonb_build_object('id', 'media_velion_snap_storyboard', 'type', 'image', 'storage_ref', 'seed/social/snap-storyboard.png', 'alt_text', 'Snapchat storyboard concept')),
    jsonb_build_object('kind', 'campaign', 'label', 'Advanced workspace launch', 'href', '/social/campaigns/campaign_velion_product_launch'),
    jsonb_build_array(jsonb_build_object('platform', 'snapchat', 'mode', 'ad_concept', 'content', 'Connected support, social, and reporting data for modern operations teams.', 'character_limit', 80, 'warnings', jsonb_build_array('Snapchat publishing uses ads permissions in this demo.'), 'media_required', true)),
    jsonb_build_object('tone', 'awareness', 'seed', 'velion-local-demo'),
    false,
    'not_required',
    null,
    :'editor_user_id',
    :'editor_user_id',
    now() - interval '5 hours',
    now()
  ),
  (
    'post_velion_case_study_evergreen',
    :'org_id',
    'Evergreen customer story: cleaner handoffs',
    'A resolved support workflow became a reusable playbook: every handoff kept the customer, evidence, status, and approval trail together.',
    'published',
    jsonb_build_array('linkedin','facebook'),
    jsonb_build_array(jsonb_build_object('id', 'media_velion_case_study', 'type', 'image', 'storage_ref', 'seed/social/case-study.png', 'alt_text', 'Support handoff playbook summary')),
    jsonb_build_object('kind', 'case_study', 'label', 'Support operations customer story', 'href', '/social/campaigns/campaign_velion_customer_story'),
    jsonb_build_array(
      jsonb_build_object('platform', 'linkedin', 'mode', 'organization_post', 'content', 'A resolved support workflow became a reusable playbook: customer, evidence, status, and approval trail stayed together.', 'character_limit', 3000, 'warnings', jsonb_build_array(), 'media_required', false),
      jsonb_build_object('platform', 'facebook', 'mode', 'page_post', 'content', 'A resolved support workflow became a reusable playbook: every handoff kept customer context and approval history together.', 'character_limit', 63206, 'warnings', jsonb_build_array(), 'media_required', false)
    ),
    jsonb_build_object('tone', 'evergreen', 'seed', 'velion-local-demo', 'reuseScore', 0.86),
    true,
    'approved',
    now() - interval '2 days',
    :'editor_user_id',
    :'admin_user_id',
    now() - interval '3 days',
    now() - interval '2 days'
  )
ON CONFLICT (id) DO UPDATE
SET title = EXCLUDED.title,
    body = EXCLUDED.body,
    status = EXCLUDED.status,
    platforms = EXCLUDED.platforms,
    media = EXCLUDED.media,
    source = EXCLUDED.source,
    previews = EXCLUDED.previews,
    ai_context = EXCLUDED.ai_context,
    approval_required = EXCLUDED.approval_required,
    approval_state = EXCLUDED.approval_state,
    scheduled_at = EXCLUDED.scheduled_at,
    created_by_user_id = EXCLUDED.created_by_user_id,
    updated_by_user_id = EXCLUDED.updated_by_user_id,
    updated_at = now();

INSERT INTO social_approvals (
  id,
  org_id,
  post_id,
  campaign_id,
  state,
  requested_by_user_id,
  requested_of_user_id,
  decided_by_user_id,
  decision_reason,
  due_at,
  decided_at,
  metadata,
  created_at,
  updated_at
)
VALUES
  ('approval_velion_campaign_followup', :'org_id', 'post_velion_campaign_followup', 'campaign_velion_summer_support', 'approved', :'editor_user_id', :'admin_user_id', :'admin_user_id', 'Approved for local demo schedule.', now() + interval '12 hours', now() - interval '2 hours', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '14 hours', now()),
  ('approval_velion_linkedin_update', :'org_id', 'post_velion_linkedin_update', 'campaign_velion_product_launch', 'pending', :'editor_user_id', :'admin_user_id', '', '', now() + interval '1 day', null, jsonb_build_object('seed', 'velion-local-demo'), now() - interval '12 hours', now()),
  ('approval_velion_facebook_inbox', :'org_id', 'post_velion_facebook_inbox_coverage', 'campaign_velion_summer_support', 'pending', :'editor_user_id', :'admin_user_id', '', '', now() + interval '18 hours', null, jsonb_build_object('seed', 'velion-local-demo'), now() - interval '8 hours', now()),
  ('approval_velion_tiktok_support_tip', :'org_id', 'post_velion_tiktok_support_tip', 'campaign_velion_customer_story', 'rejected', :'editor_user_id', :'admin_user_id', :'admin_user_id', 'Needs a real vertical video asset before publishing.', now() + interval '2 days', now() - interval '4 hours', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '6 hours', now()),
  ('approval_velion_case_study_evergreen', :'org_id', 'post_velion_case_study_evergreen', 'campaign_velion_customer_story', 'approved', :'editor_user_id', :'admin_user_id', :'admin_user_id', 'Approved as evergreen support operations content.', now() - interval '2 days', now() - interval '2 days', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '3 days', now())
ON CONFLICT (id) DO UPDATE
SET campaign_id = EXCLUDED.campaign_id,
    state = EXCLUDED.state,
    requested_by_user_id = EXCLUDED.requested_by_user_id,
    requested_of_user_id = EXCLUDED.requested_of_user_id,
    decided_by_user_id = EXCLUDED.decided_by_user_id,
    decision_reason = EXCLUDED.decision_reason,
    due_at = EXCLUDED.due_at,
    decided_at = EXCLUDED.decided_at,
    metadata = EXCLUDED.metadata,
    updated_at = now();

INSERT INTO social_publish_jobs (
  id,
  org_id,
  post_id,
  status,
  idempotency_key,
  requested_by_user_id,
  scheduled_for,
  locked_at,
  locked_by,
  attempts,
  last_error,
  created_at,
  updated_at
)
VALUES
  ('pubjob_velion_campaign_followup', :'org_id', 'post_velion_campaign_followup', 'queued', 'velion-demo-post-campaign-followup', :'editor_user_id', now() + interval '1 day', null, '', 0, '', now() - interval '2 hours', now()),
  ('pubjob_velion_x_metrics', :'org_id', 'post_velion_x_thread_support_metrics', 'completed', 'velion-demo-post-x-metrics', :'admin_user_id', now() - interval '6 hours', now() - interval '6 hours', 'social-worker-local', 1, '', now() - interval '7 hours', now() - interval '6 hours'),
  ('pubjob_velion_case_study', :'org_id', 'post_velion_case_study_evergreen', 'completed', 'velion-demo-post-case-study', :'editor_user_id', now() - interval '2 days', now() - interval '2 days', 'social-worker-local', 2, '', now() - interval '2 days', now() - interval '2 days'),
  ('pubjob_velion_tiktok_tip', :'org_id', 'post_velion_tiktok_support_tip', 'blocked', 'velion-demo-post-tiktok-tip', :'editor_user_id', now() + interval '3 days', null, '', 0, 'Missing vertical video asset.', now() - interval '4 hours', now())
ON CONFLICT (id) DO UPDATE
SET status = EXCLUDED.status,
    requested_by_user_id = EXCLUDED.requested_by_user_id,
    scheduled_for = EXCLUDED.scheduled_for,
    locked_at = EXCLUDED.locked_at,
    locked_by = EXCLUDED.locked_by,
    attempts = EXCLUDED.attempts,
    last_error = EXCLUDED.last_error,
    updated_at = now();

INSERT INTO social_publish_attempts (
  id,
  org_id,
  job_id,
  post_id,
  provider_key,
  status,
  mode,
  endpoint,
  external_id,
  message,
  warnings,
  response,
  attempted_at,
  created_at
)
VALUES
  ('pubattempt_velion_x_metrics', :'org_id', 'pubjob_velion_x_metrics', 'post_velion_x_thread_support_metrics', 'x', 'succeeded', 'api', '/2/tweets', 'x-demo-1781710', 'Published local demo X thread.', jsonb_build_array(), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '6 hours', now() - interval '6 hours'),
  ('pubattempt_velion_case_study_linkedin', :'org_id', 'pubjob_velion_case_study', 'post_velion_case_study_evergreen', 'linkedin', 'succeeded', 'api', '/v2/ugcPosts', 'linkedin-demo-997711', 'Published local demo LinkedIn post.', jsonb_build_array(), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 days', now() - interval '2 days'),
  ('pubattempt_velion_case_study_facebook', :'org_id', 'pubjob_velion_case_study', 'post_velion_case_study_evergreen', 'facebook', 'succeeded', 'api', '/me/feed', 'facebook-demo-112233', 'Published local demo Facebook page post.', jsonb_build_array(), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 days', now() - interval '2 days'),
  ('pubattempt_velion_tiktok_tip_blocked', :'org_id', 'pubjob_velion_tiktok_tip', 'post_velion_tiktok_support_tip', 'tiktok', 'blocked', 'api', '/v2/post/publish', '', 'Blocked before publish because media is missing.', jsonb_build_array('TikTok requires a video asset.'), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '4 hours', now() - interval '4 hours')
ON CONFLICT (id) DO UPDATE
SET status = EXCLUDED.status,
    mode = EXCLUDED.mode,
    endpoint = EXCLUDED.endpoint,
    external_id = EXCLUDED.external_id,
    message = EXCLUDED.message,
    warnings = EXCLUDED.warnings,
    response = EXCLUDED.response,
    attempted_at = EXCLUDED.attempted_at;

INSERT INTO social_audit_events (id, org_id, post_id, actor_user_id, action, payload, created_at)
VALUES
  ('socaudit_velion_accounts_seeded', :'org_id', '', :'admin_user_id', 'social.accounts.seeded', jsonb_build_object('seed', 'velion-local-demo', 'account_count', 6), now() - interval '7 days'),
  ('socaudit_velion_campaign_followup_approved', :'org_id', 'post_velion_campaign_followup', :'admin_user_id', 'social.approval.approved', jsonb_build_object('seed', 'velion-local-demo', 'campaign_id', 'campaign_velion_summer_support'), now() - interval '2 hours'),
  ('socaudit_velion_x_published', :'org_id', 'post_velion_x_thread_support_metrics', :'admin_user_id', 'social.post.published', jsonb_build_object('seed', 'velion-local-demo', 'provider', 'x'), now() - interval '6 hours'),
  ('socaudit_velion_tiktok_blocked', :'org_id', 'post_velion_tiktok_support_tip', :'admin_user_id', 'social.post.blocked', jsonb_build_object('seed', 'velion-local-demo', 'reason', 'missing_video'), now() - interval '4 hours')
ON CONFLICT (id) DO UPDATE
SET actor_user_id = EXCLUDED.actor_user_id,
    action = EXCLUDED.action,
    payload = EXCLUDED.payload,
    created_at = EXCLUDED.created_at;

COMMIT;
SQL
}

seed_data_plane() {
  echo "Seeding dataplane..."
  run_psql dpv2-postgres dataplane dataplane <<'SQL'
BEGIN;

INSERT INTO org_quotas (
  org_id,
  plan_tier,
  documents_limit,
  api_calls_per_month,
  storage_gb,
  concurrent_users,
  custom_models,
  created_at,
  updated_at
)
VALUES (
  :'org_id',
  'advanced',
  20000,
  100000,
  100.0,
  25,
  true,
  now() - interval '14 days',
  now()
)
ON CONFLICT (org_id) DO UPDATE
SET plan_tier = EXCLUDED.plan_tier,
    documents_limit = EXCLUDED.documents_limit,
    api_calls_per_month = EXCLUDED.api_calls_per_month,
    storage_gb = EXCLUDED.storage_gb,
    concurrent_users = EXCLUDED.concurrent_users,
    custom_models = EXCLUDED.custom_models,
    updated_at = now();

INSERT INTO source_objects (
  source_object_id,
  org_id,
  connector,
  source,
  external_id,
  path,
  name,
  mime_type,
  size_bytes,
  content_hash,
  acl_tags,
  metadata,
  modified_at,
  discovered_at,
  updated_at
)
VALUES
  ('src_velion_admin_runbook', :'org_id', 'local_seed', 'drive', 'seed/admin-runbook', '/Velion/Runbooks/refund-security.md', 'Refund and security runbook', 'text/markdown', 4096, 'sha1-admin-runbook-demo', ARRAY['org:' || :'org_id', 'role:admin'], jsonb_build_object('seed', 'velion-local-demo'), now() - interval '3 days', now() - interval '3 days', now()),
  ('src_velion_support_guide', :'org_id', 'local_seed', 'drive', 'seed/support-guide', '/Velion/Support/whatsapp-invoice.md', 'WhatsApp and invoice support guide', 'text/markdown', 3584, 'sha1-support-guide-demo', ARRAY['org:' || :'org_id', 'role:member'], jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 days', now() - interval '2 days', now()),
  ('src_velion_brand_voice', :'org_id', 'local_seed', 'drive', 'seed/brand-voice', '/Velion/Content/brand-voice.md', 'Brand voice and campaign notes', 'text/markdown', 5120, 'sha1-brand-voice-demo', ARRAY['org:' || :'org_id', 'role:editor'], jsonb_build_object('seed', 'velion-local-demo'), now() - interval '1 day', now() - interval '1 day', now())
ON CONFLICT (source_object_id) DO UPDATE
SET path = EXCLUDED.path,
    name = EXCLUDED.name,
    mime_type = EXCLUDED.mime_type,
    size_bytes = EXCLUDED.size_bytes,
    content_hash = EXCLUDED.content_hash,
    acl_tags = EXCLUDED.acl_tags,
    metadata = EXCLUDED.metadata,
    updated_at = now();

INSERT INTO documents (
  document_id,
  org_id,
  source,
  type,
  title,
  content,
  status,
  metadata,
  zdr_classification,
  zdr_reason,
  extraction_trace,
  created_by,
  idempotency_key,
  created_at,
  updated_at
)
VALUES
  ('doc_velion_admin_runbook', :'org_id', 'local_seed', 'runbook', 'Refund and security escalation runbook', 'Refund tickets with delivery evidence should be routed to Billing. Security tickets must be escalated to manual review, device checks, and customer verification before closure.', 'indexed', jsonb_build_object('source_object_id', 'src_velion_admin_runbook', 'owner_user_id', :'admin_user_id', 'seed', 'velion-local-demo'), 'internal', 'Demo runbook for local testing.', jsonb_build_object('parser', 'seed', 'chunks', 2), :'admin_user_id', 'velion-demo-admin-runbook', now() - interval '3 days', now()),
  ('doc_velion_normal_support_guide', :'org_id', 'local_seed', 'guide', 'WhatsApp setup and invoice support guide', 'For WhatsApp setup, request the business verification screenshot and confirm the display name. For invoice requests, verify account ownership and send the paid invoice copy to finance.', 'indexed', jsonb_build_object('source_object_id', 'src_velion_support_guide', 'owner_user_id', :'normal_user_id', 'seed', 'velion-local-demo'), 'internal', 'Demo support guide for local testing.', jsonb_build_object('parser', 'seed', 'chunks', 2), :'normal_user_id', 'velion-demo-support-guide', now() - interval '2 days', now()),
  ('doc_velion_editor_brand_voice', :'org_id', 'local_seed', 'brand', 'Brand voice and campaign approvals', 'Velion brand voice is clear, calm, and practical. Social posts require approval from an admin before publishing when they reference support availability or customer workflows.', 'indexed', jsonb_build_object('source_object_id', 'src_velion_brand_voice', 'owner_user_id', :'editor_user_id', 'seed', 'velion-local-demo'), 'internal', 'Demo content guide for local testing.', jsonb_build_object('parser', 'seed', 'chunks', 2), :'editor_user_id', 'velion-demo-brand-voice', now() - interval '1 day', now())
ON CONFLICT (document_id) DO UPDATE
SET source = EXCLUDED.source,
    type = EXCLUDED.type,
    title = EXCLUDED.title,
    content = EXCLUDED.content,
    status = EXCLUDED.status,
    metadata = EXCLUDED.metadata,
    zdr_classification = EXCLUDED.zdr_classification,
    zdr_reason = EXCLUDED.zdr_reason,
    extraction_trace = EXCLUDED.extraction_trace,
    created_by = EXCLUDED.created_by,
    idempotency_key = EXCLUDED.idempotency_key,
    deleted_at = null,
    updated_at = now();

INSERT INTO knowledge_units (
  knowledge_id,
  document_id,
  org_id,
  chunk_index,
  text,
  embedding_status,
  content_hash,
  chunk_version,
  embedding_model,
  metadata,
  created_at,
  updated_at
)
VALUES
  ('ku_velion_admin_runbook_1', 'doc_velion_admin_runbook', :'org_id', 1, 'Refund tickets with delivery evidence should be routed to Billing with customer-visible status waiting_team.', 'indexed', 'ku-admin-runbook-1', '1', 'text-embedding-3-small', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '3 days', now()),
  ('ku_velion_admin_runbook_2', 'doc_velion_admin_runbook', :'org_id', 2, 'Security tickets must be escalated for manual device checks, password reset review, and customer verification before closure.', 'indexed', 'ku-admin-runbook-2', '1', 'text-embedding-3-small', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '3 days', now()),
  ('ku_velion_support_guide_1', 'doc_velion_normal_support_guide', :'org_id', 1, 'WhatsApp setup requires a verification screenshot and confirmation of the business display name.', 'indexed', 'ku-support-guide-1', '1', 'text-embedding-3-small', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 days', now()),
  ('ku_velion_support_guide_2', 'doc_velion_normal_support_guide', :'org_id', 2, 'Invoice copy requests should verify account ownership before sending paid invoices to finance contacts.', 'indexed', 'ku-support-guide-2', '1', 'text-embedding-3-small', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 days', now()),
  ('ku_velion_brand_voice_1', 'doc_velion_editor_brand_voice', :'org_id', 1, 'Velion brand voice is clear, calm, practical, and focused on operational clarity.', 'indexed', 'ku-brand-voice-1', '1', 'text-embedding-3-small', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '1 day', now()),
  ('ku_velion_brand_voice_2', 'doc_velion_editor_brand_voice', :'org_id', 2, 'Social posts about support availability require admin approval before publishing.', 'indexed', 'ku-brand-voice-2', '1', 'text-embedding-3-small', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '1 day', now())
ON CONFLICT (knowledge_id) DO UPDATE
SET text = EXCLUDED.text,
    embedding_status = EXCLUDED.embedding_status,
    content_hash = EXCLUDED.content_hash,
    embedding_model = EXCLUDED.embedding_model,
    metadata = EXCLUDED.metadata,
    updated_at = now();

INSERT INTO document_acl (acl_id, org_id, document_id, user_id, permission_level, created_at)
VALUES
  ('acl_velion_admin_doc_admin', :'org_id', 'doc_velion_admin_runbook', :'admin_user_id', 'owner', now() - interval '3 days'),
  ('acl_velion_admin_doc_normal', :'org_id', 'doc_velion_admin_runbook', :'normal_user_id', 'read', now() - interval '3 days'),
  ('acl_velion_support_doc_normal', :'org_id', 'doc_velion_normal_support_guide', :'normal_user_id', 'owner', now() - interval '2 days'),
  ('acl_velion_support_doc_admin', :'org_id', 'doc_velion_normal_support_guide', :'admin_user_id', 'write', now() - interval '2 days'),
  ('acl_velion_brand_doc_editor', :'org_id', 'doc_velion_editor_brand_voice', :'editor_user_id', 'owner', now() - interval '1 day'),
  ('acl_velion_brand_doc_admin', :'org_id', 'doc_velion_editor_brand_voice', :'admin_user_id', 'write', now() - interval '1 day')
ON CONFLICT (acl_id) DO UPDATE
SET permission_level = EXCLUDED.permission_level;

INSERT INTO wiki_pages (
  page_id,
  org_id,
  workspace_id,
  title,
  path,
  current_version_id,
  page_status,
  backlinks,
  metadata,
  created_at,
  updated_at
)
VALUES
  ('wiki_velion_support_playbook', :'org_id', 'workspace_velion_demo', 'Support playbook', '/support/playbook', 'wikiver_velion_support_playbook_v1', 'published', jsonb_build_array(), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 days', now()),
  ('wiki_velion_billing_escalations', :'org_id', 'workspace_velion_demo', 'Billing escalations', '/support/billing-escalations', 'wikiver_velion_billing_v1', 'published', jsonb_build_array('/support/playbook'), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 days', now()),
  ('wiki_velion_brand_social', :'org_id', 'workspace_velion_demo', 'Brand and social approvals', '/content/brand-social', 'wikiver_velion_brand_social_v1', 'published', jsonb_build_array('/support/playbook'), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '1 day', now())
ON CONFLICT (page_id) DO UPDATE
SET title = EXCLUDED.title,
    path = EXCLUDED.path,
    current_version_id = EXCLUDED.current_version_id,
    page_status = EXCLUDED.page_status,
    backlinks = EXCLUDED.backlinks,
    metadata = EXCLUDED.metadata,
    updated_at = now();

INSERT INTO wiki_page_versions (
  version_id,
  page_id,
  content,
  source_refs,
  proposed_by_agent,
  proposed_by_user,
  approved_by,
  edit_reason,
  version_status,
  metadata,
  created_at,
  published_at
)
VALUES
  ('wikiver_velion_support_playbook_v1', 'wiki_velion_support_playbook', 'Use ticket queues for support triage. Refund and security work should stay visible in ticket history with SLA context.', jsonb_build_array(jsonb_build_object('document_id', 'doc_velion_admin_runbook')), 'seed-agent', :'admin_user_id', :'admin_user_id', 'Local demo baseline.', 'published', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 days', now() - interval '2 days'),
  ('wikiver_velion_billing_v1', 'wiki_velion_billing_escalations', 'Billing escalations should include the customer, ticket key, refund evidence, and invoice status before handoff.', jsonb_build_array(jsonb_build_object('document_id', 'doc_velion_admin_runbook')), 'seed-agent', :'normal_user_id', :'admin_user_id', 'Local demo baseline.', 'published', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 days', now() - interval '2 days'),
  ('wikiver_velion_brand_social_v1', 'wiki_velion_brand_social', 'Content editors draft social posts, then admins approve customer-facing support availability updates before publishing.', jsonb_build_array(jsonb_build_object('document_id', 'doc_velion_editor_brand_voice')), 'seed-agent', :'editor_user_id', :'admin_user_id', 'Local demo baseline.', 'published', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '1 day', now() - interval '1 day')
ON CONFLICT (version_id) DO UPDATE
SET content = EXCLUDED.content,
    source_refs = EXCLUDED.source_refs,
    proposed_by_agent = EXCLUDED.proposed_by_agent,
    proposed_by_user = EXCLUDED.proposed_by_user,
    approved_by = EXCLUDED.approved_by,
    edit_reason = EXCLUDED.edit_reason,
    version_status = EXCLUDED.version_status,
    metadata = EXCLUDED.metadata,
    published_at = EXCLUDED.published_at;

INSERT INTO retrieval_runs (
  trace_id,
  org_id,
  query,
  query_embedding_model,
  index_version,
  filters_json,
  reranker_name,
  reranker_model,
  zdr_mode,
  top_k,
  dense_retrieval_ms,
  sparse_retrieval_ms,
  rerank_ms,
  total_ms,
  candidate_count_dense,
  candidate_count_sparse,
  candidate_count_fused,
  candidate_count_reranked,
  created_at,
  mode_mix,
  zdr_actions_applied,
  mode_mix_applied
)
VALUES
  ('retrieval_velion_refund_demo', :'org_id', 'refund blocked delivery evidence billing handoff', 'text-embedding-3-small', 'local-demo-v1', jsonb_build_object('ticket_id', 'ticket_refund_delivery'), 'rrf', 'local-rerank', 'internal', 5, 22, 8, 4, 39, 8, 5, 6, 3, now() - interval '1 hour', jsonb_build_object('dense', 0.7, 'sparse', 0.3), jsonb_build_array('allow_internal'), jsonb_build_object('dense', true, 'sparse', true)),
  ('retrieval_velion_social_demo', :'org_id', 'brand voice social approval support availability', 'text-embedding-3-small', 'local-demo-v1', jsonb_build_object('ticket_id', 'ticket_social_schedule'), 'rrf', 'local-rerank', 'internal', 5, 20, 9, 5, 42, 7, 6, 6, 3, now() - interval '45 minutes', jsonb_build_object('dense', 0.6, 'sparse', 0.4), jsonb_build_array('allow_internal'), jsonb_build_object('dense', true, 'sparse', true))
ON CONFLICT (trace_id) DO UPDATE
SET query = EXCLUDED.query,
    filters_json = EXCLUDED.filters_json,
    total_ms = EXCLUDED.total_ms,
    mode_mix = EXCLUDED.mode_mix,
    zdr_actions_applied = EXCLUDED.zdr_actions_applied,
    mode_mix_applied = EXCLUDED.mode_mix_applied;

INSERT INTO access_audit_log (request_id, user_id, org_id, endpoint, http_status, latency_ms, auth_method, document_ids, cause, created_at)
SELECT request_id, user_id, :'org_id', endpoint, 200, latency_ms, 'session', document_ids, 'ok', created_at
FROM (
  VALUES
    ('audit_velion_admin_doc_lookup', :'admin_user_id', '/api/v1/documents/search', 42, ARRAY['doc_velion_admin_runbook'], now() - interval '1 hour'),
    ('audit_velion_normal_doc_lookup', :'normal_user_id', '/api/v1/documents/search', 38, ARRAY['doc_velion_normal_support_guide'], now() - interval '50 minutes'),
    ('audit_velion_editor_doc_lookup', :'editor_user_id', '/api/v1/documents/search', 40, ARRAY['doc_velion_editor_brand_voice'], now() - interval '45 minutes')
) AS seed(request_id, user_id, endpoint, latency_ms, document_ids, created_at)
WHERE NOT EXISTS (
  SELECT 1
  FROM access_audit_log existing
  WHERE existing.request_id = seed.request_id
);

COMMIT;
SQL
}

seed_model_plane() {
  echo "Seeding session_core..."
  run_psql model-plane-postgres-1 postgres session_core <<'SQL'
BEGIN;

INSERT INTO threads (id, session_key, org_id, user_id, created_at)
VALUES
  ('thread_velion_admin_triage', 'velion-demo-admin-triage', :'org_id', :'admin_user_id', now() - interval '2 hours'),
  ('thread_velion_normal_support', 'velion-demo-normal-support', :'org_id', :'normal_user_id', now() - interval '90 minutes'),
  ('thread_velion_editor_campaign', 'velion-demo-editor-campaign', :'org_id', :'editor_user_id', now() - interval '75 minutes')
ON CONFLICT (id) DO UPDATE
SET session_key = EXCLUDED.session_key,
    org_id = EXCLUDED.org_id,
    user_id = EXCLUDED.user_id;

INSERT INTO messages (id, thread_id, role, content, metadata, created_at)
VALUES
  ('mp_msg_admin_user_1', 'thread_velion_admin_triage', 'user', 'Find tickets that need security or billing escalation for Velion AS.', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 hours'),
  ('mp_msg_admin_assistant_1', 'thread_velion_admin_triage', 'assistant', 'I found the security login case and the refund delivery handoff. Both are linked to runbook guidance and SLA context.', jsonb_build_object('seed', 'velion-local-demo', 'tickets', jsonb_build_array('ticket_security_login','ticket_refund_delivery')), now() - interval '119 minutes'),
  ('mp_msg_normal_user_1', 'thread_velion_normal_support', 'user', 'Show me what to ask the customer for WhatsApp setup.', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '90 minutes'),
  ('mp_msg_normal_assistant_1', 'thread_velion_normal_support', 'assistant', 'Ask for the business verification screenshot and confirm the WhatsApp display name before completing setup.', jsonb_build_object('seed', 'velion-local-demo', 'document_id', 'doc_velion_normal_support_guide'), now() - interval '89 minutes'),
  ('mp_msg_editor_user_1', 'thread_velion_editor_campaign', 'user', 'Prepare a social follow-up for the support availability campaign.', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '75 minutes'),
  ('mp_msg_editor_assistant_1', 'thread_velion_editor_campaign', 'assistant', 'I created a scheduled Instagram follow-up and kept the LinkedIn version pending for admin approval.', jsonb_build_object('seed', 'velion-local-demo', 'post_id', 'post_velion_campaign_followup'), now() - interval '74 minutes')
ON CONFLICT (id) DO UPDATE
SET role = EXCLUDED.role,
    content = EXCLUDED.content,
    metadata = EXCLUDED.metadata,
    created_at = EXCLUDED.created_at;

INSERT INTO runs (
  id,
  thread_id,
  agent_id,
  goal,
  mode,
  status,
  org_id,
  user_id,
  final_output,
  metadata,
  created_at,
  updated_at,
  ended_at,
  residency
)
VALUES
  ('run_velion_admin_triage', 'thread_velion_admin_triage', 'velion-balance', 'Triage security and billing tickets for Velion AS.', 'execute', 'completed', :'org_id', :'admin_user_id', 'Security and refund tickets need active follow-up.', jsonb_build_object('seed', 'velion-local-demo', 'tools', jsonb_build_array('ticket_search','knowledge_lookup')), now() - interval '2 hours', now() - interval '119 minutes', now() - interval '119 minutes', 'swedencentral'),
  ('run_velion_normal_support', 'thread_velion_normal_support', 'velion-balance', 'Retrieve WhatsApp setup steps for a support ticket.', 'execute', 'completed', :'org_id', :'normal_user_id', 'Ask for verification screenshot and display name.', jsonb_build_object('seed', 'velion-local-demo', 'tools', jsonb_build_array('knowledge_lookup')), now() - interval '90 minutes', now() - interval '89 minutes', now() - interval '89 minutes', 'swedencentral'),
  ('run_velion_editor_campaign', 'thread_velion_editor_campaign', 'velion-balance', 'Create campaign follow-up draft and approval context.', 'execute', 'completed', :'org_id', :'editor_user_id', 'Instagram scheduled; LinkedIn pending approval.', jsonb_build_object('seed', 'velion-local-demo', 'tools', jsonb_build_array('social_draft','approval_lookup')), now() - interval '75 minutes', now() - interval '74 minutes', now() - interval '74 minutes', 'swedencentral')
ON CONFLICT (id) DO UPDATE
SET goal = EXCLUDED.goal,
    status = EXCLUDED.status,
    final_output = EXCLUDED.final_output,
    metadata = EXCLUDED.metadata,
    updated_at = EXCLUDED.updated_at,
    ended_at = EXCLUDED.ended_at;

INSERT INTO plans (id, thread_id, run_id, status, goal, org_id, user_id, metadata, created_at, updated_at)
VALUES
  ('plan_velion_admin_triage', 'thread_velion_admin_triage', 'run_velion_admin_triage', 'completed', 'Inspect tickets, pull knowledge, and summarize next actions.', :'org_id', :'admin_user_id', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 hours', now() - interval '119 minutes'),
  ('plan_velion_normal_support', 'thread_velion_normal_support', 'run_velion_normal_support', 'completed', 'Retrieve support guide and answer the setup question.', :'org_id', :'normal_user_id', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '90 minutes', now() - interval '89 minutes'),
  ('plan_velion_editor_campaign', 'thread_velion_editor_campaign', 'run_velion_editor_campaign', 'completed', 'Draft social follow-up and track approval.', :'org_id', :'editor_user_id', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '75 minutes', now() - interval '74 minutes')
ON CONFLICT (id) DO UPDATE
SET status = EXCLUDED.status,
    goal = EXCLUDED.goal,
    metadata = EXCLUDED.metadata,
    updated_at = EXCLUDED.updated_at;

INSERT INTO plan_steps (id, plan_id, ordinal, kind, status, payload, metadata, created_at, updated_at)
VALUES
  ('step_velion_admin_1', 'plan_velion_admin_triage', 1, 'tool', 'completed', jsonb_build_object('tool', 'ticket_search', 'query', 'security billing escalation'), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '2 hours', now() - interval '119 minutes'),
  ('step_velion_admin_2', 'plan_velion_admin_triage', 2, 'tool', 'completed', jsonb_build_object('tool', 'knowledge_lookup', 'documents', jsonb_build_array('doc_velion_admin_runbook')), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '119 minutes', now() - interval '119 minutes'),
  ('step_velion_admin_3', 'plan_velion_admin_triage', 3, 'response', 'completed', jsonb_build_object('summary', 'Security and refund tickets need follow-up.'), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '119 minutes', now() - interval '119 minutes'),
  ('step_velion_normal_1', 'plan_velion_normal_support', 1, 'tool', 'completed', jsonb_build_object('tool', 'knowledge_lookup', 'documents', jsonb_build_array('doc_velion_normal_support_guide')), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '90 minutes', now() - interval '89 minutes'),
  ('step_velion_normal_2', 'plan_velion_normal_support', 2, 'response', 'completed', jsonb_build_object('summary', 'Ask for screenshot and display name.'), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '89 minutes', now() - interval '89 minutes'),
  ('step_velion_editor_1', 'plan_velion_editor_campaign', 1, 'tool', 'completed', jsonb_build_object('tool', 'social_draft', 'post_id', 'post_velion_campaign_followup'), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '75 minutes', now() - interval '74 minutes'),
  ('step_velion_editor_2', 'plan_velion_editor_campaign', 2, 'response', 'completed', jsonb_build_object('summary', 'Instagram scheduled and LinkedIn pending approval.'), jsonb_build_object('seed', 'velion-local-demo'), now() - interval '74 minutes', now() - interval '74 minutes')
ON CONFLICT (id) DO UPDATE
SET ordinal = EXCLUDED.ordinal,
    kind = EXCLUDED.kind,
    status = EXCLUDED.status,
    payload = EXCLUDED.payload,
    metadata = EXCLUDED.metadata,
    updated_at = EXCLUDED.updated_at;

INSERT INTO todos (id, plan_id, thread_id, ordinal, content, status, priority, metadata, created_at, updated_at)
VALUES
  ('todo_velion_admin_refund', 'plan_velion_admin_triage', 'thread_velion_admin_triage', 1, 'Confirm refund evidence with Billing.', 'pending', 'high', jsonb_build_object('ticket_id', 'ticket_refund_delivery', 'seed', 'velion-local-demo'), now() - interval '119 minutes', now()),
  ('todo_velion_admin_security', 'plan_velion_admin_triage', 'thread_velion_admin_triage', 2, 'Complete manual security review.', 'pending', 'urgent', jsonb_build_object('ticket_id', 'ticket_security_login', 'seed', 'velion-local-demo'), now() - interval '119 minutes', now()),
  ('todo_velion_normal_whatsapp', 'plan_velion_normal_support', 'thread_velion_normal_support', 1, 'Wait for WhatsApp verification screenshot.', 'pending', 'normal', jsonb_build_object('ticket_id', 'ticket_whatsapp_setup', 'seed', 'velion-local-demo'), now() - interval '89 minutes', now()),
  ('todo_velion_editor_linkedin', 'plan_velion_editor_campaign', 'thread_velion_editor_campaign', 1, 'Get admin approval for LinkedIn update.', 'pending', 'normal', jsonb_build_object('post_id', 'post_velion_linkedin_update', 'seed', 'velion-local-demo'), now() - interval '74 minutes', now())
ON CONFLICT (id) DO UPDATE
SET content = EXCLUDED.content,
    status = EXCLUDED.status,
    priority = EXCLUDED.priority,
    metadata = EXCLUDED.metadata,
    updated_at = now();

INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, idempotency_key, resource_ref, producer)
VALUES
  ('event_velion_admin_run_completed', 'run.completed', 'run_velion_admin_triage', jsonb_build_object('final_output', 'Security and refund tickets need active follow-up.'), now() - interval '119 minutes', :'org_id', :'admin_user_id', 'corr_velion_admin_triage', 'idem_velion_admin_run_completed', 'run:run_velion_admin_triage', 'velion-local-demo'),
  ('event_velion_normal_run_completed', 'run.completed', 'run_velion_normal_support', jsonb_build_object('final_output', 'Ask for verification screenshot and display name.'), now() - interval '89 minutes', :'org_id', :'normal_user_id', 'corr_velion_normal_support', 'idem_velion_normal_run_completed', 'run:run_velion_normal_support', 'velion-local-demo'),
  ('event_velion_editor_run_completed', 'run.completed', 'run_velion_editor_campaign', jsonb_build_object('final_output', 'Instagram scheduled; LinkedIn pending approval.'), now() - interval '74 minutes', :'org_id', :'editor_user_id', 'corr_velion_editor_campaign', 'idem_velion_editor_run_completed', 'run:run_velion_editor_campaign', 'velion-local-demo')
ON CONFLICT (id) DO UPDATE
SET event_type = EXCLUDED.event_type,
    payload = EXCLUDED.payload,
    ts = EXCLUDED.ts,
    org_id = EXCLUDED.org_id,
    user_id = EXCLUDED.user_id,
    correlation_id = EXCLUDED.correlation_id,
    idempotency_key = EXCLUDED.idempotency_key,
    resource_ref = EXCLUDED.resource_ref,
    producer = EXCLUDED.producer;

INSERT INTO tasks (
  id,
  org_id,
  run_id,
  kind,
  title,
  description,
  assignee,
  status,
  priority,
  inputs,
  outputs,
  config_json,
  idempotency_key,
  started_at,
  completed_at,
  created_by,
  created_at,
  updated_at
)
VALUES
  ('task_velion_admin_triage', :'org_id', 'run_velion_admin_triage', 'agent', 'Ticket triage summary', 'Summarize admin ticket escalations for local demo.', 'velion-balance', 'completed', 20, jsonb_build_array(jsonb_build_object('ticket_ids', jsonb_build_array('ticket_refund_delivery','ticket_security_login'))), jsonb_build_array(jsonb_build_object('kind', 'summary', 'value', 'Two active escalations found.')), jsonb_build_object('seed', 'velion-local-demo'), 'idem_task_velion_admin_triage', now() - interval '2 hours', now() - interval '119 minutes', :'admin_user_id', now() - interval '2 hours', now()),
  ('task_velion_editor_campaign', :'org_id', 'run_velion_editor_campaign', 'agent', 'Social campaign follow-up', 'Draft and track social campaign follow-up.', 'velion-balance', 'completed', 10, jsonb_build_array(jsonb_build_object('campaign_id', 'campaign_velion_summer_support')), jsonb_build_array(jsonb_build_object('post_id', 'post_velion_campaign_followup')), jsonb_build_object('seed', 'velion-local-demo'), 'idem_task_velion_editor_campaign', now() - interval '75 minutes', now() - interval '74 minutes', :'editor_user_id', now() - interval '75 minutes', now())
ON CONFLICT (id) DO UPDATE
SET title = EXCLUDED.title,
    description = EXCLUDED.description,
    assignee = EXCLUDED.assignee,
    status = EXCLUDED.status,
    priority = EXCLUDED.priority,
    inputs = EXCLUDED.inputs,
    outputs = EXCLUDED.outputs,
    config_json = EXCLUDED.config_json,
    idempotency_key = EXCLUDED.idempotency_key,
    started_at = EXCLUDED.started_at,
    completed_at = EXCLUDED.completed_at,
    created_by = EXCLUDED.created_by,
    updated_at = now();

INSERT INTO task_events (id, task_id, event_type, actor, payload, ts)
VALUES
  ('taskevent_velion_admin_completed', 'task_velion_admin_triage', 'completed', 'velion-balance', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '119 minutes'),
  ('taskevent_velion_editor_completed', 'task_velion_editor_campaign', 'completed', 'velion-balance', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '74 minutes')
ON CONFLICT (id) DO UPDATE
SET event_type = EXCLUDED.event_type,
    actor = EXCLUDED.actor,
    payload = EXCLUDED.payload,
    ts = EXCLUDED.ts;

INSERT INTO task_artifacts (id, task_id, role, kind, name, mime_type, uri, size_bytes, checksum, metadata, created_at)
VALUES
  ('artifact_velion_admin_summary', 'task_velion_admin_triage', 'output', 'json', 'ticket-triage-summary.json', 'application/json', 'local://velion-demo/ticket-triage-summary.json', 1024, 'sha256-admin-summary-demo', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '119 minutes'),
  ('artifact_velion_editor_post', 'task_velion_editor_campaign', 'output', 'json', 'social-followup-draft.json', 'application/json', 'local://velion-demo/social-followup-draft.json', 2048, 'sha256-editor-post-demo', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '74 minutes')
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    mime_type = EXCLUDED.mime_type,
    uri = EXCLUDED.uri,
    size_bytes = EXCLUDED.size_bytes,
    checksum = EXCLUDED.checksum,
    metadata = EXCLUDED.metadata;

INSERT INTO cost_entries (
  id,
  org_id,
  user_id,
  run_id,
  request_id,
  model,
  input_tokens,
  output_tokens,
  cost_usd,
  idempotency_key,
  metadata,
  created_at
)
VALUES
  ('11111111-1111-4111-8111-111111111111', :'org_id', :'admin_user_id', 'run_velion_admin_triage', 'req_velion_admin_triage', 'velion-balance', 2200, 560, 0.0064000000, 'cost_velion_admin_triage', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '119 minutes'),
  ('22222222-2222-4222-8222-222222222222', :'org_id', :'normal_user_id', 'run_velion_normal_support', 'req_velion_normal_support', 'velion-balance', 960, 240, 0.0024000000, 'cost_velion_normal_support', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '89 minutes'),
  ('33333333-3333-4333-8333-333333333333', :'org_id', :'editor_user_id', 'run_velion_editor_campaign', 'req_velion_editor_campaign', 'velion-balance', 1340, 420, 0.0041000000, 'cost_velion_editor_campaign', jsonb_build_object('seed', 'velion-local-demo'), now() - interval '74 minutes')
ON CONFLICT (id) DO UPDATE
SET user_id = EXCLUDED.user_id,
    run_id = EXCLUDED.run_id,
    request_id = EXCLUDED.request_id,
    model = EXCLUDED.model,
    input_tokens = EXCLUDED.input_tokens,
    output_tokens = EXCLUDED.output_tokens,
    cost_usd = EXCLUDED.cost_usd,
    idempotency_key = EXCLUDED.idempotency_key,
    metadata = EXCLUDED.metadata;

INSERT INTO routing_policies (
  id,
  org_id,
  name,
  description,
  strategy,
  config_json,
  model_ids,
  priority,
  enabled,
  created_by,
  created_at,
  updated_at
)
VALUES (
  'routing_velion_advanced_demo',
  :'org_id',
  'Velion Advanced demo routing',
  'Local demo policy for advanced plan balancing.',
  'weighted',
  jsonb_build_object('default_model', 'velion-balance', 'web_search_allowed', true, 'image_generation_allowed', true),
  ARRAY['velion-balance','gpt-image-1'],
  100,
  true,
  :'admin_user_id',
  now() - interval '14 days',
  now()
)
ON CONFLICT (id) DO UPDATE
SET name = EXCLUDED.name,
    description = EXCLUDED.description,
    strategy = EXCLUDED.strategy,
    config_json = EXCLUDED.config_json,
    model_ids = EXCLUDED.model_ids,
    priority = EXCLUDED.priority,
    enabled = EXCLUDED.enabled,
    created_by = EXCLUDED.created_by,
    updated_at = now(),
    deleted_at = null;

COMMIT;
SQL
}

seed_controlplane_events() {
  echo "Seeding controlplane audit and usage events..."
  run_psql controlplane-postgres aquatiq controlplane <<'SQL'
BEGIN;

INSERT INTO audit_events (occurred_at, org_id, user_id, actor_role, plane, event, subject, resource_id, outcome, details, request_id, user_agent)
SELECT occurred_at, :'org_id', user_id, actor_role, plane, event, subject, resource_id, 'ok', details, request_id, 'velion-local-demo-seed'
FROM (
  VALUES
    (now() - interval '2 hours', :'admin_user_id', 'admin', 'application', 'ticket.triage', 'Ticket triage opened', 'ticket_refund_delivery', jsonb_build_object('seed', 'velion-local-demo'), 'audit_velion_ticket_triage'),
    (now() - interval '90 minutes', :'normal_user_id', 'member', 'data', 'document.search', 'Support guide searched', 'doc_velion_normal_support_guide', jsonb_build_object('seed', 'velion-local-demo'), 'audit_velion_support_search'),
    (now() - interval '75 minutes', :'editor_user_id', 'editor', 'application', 'social.post.draft', 'Social post drafted', 'post_velion_campaign_followup', jsonb_build_object('seed', 'velion-local-demo'), 'audit_velion_social_draft')
) AS seed(occurred_at, user_id, actor_role, plane, event, subject, resource_id, details, request_id)
WHERE NOT EXISTS (
  SELECT 1
  FROM audit_events existing
  WHERE existing.request_id = seed.request_id
);

INSERT INTO usage_events (occurred_at, org_id, user_id, plane, op, tokens_in, tokens_out, bytes_in, bytes_out, cost_cents, request_id, metadata)
SELECT occurred_at, :'org_id', user_id, plane, op, tokens_in, tokens_out, bytes_in, bytes_out, cost_cents, request_id, metadata
FROM (
  VALUES
    (now() - interval '119 minutes', :'admin_user_id', 'model', 'agent.run', 2200::bigint, 560::bigint, 4096::bigint, 2048::bigint, 0.640000::numeric, 'usage_velion_admin_triage', jsonb_build_object('seed', 'velion-local-demo')),
    (now() - interval '89 minutes', :'normal_user_id', 'model', 'agent.run', 960::bigint, 240::bigint, 2048::bigint, 1024::bigint, 0.240000::numeric, 'usage_velion_normal_support', jsonb_build_object('seed', 'velion-local-demo')),
    (now() - interval '74 minutes', :'editor_user_id', 'model', 'agent.run', 1340::bigint, 420::bigint, 3072::bigint, 1536::bigint, 0.410000::numeric, 'usage_velion_editor_campaign', jsonb_build_object('seed', 'velion-local-demo'))
) AS seed(occurred_at, user_id, plane, op, tokens_in, tokens_out, bytes_in, bytes_out, cost_cents, request_id, metadata)
WHERE NOT EXISTS (
  SELECT 1
  FROM usage_events existing
  WHERE existing.request_id = seed.request_id
);

COMMIT;
SQL
}

main() {
  require_container controlplane-postgres
  require_container ingestion-postgres
  require_container application-postgres
  require_container dpv2-postgres
  require_container model-plane-postgres-1

  seed_auth_service
  seed_org_core
  seed_user_service
  seed_billing_service
  seed_integration_core
  seed_application_plane
  seed_data_plane
  seed_model_plane
  seed_controlplane_events

  echo
  echo "Velion local demo seed complete."
  echo "Org: $ORG_NAME ($ORG_ID / $ORG_SLUG), stored plan: standard (Velion Advanced)"
  echo "Users:"
  echo "  admin:  $ADMIN_EMAIL ($ADMIN_USER_ID)"
  echo "  member: $NORMAL_EMAIL ($NORMAL_USER_ID)"
  echo "  editor: $EDITOR_EMAIL ($EDITOR_USER_ID)"
}

main "$@"
