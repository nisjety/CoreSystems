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
  SUPPORT_NOTIFICATION_MODE: z.enum(['disabled', 'user_resolved']).default('disabled'),
  NOTIFICATION_SUPPORT_WORKER_SERVICE_TOKEN: z.string().default(''),
}).superRefine((value, ctx) => {
  if (
    value.SUPPORT_NOTIFICATION_MODE === 'user_resolved' &&
    value.NOTIFICATION_SUPPORT_WORKER_SERVICE_TOKEN.trim().length < 32
  ) {
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      path: ['NOTIFICATION_SUPPORT_WORKER_SERVICE_TOKEN'],
      message: 'must be at least 32 bytes when support notifications are enabled',
    });
  }
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
