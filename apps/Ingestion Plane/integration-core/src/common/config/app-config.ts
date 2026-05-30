import { z } from 'zod';

const logLevels = ['fatal', 'error', 'warn', 'info', 'debug', 'trace', 'silent'] as const;

const envSchema = z.object({
  INTEGRATION_SERVICE_NAME: z.string().trim().min(1).default('integration-core'),
  INTEGRATION_SERVICE_PORT: z.coerce.number().int().positive().default(3026),
  LOG_LEVEL: z.enum(logLevels).default('info'),
  DATABASE_URL: z.string().trim().min(1, 'DATABASE_URL is required'),
  INTEGRATION_PUBLIC_BASE_URL: z.string().url().default('http://localhost:3026'),
  AUTH_CORE_URL: z.string().url().default('http://auth-core:3011'),
  AUTH_CORE_INTERNAL_API_KEY: z.string().trim().min(1, 'AUTH_CORE_INTERNAL_API_KEY is required'),
  USER_CORE_URL: z.string().url().default('http://user-core:3012'),
  ORG_CORE_URL: z.string().url().default('http://org-core:8080'),
  BILLING_CORE_URL: z.string().url().default('http://billing-core:3014'),
  CONNECTOR_RUNTIME_BASE_URL: z.string().url().default('http://connector-runtime-engine:3003'),
  CONNECTOR_RUNTIME_PUBLIC_BASE_URL: z.string().url().default('http://localhost:3003'),
  CONNECTOR_RUNTIME_SECRET: z.string().trim().min(1, 'CONNECTOR_RUNTIME_SECRET is required'),
  CONNECTOR_RUNTIME_WEBHOOK_SECRET: z.string().trim().min(1, 'CONNECTOR_RUNTIME_WEBHOOK_SECRET is required'),
  CONNECTOR_RUNTIME_TIMEOUT_MS: z.coerce.number().int().positive().default(30000),
  DATA_PLANE_DOCUMENTS_URL: z.string().url().default('http://dpv2-documents-api:8010'),
  DATA_PLANE_INTERNAL_API_KEY: z.string().trim().optional(),
  VELION_NATS_URL: z.string().trim().min(1).default('nats://velion-nats:4222'),
  VELION_NATS_TOKEN: z.string().trim().optional(),
  MICROSOFT_INTEGRATION_KEY: z.string().trim().min(1).default('microsoft-graph'),
  GOOGLE_INTEGRATION_KEY: z.string().trim().min(1).default('google-workspace'),
  GOOGLE_DRIVE_INTEGRATION_KEY: z.string().trim().min(1).default('google-drive'),
  NOTION_INTEGRATION_KEY: z.string().trim().min(1).default('notion'),
  SLACK_INTEGRATION_KEY: z.string().trim().min(1).default('slack'),
  SHOPIFY_INTEGRATION_KEY: z.string().trim().min(1).default('shopify'),
  STRIPE_INTEGRATION_KEY: z.string().trim().min(1).default('stripe'),
  HUBSPOT_INTEGRATION_KEY: z.string().trim().min(1).default('hubspot'),
  GITHUB_INTEGRATION_KEY: z.string().trim().min(1).default('github'),
  // Token that Zammad sends in Authorization: Bearer <token> for webhook requests.
  // Set in Zammad trigger config. Optional — skips validation if not set (dev only).
  ZAMMAD_WEBHOOK_TOKEN: z.string().trim().optional(),
  // Secret that Novu uses to sign webhook requests (x-novu-signature-v1 header).
  // Optional — skips validation if not set (dev only).
  NOVU_WEBHOOK_SECRET: z.string().trim().optional(),
  // Nango API key for integration management (list connectors, check status)
  NANGO_API_KEY: z.string().trim().optional(),
  // Novu API key for notification management
  NOVU_API_KEY: z.string().trim().optional(),
  // Novu API URL (defaults to https://api.novu.co)
  NOVU_API_URL: z.string().url().optional(),
  // AI Core service URL for intelligent categorization and routing
  AI_CORE_URL: z.string().url().default('http://ai-core:8001')
});

