import { z } from 'zod';

const ConfigSchema = z.object({
  TEMPORAL_ADDRESS: z.string().min(1),
  TEMPORAL_NAMESPACE: z.string().default('default'),
  VELION_NATS_URL: z.string().url(),
  VELION_NATS_TOKEN: z.string().optional(),
  ZAMMAD_API_URL: z.string().url(),
  ZAMMAD_API_TOKEN: z.string().optional().default(''),
  AI_CORE_URL: z.string().url(),
  NOTIFICATION_CORE_URL: z.string().url(),
  INTERNAL_API_KEY: z.string().min(1),
});

export type Config = z.infer<typeof ConfigSchema>;

function loadConfig(): Config {
  const result = ConfigSchema.safeParse(process.env);
  if (!result.success) {
    const issues = result.error.issues
      .map((i) => `  ${i.path.join('.')}: ${i.message}`)
      .join('\n');
    throw new Error(`Invalid environment configuration:\n${issues}`);
  }
  return result.data;
}

export const config: Config = loadConfig();
