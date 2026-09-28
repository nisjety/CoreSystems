import type { HomeOverviewSection } from '@/shared/actions/home-overview'

export type HomeWorkSection = Exclude<HomeOverviewSection, 'home'>
export type HomeSectionItem = { id: string; title: string; detail?: string; status?: string; href?: string; updatedAt?: string; value?: string }
export type HomeSectionData = {
  items: HomeSectionItem[]
  metrics: { label: string; value: number | string }[]
  note?: string
}
type Translate = (no: string, en: string) => string
const newest = <T extends { updatedAt?: string }>(items: T[]) => [...items].sort((a, b) => (Date.parse(b.updatedAt ?? '') || 0) - (Date.parse(a.updatedAt ?? '') || 0))

export function homeSectionDescription(section: HomeWorkSection, tr: Translate): string {
  const descriptions: Record<HomeWorkSection, [string, string]> = {
    spaces: ['Samtaler og samarbeid i rommene dine.', 'Conversations and collaboration across your rooms.'],
    studio: ['Dine visuelle prosjekter og arbeidet som er underveis.', 'Your visual projects and work in progress.'],
    support: ['Nylige kundesamtaler og oppfølging.', 'Recent customer conversations and follow-up.'],
    social: ['Innhold, publisering og tilkoblede kanaler.', 'Content, publishing and connected channels.'],
    knowledge: ['Kildene arbeidsområdet kan hente svar fra.', 'The sources your workspace can draw answers from.'],
    insights: ['Registrerte målinger fra arbeidsområdet.', 'Recorded measurements from your workspace.'],
    leads: ['Lagrede bedriftslister og muligheter å følge opp.', 'Saved company lists and opportunities to follow up.'],
    agents: ['Status for planlagte kjøringer og bakgrunnsarbeid.', 'Status of scheduled runs and background work.'],
  }
  return tr(...descriptions[section])
}

export function homeSectionContent(section: HomeWorkSection, tr: Translate) {
  const content: Record<HomeWorkSection, { title: string; empty: string; href: string; links: { label: string; href: string }[] }> = {
    spaces: { title: tr('Rommene dine', 'Your rooms'), empty: tr('Ingen rom er tilgjengelige ennå.', 'No rooms are available yet.'), href: '/spaces', links: [{ label: tr('Se alle rom', 'Browse rooms'), href: '/spaces' }] },
    studio: { title: tr('Prosjekter under arbeid', 'Work in progress'), empty: tr('Ingen studioprosjekter ennå.', 'No studio projects yet.'), href: '/studio/canvas', links: [{ label: 'Canvas', href: '/studio/canvas' }, { label: tr('Kampanjer', 'Campaigns'), href: '/studio/campaigns' }, { label: tr('Maler', 'Templates'), href: '/studio/templates' }] },
    support: { title: tr('Nylige samtaler', 'Recent conversations'), empty: tr('Ingen kundesamtaler ennå.', 'No customer conversations yet.'), href: '/support', links: [{ label: tr('Samtaler', 'Conversations'), href: '/support' }, { label: tr('Saker', 'Tickets'), href: '/support?surface=tickets' }, { label: tr('AI-utkast', 'AI drafts'), href: '/support?surface=drafts' }] },
    social: { title: tr('Innhold og publisering', 'Content & publishing'), empty: tr('Ingen innlegg ennå.', 'No posts yet.'), href: '/social/calendar', links: [{ label: tr('Kalender', 'Calendar'), href: '/social/calendar' }, { label: tr('Utkast', 'Drafts'), href: '/social/drafts' }, { label: tr('Godkjenninger', 'Approvals'), href: '/social/approvals' }] },
    knowledge: { title: tr('Kilder i arbeidsområdet', 'Workspace sources'), empty: tr('Ingen kunnskapskilder ennå.', 'No knowledge sources yet.'), href: '/knowledge', links: [{ label: tr('Kilder og dokumenter', 'Sources & documents'), href: '/knowledge' }, { label: tr('Innhentinger', 'Ingestions'), href: '/ingestions' }] },
    insights: { title: tr('Registrerte målinger', 'Recorded measurements'), empty: tr('Ingen målinger er registrert ennå.', 'No measurements have been recorded yet.'), href: '/insights/overview', links: [{ label: tr('Alle målinger', 'All measurements'), href: '/insights/overview' }] },
    leads: { title: tr('Lagrede bedriftslister', 'Saved company lists'), empty: tr('Ingen lagrede bedriftslister ennå.', 'No saved company lists yet.'), href: '/leads', links: [{ label: tr('Finn bedrifter', 'Find companies'), href: '/leads' }] },
    agents: { title: tr('Planlagte kjøringer og bakgrunnskjøringer', 'Scheduled & background runs'), empty: tr('Ingen bakgrunnskjøringer ennå.', 'No background runs yet.'), href: '/agents/runs', links: [{ label: tr('Kjørekonsoll', 'Run console'), href: '/agents/runs' }, { label: tr('Alle agenter', 'All agents'), href: '/agents' }, { label: tr('Kostnad', 'Cost'), href: '/agents/cost' }] },
  }
  return content[section]
}

