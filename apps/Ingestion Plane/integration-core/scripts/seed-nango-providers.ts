/**
 * One-shot seed script that registers OAuth provider integrations in the
 * self-hosted Nango (connector-runtime-engine) instance.
 *
 * Uses Nango's REST API to PUT (upsert) each integration, making the script
 * idempotent — safe to re-run on every compose boot.
 *
 * Required environment variables:
 *   CONNECTOR_RUNTIME_BASE_URL  — Nango server URL (default http://connector-runtime-engine:3003)
 *   CONNECTOR_RUNTIME_SECRET    — Nango secret key for API auth
 *   CONNECTOR_RUNTIME_WEBHOOK_URL — integration-core Nango webhook URL
 *   MICROSOFT_CLIENT_ID / MICROSOFT_CLIENT_SECRET
 *   GOOGLE_CLIENT_ID / GOOGLE_CLIENT_SECRET
 *   GOOGLE_DRIVE_CLIENT_ID / GOOGLE_DRIVE_CLIENT_SECRET (falls back to Google)
 *   NOTION_CLIENT_ID / NOTION_CLIENT_SECRET
 *   SLACK_CLIENT_ID / SLACK_CLIENT_SECRET
 *   GITHUB_CLIENT_ID / GITHUB_CLIENT_SECRET
 *   HUBSPOT_CLIENT_ID / HUBSPOT_CLIENT_SECRET
 *   SHOPIFY_CLIENT_ID / SHOPIFY_CLIENT_SECRET
 *   STRIPE_CLIENT_ID / STRIPE_CLIENT_SECRET
 *
 * Run:
 *   npx tsx scripts/seed-nango-providers.ts
 */

interface ProviderSeed {
  uniqueKey: string;
  provider: string;
  clientId: string;
  clientSecret: string;
  scopes?: string;
}

async function seedProvider(
  baseUrl: string,
  secretKey: string,
  seed: ProviderSeed
): Promise<void> {
  const body = {
    unique_key: seed.uniqueKey,
    provider: seed.provider,
    credentials: {
      type: 'OAUTH2',
      client_id: seed.clientId,
      client_secret: seed.clientSecret,
      ...(seed.scopes ? { scopes: seed.scopes } : {})
    }
  };

  const response = await fetch(`${baseUrl}/integrations`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${secretKey}`
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(10_000)
  });

  const responseText = response.ok ? '' : await response.text().catch(() => '');

  if (response.status === 409 || integrationAlreadyExists(response.status, responseText)) {
    const patchResponse = await fetch(`${baseUrl}/integrations/${encodeURIComponent(seed.uniqueKey)}`, {
      method: 'PATCH',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${secretKey}`
      },
      body: JSON.stringify({ credentials: body.credentials }),
      signal: AbortSignal.timeout(10_000)
    });

    if (!patchResponse.ok) {
      const text = await patchResponse.text().catch(() => '(no body)');
      console.error(`  ✗ ${seed.uniqueKey}: HTTP ${patchResponse.status} — ${text}`);
      return;
    }

    console.log(`  ✓ ${seed.uniqueKey} (${seed.provider}, updated)`);
    return;
  }

  if (!response.ok) {
    console.error(`  ✗ ${seed.uniqueKey}: HTTP ${response.status} — ${responseText || '(no body)'}`);
    return;
  }

  console.log(`  ✓ ${seed.uniqueKey} (${seed.provider})`);
}

function integrationAlreadyExists(status: number, body: string): boolean {
  return status === 400 && /unique key already exists/i.test(body);
}

function optionalEnv(name: string, fallback?: string): string | undefined {
  const value = process.env[name];
  return value && value.trim() ? value : fallback;
}

