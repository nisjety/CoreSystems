import { z } from 'zod'

// Presentation-only selection. It neither changes workspace membership nor
// invokes a tool; the owning workspace still authorizes each subsequent action.
export const homeOverviewSectionSchema = z.enum(['home', 'spaces', 'studio', 'support', 'social', 'knowledge', 'insights', 'leads', 'agents'])
export type HomeOverviewSection = z.infer<typeof homeOverviewSectionSchema>
export const homeOverviewActionSchema = z.object({
  type: z.literal('home.overview.select'),
  section: homeOverviewSectionSchema,
})

export function parseHomeOverview(value: unknown): HomeOverviewSection {
  const parsed = homeOverviewSectionSchema.safeParse(value)
  return parsed.success ? parsed.data : 'home'
}

export const homeOverviewSidebarItems: Readonly<Record<string, HomeOverviewSection>> = {
  'overview-home': 'home', 'overview-spaces': 'spaces', 'overview-studio': 'studio',
  'overview-inbox': 'support', 'overview-social': 'social', 'overview-knowledge': 'knowledge',
  'overview-insights': 'insights', 'overview-leads': 'leads', 'overview-agents': 'agents',
}

export function homeOverviewGroup(section: HomeOverviewSection) {
  return ['home', 'spaces', 'studio', 'support'].includes(section) ? 'my-account' : 'shared'
}

export function homeOverviewLabel(section: HomeOverviewSection, tr: (no: string, en: string) => string): string {
  const labels: Record<HomeOverviewSection, [string, string]> = {
    home: ['Hjem', 'Home'], spaces: ['Rom', 'Rom'], studio: ['Studio', 'Studio'],
    support: ['Support', 'Support'], social: ['Sosialt', 'Social'], knowledge: ['Kunnskap', 'Knowledge'],
    insights: ['Innsikt', 'Insights'], leads: ['Leads', 'Leads'], agents: ['Agenter', 'Agents'],
  }
  return tr(...labels[section])
}