export type AppConfig = {
  serviceName: string;
  port: number;
  logLevel: (typeof logLevels)[number];
  databaseUrl: string;
  integrationPublicBaseUrl: string;
  authCoreUrl: string;
  authCoreInternalApiKey: string;
  userCoreUrl: string;
  orgCoreUrl: string;
  billingCoreUrl: string;
  connectorRuntimeBaseUrl: string;
  connectorRuntimePublicBaseUrl: string;
  connectorRuntimeSecret: string;
  connectorRuntimeWebhookSecret: string;
  connectorRuntimeTimeoutMs: number;
  dataPlaneDocumentsUrl: string;
  dataPlaneInternalApiKey?: string;
  velionNatsUrl: string;
  velionNatsToken?: string;
  microsoftIntegrationKey: string;
  googleIntegrationKey: string;
  googleDriveIntegrationKey: string;
  notionIntegrationKey: string;
  slackIntegrationKey: string;
  shopifyIntegrationKey: string;
  stripeIntegrationKey: string;
  hubspotIntegrationKey: string;
  githubIntegrationKey: string;
  zammadWebhookToken?: string;
  novuWebhookSecret?: string;
  nangoApiKey?: string;
  novuApiKey?: string;
  novuApiUrl?: string;
  aiCoreUrl: string;
};

export function createConfig(env: NodeJS.ProcessEnv): AppConfig {
  const parsed = envSchema.safeParse(env);

  if (!parsed.success) {
    const details = parsed.error.issues.map((issue) => `${issue.path.join('.')}: ${issue.message}`).join('; ');
    throw new Error(`Invalid integration-core configuration: ${details}`);
  }

  return {
    serviceName: parsed.data.INTEGRATION_SERVICE_NAME,
    port: parsed.data.INTEGRATION_SERVICE_PORT,
    logLevel: parsed.data.LOG_LEVEL,
    databaseUrl: parsed.data.DATABASE_URL,
    integrationPublicBaseUrl: parsed.data.INTEGRATION_PUBLIC_BASE_URL,
    authCoreUrl: parsed.data.AUTH_CORE_URL,
    authCoreInternalApiKey: parsed.data.AUTH_CORE_INTERNAL_API_KEY,
    userCoreUrl: parsed.data.USER_CORE_URL,
    orgCoreUrl: parsed.data.ORG_CORE_URL,
    billingCoreUrl: parsed.data.BILLING_CORE_URL,
    connectorRuntimeBaseUrl: parsed.data.CONNECTOR_RUNTIME_BASE_URL,
    connectorRuntimePublicBaseUrl: parsed.data.CONNECTOR_RUNTIME_PUBLIC_BASE_URL,
    connectorRuntimeSecret: parsed.data.CONNECTOR_RUNTIME_SECRET,
    connectorRuntimeWebhookSecret: parsed.data.CONNECTOR_RUNTIME_WEBHOOK_SECRET,
    connectorRuntimeTimeoutMs: parsed.data.CONNECTOR_RUNTIME_TIMEOUT_MS,
    dataPlaneDocumentsUrl: parsed.data.DATA_PLANE_DOCUMENTS_URL,
    dataPlaneInternalApiKey: parsed.data.DATA_PLANE_INTERNAL_API_KEY,
    velionNatsUrl: parsed.data.VELION_NATS_URL,
    velionNatsToken: parsed.data.VELION_NATS_TOKEN,
    microsoftIntegrationKey: parsed.data.MICROSOFT_INTEGRATION_KEY,
    googleIntegrationKey: parsed.data.GOOGLE_INTEGRATION_KEY,
    googleDriveIntegrationKey: parsed.data.GOOGLE_DRIVE_INTEGRATION_KEY,
    notionIntegrationKey: parsed.data.NOTION_INTEGRATION_KEY,
    slackIntegrationKey: parsed.data.SLACK_INTEGRATION_KEY,
    shopifyIntegrationKey: parsed.data.SHOPIFY_INTEGRATION_KEY,
    stripeIntegrationKey: parsed.data.STRIPE_INTEGRATION_KEY,
    hubspotIntegrationKey: parsed.data.HUBSPOT_INTEGRATION_KEY,
    githubIntegrationKey: parsed.data.GITHUB_INTEGRATION_KEY,
    zammadWebhookToken: parsed.data.ZAMMAD_WEBHOOK_TOKEN,
    novuWebhookSecret: parsed.data.NOVU_WEBHOOK_SECRET,
    nangoApiKey: parsed.data.NANGO_API_KEY,
    novuApiKey: parsed.data.NOVU_API_KEY,
    novuApiUrl: parsed.data.NOVU_API_URL,
    aiCoreUrl: parsed.data.AI_CORE_URL
  };
}
