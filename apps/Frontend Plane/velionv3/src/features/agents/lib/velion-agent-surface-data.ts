import { CheckCircle2, Sparkles, TicketCheck } from 'lucide-solid'
import type { RoleFeature } from '@/features/agents/lib/velion-agent-page-types'

export const serviceResolutionQueue = [
  { title: 'Answer policy', status: 'Configure sources', detail: 'Reply only after an approved source or tool result supports it' },
  { title: 'Action boundary', status: 'Scope tools', detail: 'Ticket, lookup, and routing permissions must be explicit' },
  { title: 'Human handoff', status: 'Map escalation', detail: 'Sensitive or low-confidence cases route with a summary' },
] as const

export const serviceActionChecks = ['Requires customer context', 'Follows approved process', 'Logs tool result', 'Escalates on policy conflict'] as const
export const serviceUnansweredTopics = ['Warranty proof', 'B2B tax invoice', 'Damaged gift order'] as const
export const serviceNextImprovements = ['Write warranty article', 'Scope refund lookup', 'Add voice fallback', 'Add VIP handoff'] as const
export const serviceGuidanceControls = ['Tone: concise', 'Answer length: standard', 'Attributes: plan, region', 'Escalation: billing'] as const
export const serviceChannelRollout = ['Chat', 'Email', 'Voice', 'WhatsApp', 'Instagram', 'Discord'] as const
export const serviceVerifiedQaRows = [
  { label: 'Verified resolution', value: 'Required' },
  { label: 'Procedure followed', value: 'Mapped' },
  { label: 'Human handoff quality', value: 'Review' },
] as const
export const serviceInsightQaRows = [
  { label: 'Source grounded', value: 'Waiting' },
  { label: 'Policy match', value: 'Audit' },
  { label: 'Automation opportunity', value: 'Backlog' },
] as const

export const salesIntentSignals = ['Pricing page trigger', 'Campaign return trigger', 'Enterprise keyword trigger', 'Security page trigger'] as const
export const salesEngagementModes = ['Text chat', 'Voice', 'Video', 'Guided tour'] as const
export const salesQualificationScores = [
  { label: 'Company fit', value: 'Define' },
  { label: 'Timeline', value: 'Required' },
  { label: 'Use case', value: 'Review' },
] as const
const salesObjections = ['Product fit question', 'Pricing needs approval', 'Security review required', 'Migration timeline unclear'] as const
export const salesObjectionRows: RoleFeature[] = salesObjections.map((item) => ({
  title: item,
  description: 'Use approved answer and route if the buyer needs custom terms.',
  status: 'Approved',
  icon: CheckCircle2,
}))
export const salesVisitorIntelligenceSignals = ['Company', 'Location', 'Account history', 'Engagement score'] as const
export const salesMeetingSlots = ['Owner calendar', 'Round-robin', 'Fallback link', 'Human handoff'] as const
export const salesCrmHandoffItems = ['Source page', 'Buying signal', 'Fit summary', 'Next step'] as const
export const salesLeadTags = ['Seat range', 'Timeline', 'Use case'] as const
export const salesBreezeResearch = ['Company growth', 'Recent hiring', 'CRM history', 'Personalized opener'] as const
export const salesInsightMetrics = [
  { label: 'Qualified meeting rate', value: 'Waiting', detail: 'Live conversion appears after connected visitor and booking events' },
  { label: 'Top objection', value: 'Waiting', detail: 'Objection clusters appear after handoffs are connected' },
  { label: 'Routing accuracy', value: 'Review', detail: 'Owner assignment needs CRM and calendar rules before measurement' },
  { label: 'Audit coverage', value: 'Required', detail: 'Every automated route must keep a review trail' },
] as const

export const ecommerceProductRecommendations = [
  { name: 'Catalog match', fit: 'Mapped attributes', price: 'Catalog price' },
  { name: 'Inventory option', fit: 'Stock checked', price: 'Catalog rule' },
  { name: 'Policy-safe pick', fit: 'Return terms', price: 'Source required' },
] as const
export const ecommerceSupportRequests = [
  { title: 'Tracking delay', detail: 'Carrier scan missing' },
  { title: 'Return request', detail: 'Inside return window' },
  { title: 'Pause subscription', detail: 'Approval required' },
] as const
export const ecommerceFinderChips = ['Use case', 'Fit', 'Budget range', 'Availability'] as const
export const ecommerceCartRecoveryCards = [
  { title: 'Exit intent', detail: 'Visitor returns to shipping step twice.', value: 'High risk' },
  { title: 'Concern', detail: 'Asks about waterproof warranty and returns.', value: 'Answerable' },
  { title: 'Nudge', detail: 'Offer compare options or save cart.', value: 'Approved' },
] as const
export const ecommerceStoreActionPlan = ['Draft product finder', 'Check low-stock rules', 'Prepare Shopify Flow approval'] as const
export const ecommerceSidekickTasks = ['Generate product copy', 'Draft admin app', 'Create discount task'] as const
export const ecommerceInsightCards = [
  { title: 'Assisted revenue', value: 'Waiting', detail: 'Attribution appears after product and checkout events connect' },
  { title: 'Cart recovery', value: 'Waiting', detail: 'Recovery rate appears after cart events connect' },
  { title: 'Top product gap', value: 'Backlog', detail: 'Repeated pre-purchase uncertainty is clustered from live conversations' },
] as const
export const ecommerceAssistantModules = [
  { title: 'Shopping Assistant', detail: 'Product questions, upsells, discounts, and guided buying.', icon: Sparkles },
  { title: 'Support Agent', detail: 'Order tracking, returns, edits, subscriptions, and FAQs.', icon: TicketCheck },
] as const
export const ecommerceProductQuestions = ['What size should I choose?', 'Are these waterproof?', 'Can I return if they do not fit?'] as const
export const ecommerceQuickReplies = ['Compare options', 'Show sizes', 'Add care kit'] as const