async function configureWebhook(
  baseUrl: string,
  secretKey: string,
  webhookUrl: string
): Promise<void> {
  const response = await fetch(`${baseUrl}/api/v1/environments/webhook?env=dev`, {
    method: 'PATCH',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${secretKey}`
    },
    body: JSON.stringify({
      primary_url: webhookUrl,
      on_auth_creation: true,
      on_auth_refresh_error: true,
      on_sync_completion_always: true,
      on_sync_error: true,
      on_async_action_completion: true
    }),
    signal: AbortSignal.timeout(10_000)
  });

  if (!response.ok) {
    const text = await response.text().catch(() => '(no body)');
    console.error(`  ✗ webhook: HTTP ${response.status} — ${text}`);
    return;
  }

  console.log(`  ✓ webhook (${webhookUrl})`);
}

async function main(): Promise<void> {
  const baseUrl = (process.env.CONNECTOR_RUNTIME_BASE_URL ?? 'http://connector-runtime-engine:3003').replace(/\/$/, '');
  const secretKey = process.env.CONNECTOR_RUNTIME_SECRET ?? '';
  const webhookUrl = process.env.CONNECTOR_RUNTIME_WEBHOOK_URL ?? 'http://integration-api:3026/api/v1/webhooks/nango';

  if (!secretKey) {
    console.error('CONNECTOR_RUNTIME_SECRET is not set — skipping provider seed');
    process.exit(0);
  }

  await configureWebhook(baseUrl, secretKey, webhookUrl);

  const providers: ProviderSeed[] = [];

  // Microsoft Graph
  if (process.env.MICROSOFT_CLIENT_ID && process.env.MICROSOFT_CLIENT_SECRET) {
    providers.push({
      uniqueKey: process.env.MICROSOFT_INTEGRATION_KEY ?? 'microsoft-graph',
      provider: 'microsoft',
      clientId: process.env.MICROSOFT_CLIENT_ID,
      clientSecret: process.env.MICROSOFT_CLIENT_SECRET,
      scopes: optionalEnv(
        'MICROSOFT_SCOPES',
        'offline_access,User.Read,Files.Read.All,Sites.Read.All,Team.ReadBasic.All,Channel.ReadBasic.All,Chat.Read'
      )
    });
  }

  // Google Workspace
  if (process.env.GOOGLE_CLIENT_ID && process.env.GOOGLE_CLIENT_SECRET) {
    providers.push({
      uniqueKey: process.env.GOOGLE_INTEGRATION_KEY ?? 'google-workspace',
      provider: 'google',
      clientId: process.env.GOOGLE_CLIENT_ID,
      clientSecret: process.env.GOOGLE_CLIENT_SECRET,
      scopes: optionalEnv(
        'GOOGLE_SCOPES',
        'openid,email,profile,https://www.googleapis.com/auth/drive.readonly'
      )
    });
  }

  // Google Drive can reuse the same Google OAuth app when its scopes are allowed.
  const googleDriveClientId = process.env.GOOGLE_DRIVE_CLIENT_ID || process.env.GOOGLE_CLIENT_ID;
  const googleDriveClientSecret = process.env.GOOGLE_DRIVE_CLIENT_SECRET || process.env.GOOGLE_CLIENT_SECRET;
  if (googleDriveClientId && googleDriveClientSecret) {
    providers.push({
      uniqueKey: process.env.GOOGLE_DRIVE_INTEGRATION_KEY ?? 'google-drive',
      provider: 'google-drive',
      clientId: googleDriveClientId,
      clientSecret: googleDriveClientSecret,
      scopes: optionalEnv('GOOGLE_DRIVE_SCOPES', process.env.GOOGLE_SCOPES)
    });
  }

  // Notion
  if (process.env.NOTION_CLIENT_ID && process.env.NOTION_CLIENT_SECRET) {
    providers.push({
      uniqueKey: process.env.NOTION_INTEGRATION_KEY ?? 'notion',
      provider: 'notion',
      clientId: process.env.NOTION_CLIENT_ID,
      clientSecret: process.env.NOTION_CLIENT_SECRET,
      scopes: optionalEnv('NOTION_SCOPES')
    });
  }

  // Slack
  if (process.env.SLACK_CLIENT_ID && process.env.SLACK_CLIENT_SECRET) {
    providers.push({
      uniqueKey: process.env.SLACK_INTEGRATION_KEY ?? 'slack',
      provider: 'slack',
      clientId: process.env.SLACK_CLIENT_ID,
      clientSecret: process.env.SLACK_CLIENT_SECRET,
      scopes: optionalEnv(
        'SLACK_SCOPES',
        'channels:read,channels:history,groups:read,groups:history,im:read,im:history,mpim:read,mpim:history,users:read,team:read,files:read'
      )
    });
  }

  // GitHub
  if (process.env.GITHUB_CLIENT_ID && process.env.GITHUB_CLIENT_SECRET) {
    providers.push({
      uniqueKey: process.env.GITHUB_INTEGRATION_KEY ?? 'github',
      provider: 'github',
      clientId: process.env.GITHUB_CLIENT_ID,
      clientSecret: process.env.GITHUB_CLIENT_SECRET,
      scopes: optionalEnv('GITHUB_SCOPES', 'read:user,user:email,repo')
    });
  }

  // HubSpot
  if (process.env.HUBSPOT_CLIENT_ID && process.env.HUBSPOT_CLIENT_SECRET) {
    providers.push({
      uniqueKey: process.env.HUBSPOT_INTEGRATION_KEY ?? 'hubspot',
      provider: 'hubspot',
      clientId: process.env.HUBSPOT_CLIENT_ID,
      clientSecret: process.env.HUBSPOT_CLIENT_SECRET,
      scopes: optionalEnv('HUBSPOT_SCOPES')
    });
  }

  // Shopify
  if (process.env.SHOPIFY_CLIENT_ID && process.env.SHOPIFY_CLIENT_SECRET) {
    providers.push({
      uniqueKey: process.env.SHOPIFY_INTEGRATION_KEY ?? 'shopify',
      provider: 'shopify',
      clientId: process.env.SHOPIFY_CLIENT_ID,
      clientSecret: process.env.SHOPIFY_CLIENT_SECRET,
      scopes: optionalEnv('SHOPIFY_SCOPES')
    });
  }

  // Stripe
  if (process.env.STRIPE_CLIENT_ID && process.env.STRIPE_CLIENT_SECRET) {
    providers.push({
      uniqueKey: process.env.STRIPE_INTEGRATION_KEY ?? 'stripe',
      provider: 'stripe',
      clientId: process.env.STRIPE_CLIENT_ID,
      clientSecret: process.env.STRIPE_CLIENT_SECRET,
      scopes: optionalEnv('STRIPE_SCOPES', 'read_write')
    });
  }

  if (providers.length === 0) {
    console.log('No OAuth credentials provided — skipping Nango provider seed');
    console.log('Set MICROSOFT, GOOGLE, GOOGLE_DRIVE, NOTION, SLACK, GITHUB, HUBSPOT, SHOPIFY, or STRIPE client credentials to register providers');
    process.exit(0);
  }

  console.log(`Seeding ${providers.length} provider(s) into Nango at ${baseUrl}...`);

  for (const provider of providers) {
    try {
      await seedProvider(baseUrl, secretKey, provider);
    } catch (error) {
      console.error(`  ✗ ${provider.uniqueKey}: ${error instanceof Error ? error.message : error}`);
    }
  }

  console.log('Provider seed complete.');
}

main().catch((error) => {
  console.error('Seed script failed:', error);
  process.exit(1);
});
