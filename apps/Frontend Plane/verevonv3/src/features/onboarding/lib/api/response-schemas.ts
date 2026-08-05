import { z } from 'zod'
import type {
  BrregEnhet,
  PlanRecommendation,
  PlanRecommendationText,
  PreviewResponse,
} from '@/features/onboarding/lib/api/contracts'
import type { CheckoutSession, CheckoutStatus } from '@/features/billing/lib/api'

const nonEmptyString = z.string().trim().min(1)

const brregAddressSchema = z.object({
  adresse: z.array(z.string()).optional(),
  postnummer: z.string().optional(),
  poststed: z.string().optional(),
  kommune: z.string().optional(),
  land: z.string().optional(),
})

export const brregEnhetSchema = z.object({
  organisasjonsnummer: nonEmptyString,
  navn: nonEmptyString,
  antallAnsatte: z.number().int().nonnegative().optional(),
  hjemmeside: z.string().optional(),
  forretningsadresse: brregAddressSchema.optional(),
  organisasjonsform: z.object({ kode: z.string().optional(), beskrivelse: z.string().optional() }).optional(),
  naeringskode1: z.object({ kode: z.string().optional(), beskrivelse: z.string().optional() }).optional(),
  konkurs: z.boolean().optional(),
  underAvvikling: z.boolean().optional(),
}) satisfies z.ZodType<BrregEnhet>

export const brregSearchResponseSchema = z.object({
  count: z.number().int().nonnegative().optional(),
  results: z.array(brregEnhetSchema).default([]),
})

const recommendationTextSchema = z.object({
  reason: nonEmptyString,
  summary: nonEmptyString,
  proofPoints: z.array(z.string()),
  scopeSignals: z.array(z.string()),
  opportunities: z.array(z.string()),
}) satisfies z.ZodType<PlanRecommendationText>

export const planRecommendationSchema = recommendationTextSchema.extend({
  planId: z.enum(['trial', 'hobby', 'standard', 'pro', 'enterprise']),
  generatedAt: nonEmptyString,
  source: z.enum(['local', 'model']),
  connectedSourceCount: z.number().int().nonnegative().optional(),
  contextHash: z.string().optional(),
  locale: z.enum(['nb', 'en']).optional(),
  sourceCount: z.number().int().nonnegative().optional(),
  translations: z.object({
    nb: recommendationTextSchema.optional(),
    en: recommendationTextSchema.optional(),
  }).partial().optional(),
}) satisfies z.ZodType<PlanRecommendation>

export const planRecommendationResponseSchema = z.object({ recommendation: planRecommendationSchema })
export const planRecommendationTranslationResponseSchema = z.object({ translation: recommendationTextSchema })

export const graphPreviewSchema = z.object({
  nodes: z.array(z.object({ id: nonEmptyString, label: nonEmptyString, group: nonEmptyString })),
  edges: z.array(z.object({ a: nonEmptyString, b: nonEmptyString })),
  counts: z.object({
    nodes: z.number().int().nonnegative(),
    edges: z.number().int().nonnegative(),
    groups: z.number().int().nonnegative(),
  }),
}) satisfies z.ZodType<PreviewResponse>

const checkoutFields = {
  id: z.string().optional(),
  provider: z.string().optional(),
  payment_id: z.string().optional(),
  client_secret: z.string().optional(),
  publishable_key: z.string().optional(),
  client_url: z.string().optional(),
  backend_url: z.string().optional(),
  status: z.string().optional(),
  amount_cents: z.number().int().nonnegative().optional(),
  currency: z.string().optional(),
}

export const checkoutSessionSchema = z.object({
  ...checkoutFields,
  url: z.string().url().optional(),
}).superRefine((value, ctx) => {
  if (!value.provider?.trim()) {
    ctx.addIssue({ code: 'custom', path: ['provider'], message: 'provider is required' })
  }
}) satisfies z.ZodType<CheckoutSession>

export const checkoutStatusSchema = z.object({
  ...checkoutFields,
  status: nonEmptyString,
  org_id: z.string().optional(),
  plan: z.string().optional(),
}) satisfies z.ZodType<CheckoutStatus>

export const organizationCreatedSchema = z.object({
  id: nonEmptyString,
  name: nonEmptyString,
  slug: z.string().optional(),
  plan: z.string().optional(),
})

export const websiteIngestStartedSchema = z.object({ id: z.string().optional() })
export const themePersistedSchema = z.object({
  persisted: z.boolean(),
  mode: z.enum(['verevon', 'brand']),
  primaryColor: nonEmptyString,
})
export const onboardingCompletedSchema = z.object({ completed: z.boolean() })
export const planSetSchema = z.object({ id: nonEmptyString, plan: nonEmptyString })
export const connectSessionSchema = z.object({
  connectUrl: nonEmptyString,
  authMode: z.string().optional(),
  sessionToken: z.string().optional(),
})
export const discoverSourceSchema = z.object({ id: z.string().optional(), discovered: z.boolean().optional() })
export const cleanupSourceSchema = z.object({ cleaned: z.boolean().optional() })
export const sharePointWarmupSchema = z.object({ warmed: z.boolean().optional() })
export const integrationSyncSchema = z.object({ id: z.string().optional(), started: z.boolean().optional() })
export const onboardingStateSchema = z.object({ step: nonEmptyString, state: z.unknown().optional() })
export const onboardingStateSavedSchema = z.object({ success: z.boolean().optional() })
export const unknownRecordSchema = z.record(z.string(), z.unknown())
export const onboardingLifecycleSchema = z.object({
  state: z.enum(['PROFILE_READY', 'COMPLETED']),
  orgId: nonEmptyString,
})
export const shippingCarriersResponseSchema = z.object({
  carriers: z.array(z.object({
    name: nonEmptyString,
    is_mock: z.boolean().optional(),
  })).default([]),
})

export function parseOnboardingResponse<T>(schema: z.ZodType<T>, value: unknown, endpoint: string): T {
  const result = schema.safeParse(value)
  if (result.success) return result.data

  const details = result.error.issues
    .map((issue) => issue.path.join('.') || issue.message)
    .join(', ')
  throw new Error(`Invalid response from ${endpoint}: ${details}`)
}
