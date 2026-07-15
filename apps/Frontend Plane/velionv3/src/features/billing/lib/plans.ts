export type BillingPlanId = 'hobby' | 'standard' | 'pro' | 'enterprise'

export type BillingPlan = {
  id: BillingPlanId
  name: string
  priceLabel: string
  description: string
  features: string[]
  checkoutEnabled: boolean
}

export const billingPlans: BillingPlan[] = [
  {
    id: 'hobby',
    name: 'Essential',
    priceLabel: '299 kr/mnd',
    description: 'For small teams validating one customer-facing assistant.',
    features: ['Chatbot and shared inbox', 'Website and knowledge sources', 'Usage-based AI resolution pricing'],
    checkoutEnabled: true,
  },
  {
    id: 'standard',
    name: 'Advanced',
    priceLabel: '999 kr/mnd',
    description: 'For teams that need automation, routing, and more sources.',
    features: ['Automations and routing', 'Multiple team inboxes', '20 Lite seats included'],
    checkoutEnabled: true,
  },
  {
    id: 'pro',
    name: 'Expert',
    priceLabel: '1499 kr/mnd',
    description: 'For larger support teams with reporting and governance.',
    features: ['SSO and identity controls', 'SLA reporting and multibrand', '50 Lite seats included'],
    checkoutEnabled: true,
  },
  {
    id: 'enterprise',
    name: 'Custom',
    priceLabel: 'Kontakt salg',
    description: 'For volume agreements, bespoke onboarding, and extended governance.',
    features: ['Custom AI answer pricing', 'Dedicated success motion', 'Contracted security terms'],
    checkoutEnabled: false,
  },
]

const planLabels: Record<string, string> = {
  free: 'Free',
  trial: 'Trial',
  hobby: 'Essential',
  standard: 'Advanced',
  pro: 'Expert',
  enterprise: 'Custom',
}

export function normalizePlanId(plan?: string | null): string {
  const normalized = (plan ?? '').trim().toLowerCase()
  switch (normalized) {
    case 'essential':
      return 'hobby'
    case 'advanced':
      return 'standard'
    case 'expert':
      return 'pro'
    case 'custom':
      return 'enterprise'
    default:
      return normalized || 'free'
  }
}

export function planLabel(plan?: string | null): string {
  const normalized = normalizePlanId(plan)
  return planLabels[normalized] ?? normalized
}

export function paidBillingPlan(plan?: string | null): BillingPlanId | null {
  const normalized = normalizePlanId(plan)
  return billingPlans.some((item) => item.id === normalized)
    ? normalized as BillingPlanId
    : null
}

export function isCheckoutActivatingStatus(status?: string): boolean {
  const normalized = (status ?? '').trim().toLowerCase()
  return normalized === 'succeeded' ||
    normalized === 'processing' ||
    normalized === 'charged' ||
    normalized === 'reserved'
}
