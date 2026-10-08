import { z } from 'zod'

// Presentation-only selection of the Home tool. It neither changes workspace
// membership nor invokes a tool; each tool still authorizes its own actions.
// The tab lives in the URL as ?tab=web|extract. Chat is the default and has no
// URL form, so a plain /dashboard is always Chat.
export const homeTabSchema = z.enum(['Chat', 'Søk', 'Crawl'])
export type HomeTab = z.infer<typeof homeTabSchema>
export const homeTabParamSchema = z.enum(['web', 'extract'])
export type HomeTabParam = z.infer<typeof homeTabParamSchema>
export const homeTabActionSchema = z.object({
  type: z.literal('home.tab.select'),
  tab: homeTabSchema,
})

const tabByParam: Readonly<Record<HomeTabParam, HomeTab>> = { web: 'Søk', extract: 'Crawl' }
const paramByTab: Readonly<Record<HomeTab, HomeTabParam | undefined>> = { Chat: undefined, Søk: 'web', Crawl: 'extract' }

// Absent, unknown, differently cased or repeated values are Chat, so a stale or
// hand-edited link still opens Home instead of an empty surface.
export function parseHomeTab(value: unknown): HomeTab {
  const parsed = homeTabParamSchema.safeParse(value)
  return parsed.success ? tabByParam[parsed.data] : 'Chat'
}

// undefined removes the param (choosing Chat), which is what a router's
// setSearchParams expects for "no value".
export function homeTabParam(tab: HomeTab): HomeTabParam | undefined {
  return paramByTab[tab]
}