/** Load only the selected overview through existing owner-scoped clients.
 * A page count is labelled as such; it is never presented as an org total. */
export async function loadHomeSection(section: HomeWorkSection, orgId: string, tr: Translate): Promise<HomeSectionData> {
  switch (section) {
    case 'spaces': {
      const spaces = await (await import('@/shared/api/spaces-client')).listSpaces()
      return { items: spaces.map((space) => ({ id: space.space_ref, title: space.name, detail: space.kind === 'personal' ? tr('Personlig rom', 'Personal room') : space.kind === 'shared' ? tr('Delt rom', 'Shared room') : space.kind === 'room' ? tr('Rom', 'Room') : space.kind, status: space.lifecycle, href: `/spaces/${encodeURIComponent(space.space_ref)}` })), metrics: [{ label: tr('Tilgjengelige rom', 'Available rooms'), value: spaces.length }, { label: tr('Aktive rom', 'Active rooms'), value: spaces.filter((space) => space.lifecycle === 'active').length }] }
    }
    case 'studio': {
      const { projects } = await (await import('@/shared/api/studio-client')).listStudioProjects(orgId)
      return { items: newest(projects).map((project) => ({ id: project.id, title: project.title, detail: tr(`${project.blocks.length} elementer`, `${project.blocks.length} elements`), status: project.status, updatedAt: project.updatedAt })), metrics: [{ label: tr('Prosjekter', 'Projects'), value: projects.length }], note: projects.some((project) => project.persistence === 'ephemeral') ? tr('Studio lagrer for tiden prosjekter midlertidig. De kan forsvinne ved en omstart av tjenesten.', 'Studio currently stores projects temporarily. They may be lost when the service restarts.') : undefined }
    }
    case 'support': {
      const { tickets, nextCursor } = await (await import('@/shared/api/inbox-client')).listConversations(orgId, { limit: 6 })
      return { items: tickets.map((ticket) => ({ id: ticket.conversationId, title: ticket.title, detail: ticket.lastMessagePreview, status: ticket.state?.name, updatedAt: ticket.updated_at, href: `/support?conversation_id=${encodeURIComponent(ticket.conversationId)}` })), metrics: [{ label: tr('Samtaler i denne oversikten', 'Conversations in this view'), value: tickets.length }], note: nextCursor ? tr('Viser de siste seks samtalene. Åpne Support for hele køen.', 'Showing the six most recent conversations. Open Support for the full queue.') : undefined }
    }
    case 'social': {
      const { posts, accounts } = await (await import('@/shared/api/social-client')).getSocialCalendar(orgId)
      return { items: posts.map((post) => ({ id: post.id, title: post.title, detail: post.platforms.join(' · '), status: post.status, updatedAt: post.scheduledAt })), metrics: [{ label: tr('Planlagte innlegg', 'Scheduled posts'), value: posts.filter((post) => post.status === 'scheduled').length }, { label: tr('Tilkoblede kanaler', 'Connected channels'), value: accounts.filter((account) => account.status === 'connected').length }] }
    }
    case 'knowledge': {
      const sources = await (await import('@/shared/api/knowledge-client')).listSources(orgId)
      return { items: sources.map((source) => ({ id: source.id, title: source.name, detail: source.type, status: source.status, updatedAt: source.lastSync, href: `/knowledge?source=${encodeURIComponent(source.id)}` })), metrics: [{ label: tr('Kilder i denne oversikten', 'Sources in this view'), value: sources.length }] }
    }
    case 'insights': {
      const data = await (await import('@/shared/api/insights-client')).getInsightsOverview()
      return { items: data.scorecards.map((metric) => ({ id: metric.id, title: metric.label || metric.metric, detail: metric.source, value: `${metric.value.toLocaleString()} ${metric.unit}`.trim() })), metrics: [{ label: tr('Registrerte målinger', 'Recorded measurements'), value: data.scorecards.length }] }
    }
    case 'leads': {
      const lists = await (await import('@/shared/api/leads-client')).listLeadLists()
      return { items: newest(lists.map((list) => ({ id: list.id, title: list.name, detail: tr(`${list.company_count} bedrifter`, `${list.company_count} companies`), updatedAt: list.updated_at }))), metrics: [{ label: tr('Lagrede lister', 'Saved lists'), value: lists.length }] }
    }
    case 'agents': {
      const { runs, hasMore } = await (await import('@/shared/api/runs-client')).listSystemRuns({ limit: 6 })
      return { items: runs.map((run) => ({ id: run.runId, title: run.goal || run.runId, detail: run.agentId, status: run.status, updatedAt: run.updatedAt })), metrics: [{ label: tr('Bakgrunnskjøringer i denne oversikten', 'Background runs in this view'), value: runs.length }], note: tr('Denne oversikten viser planlagte kjøringer og bakgrunnskjøringer. Åpne kjørekonsollen for flere detaljer.', 'This overview shows scheduled and background runs. Open the run console for more detail.') + (hasMore ? tr(' Flere kjøringer er tilgjengelige.', ' More runs are available.') : '') }
    }
  }
}
