// Space UI screenshot harness. Serves the Space page from a local dev server with every /api call
// answered here, so the UI can be judged and iterated without a backend, a model or a login.
//
//   pnpm dev --port 5174 --strictPort
//   node tests/visual/space-ui-shots.mjs <outDir> [baseUrl]
//
// Nothing here reaches a real service: every /api request is fulfilled by this file, and any request it
// does not know is answered 404 and printed, so a missing mock is visible rather than silent.
import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { chromium } from '@playwright/test'

const OUT = process.argv[2] ?? 'space-ui-shots'
const BASE = process.argv[3] ?? 'http://localhost:5174'
mkdirSync(OUT, { recursive: true })

const ME = { id: 'user_sara', email: 'sara@aquatiq.example', name: 'Sara Nilsen', emailVerified: true }
const SPACE = 'space_room_1'
const NOW = Date.parse('2026-10-28T09:30:00Z')
const iso = (minutesAgo) => new Date(NOW - minutesAgo * 60_000).toISOString()

const membership = (role = 'owner') => ({
  space_ref: SPACE, org_id: 'org_1', subject_id: ME.id, kind: 'shared', role,
  revisions: { authority: 3, membership: 4, privacy: 1, recipient_audience: 2, entitlement: 1 },
})
const space = { space_ref: SPACE, name: 'Innkjøp og leverandører', kind: 'room', lifecycle: 'active', topic: 'Ukentlig oppfølging av innkjøp, avtaler og leveranser', is_organization_room: false }
const context = {
  space, membership: membership(),
  features: { human_posts: true, thread_pane: true, send_idempotency: true, connectivity_banner: true, room_details: true, people_search: true, room_artifacts: true },
  limits: { post_max_chars: 4000 },
}
const threads = [
  { thread_id: 'thr_1', space_id: SPACE, owner_subject_id: ME.id, title: 'Rammeavtale med Nordic Pack', preview: 'Kan noen sjekke prisene mot forrige kvartal før vi svarer?', updated_at: iso(4), latest_run_status: 'completed', reply_count: 6, last_author_subject_id: 'user_ola' },
  { thread_id: 'thr_2', space_id: SPACE, owner_subject_id: 'user_ola', title: 'Leveranseforsinkelse uke 44', preview: 'Verevon har laget et utkast til varsel til kundene.', updated_at: iso(38), latest_run_status: 'running', reply_count: 3, last_author_subject_id: 'user_ola' },
  { thread_id: 'thr_3', space_id: SPACE, owner_subject_id: 'user_kari', title: 'Budsjett 2027: pakkemateriell', preview: 'Tallene er lagt inn i regnearket, se vedlegget.', updated_at: iso(180), latest_run_status: 'completed', reply_count: 11, last_author_subject_id: 'user_kari' },
  { thread_id: 'thr_4', space_id: SPACE, owner_subject_id: ME.id, title: 'Ny leverandør: kvalitetssikring', preview: 'Godkjent av kvalitet i går.', updated_at: iso(60 * 26), latest_run_status: 'completed', reply_count: 2, last_author_subject_id: ME.id },
]
const roster = [
  { subject_type: 'user', subject_id: ME.id, role: 'owner', revision: 1, display_name: 'Sara Nilsen' },
  { subject_type: 'user', subject_id: 'user_ola', role: 'manager', revision: 1, display_name: 'Ola Hansen' },
  { subject_type: 'user', subject_id: 'user_kari', role: 'editor', revision: 1, display_name: 'Kari Berg' },
  { subject_type: 'user', subject_id: 'user_per', role: 'viewer', revision: 1, display_name: 'Per Olsen' },
]
const transcript = {
  threadId: 'thr_1',
  turns: [
    { role: 'user', authorSubjectId: 'user_ola', messageId: 'm1', content: 'Nordic Pack har sendt ny prisliste for 2027. Kan noen sjekke prisene mot forrige kvartal før vi svarer?' },
    { role: 'user', authorSubjectId: 'user_kari', messageId: 'm2', content: 'Jeg tar kartongene. Vi hadde 4,2 % økning sist, så alt over 5 % bør vi diskutere.' },
    { role: 'user', authorSubjectId: ME.id, messageId: 'm3', content: '@Verevon kan du sammenligne prislisten med avtalen fra Q3 og lage en kort oppsummering?' },
    { role: 'assistant', agentName: 'Verevon', messageId: 'm4', content: 'Jeg har sammenlignet de 14 linjene. Tre linjer ligger over 5 %: bølgepapp 3-lags (+6,1 %), stretchfolie (+5,4 %) og pallehjørner (+7,8 %). De øvrige 11 ligger mellom 1,2 % og 4,6 %.\n\nForslag: be om fast pris på pallehjørner i 12 måneder, og godta økningen på folie mot volumrabatt.' },
    { role: 'user', authorSubjectId: 'user_ola', messageId: 'm5', content: 'Bra. Jeg ringer dem i morgen om pallehjørnene.' },
  ],
}
const names = { user_sara: 'Sara Nilsen', user_ola: 'Ola Hansen', user_kari: 'Kari Berg', user_per: 'Per Olsen' }

const ok = (data) => ({ status: 200, contentType: 'application/json', body: JSON.stringify({ data }) })
const raw = (body) => ({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })

const missing = new Set()
function answer(url, method) {
  const p = url.pathname
  if (p.startsWith('/api/v1/auth/session')) return raw({ user: ME })
  if (p === '/api/v1/me') return ok(ME)
  if (p === '/api/v1/session/current' || p === '/api/v1/me/session-context') {
    return ok({ status: 'authenticated', user: ME, org: { id: 'org_1', name: 'Aquatiq', role: 'owner' }, permissions: [], onboardingStatus: 'COMPLETED' })
  }
  if (p === '/api/v1/spaces') return ok({ spaces: [space, { ...space, space_ref: 'space_room_2', name: 'Kvalitet og HMS', topic: null }, { ...space, space_ref: 'space_personal_1', name: 'Personlig', kind: 'personal' }] })
  if (p === `/api/v1/spaces/${SPACE}/context`) return ok(context)
  if (p === `/api/v1/spaces/${SPACE}/threads`) return ok({ space, membership: membership(), threads, read_marker: { last_read_at: NOW - 70 * 60_000 }, shared_read: { available: true } })
  if (p === `/api/v1/spaces/${SPACE}/roster`) return ok({ members: roster, writer_api: true })
  if (p === `/api/v1/spaces/${SPACE}/agents`) return ok({ agents: [{ subject_id: 'agent_verevon', role: 'editor', revision: 1, identity_published: true, binding_ref: 'b1', agent_ref: 'agent_verevon', name: 'Verevon', title: 'Innkjøpsassistent', description: 'Sammenligner priser og lager utkast.', status: 'active', definition_status: 'active', trigger_modes: ['mention'], allowed_tools: [], approval_mode: 'auto', avatar_color: '#d85a30', delivery_targets: [], updated_at: 1 }] })
  if (p.startsWith(`/api/v1/spaces/${SPACE}/threads/`) && p.endsWith('/transcript')) return ok({ transcript: { ...transcript, threadId: p.split('/')[6] } })
  if (p === `/api/v1/spaces/${SPACE}/presence`) return ok({ members: [] })
  if (p === `/api/v1/spaces/${SPACE}/membership`) return ok(membership())
  if (p === `/api/v1/spaces/${SPACE}/instructions`) return ok({ instructions: '' })
  if (p === `/api/v1/spaces/${SPACE}/actions`) return ok({ space_ref: SPACE, actor_type: 'human', catalog: { actions: [] } })
  if (p === `/api/v1/spaces/${SPACE}/artifacts`) return ok({ artifacts: [], next_cursor: null, retention: { zdr: false }, shared_read: { available: true } })
  if (p === `/api/v1/spaces/${SPACE}/work` || p === `/api/v1/spaces/${SPACE}/activity` || p === `/api/v1/spaces/${SPACE}/knowledge`) return ok({ items: [], unavailable: [] })
  if (p === `/api/v1/spaces/${SPACE}/events`) return { status: 200, contentType: 'text/event-stream', body: 'event: connected\ndata: {"mode":"poll_only","v":1}\n\n' }
  if (p.endsWith('/gdpr/deletion/status')) return ok({ status: 'none' })
  if (p.endsWith('/model-entitlement')) return ok({ allowed: true })
  if (p === `/api/v1/spaces/${SPACE}/read`) return ok({ ok: true })
  if (p === '/api/v1/navbar') return ok({ calendar: { events: [] }, notifications: { notifications: [], messages: [], unreadCount: 0 }, plan: 'pro', profile: null, theme: null })
  if (p === '/version.json') return null
  return undefined
}

// INSPECT=<shot name> prints the page's container tree (class, display, overflow, size) so layout rules
// can be written against what the browser computed, not what the stylesheet appears to say.
async function inspect(page) {
  const tree = await page.evaluate(() => {
    const out = []
    const walk = (el, depth) => {
      if (depth > 9) return
      const cs = getComputedStyle(el)
      const r = el.getBoundingClientRect()
      const cls = typeof el.className === 'string' ? el.className.split(' ').filter(Boolean).slice(0, 3).join('.') : ''
      if (cls || depth < 3) out.push(`${'  '.repeat(depth)}${el.tagName.toLowerCase()}${cls ? '.' + cls : ''} [${cs.display}${cs.display.includes('grid') ? ' cols=' + cs.gridTemplateColumns : ''}${cs.overflowY !== 'visible' ? ' ovY=' + cs.overflowY : ''}${cs.position !== 'static' ? ' pos=' + cs.position : ''}] ${Math.round(r.width)}x${Math.round(r.height)}@${Math.round(r.x)},${Math.round(r.y)}`)
      for (const child of el.children) walk(child, depth + 1)
    }
    const root = document.querySelector('.space-page') ?? document.body
    walk(root, 0)
    return out
  })
  console.log(tree.join(String.fromCharCode(10)))
}

const browser = await chromium.launch()
async function shoot(name, { width, height, path = `/spaces/${SPACE}`, after } = {}) {
  const context = await browser.newContext({ viewport: { width, height }, locale: 'nb-NO', colorScheme: 'light' })
  const page = await context.newPage()
  page.on('pageerror', (error) => console.log(`[${name}] pageerror:`, String(error).slice(0, 200)))
  page.on('console', (m) => { if (m.type() === 'error') console.log(`[${name}] console.error:`, m.text().slice(0, 200)) })
  await page.route((url) => url.pathname.startsWith('/api/'), async (route) => {
    const url = new URL(route.request().url())
    const reply = answer(url, route.request().method())
    if (reply) return route.fulfill(reply)
    missing.add(`${route.request().method()} ${url.pathname}`)
    return route.fulfill({ status: 404, contentType: 'application/json', body: JSON.stringify({ error: { code: 'mock_not_found', message: 'not mocked' } }) })
  })
  await page.goto(BASE + path, { waitUntil: 'domcontentloaded' })
  // The first load compiles the page on demand; wait for the room's own heading, then let it settle.
  await page.locator('h1').first().waitFor({ timeout: 30_000 }).catch(() => undefined)
  await page.waitForTimeout(1500)
  if (process.env.INSPECT === name) await inspect(page)
  if (after) await after(page)
  await page.waitForTimeout(600)
  await page.screenshot({ path: join(OUT, `${name}.png`) })
  await context.close()
  console.log('shot', name)
}

await shoot('01-room-desktop', { width: 1440, height: 900 })
await shoot('02-thread-pane-desktop', { width: 1440, height: 900, path: `/spaces/${SPACE}?thread_id=thr_1` })
// The shell opens its sidebar drawer over the room on a first visit at phone width; close it to see the room.
await shoot('03-room-mobile', { width: 390, height: 844, after: async (page) => { await page.mouse.click(31, 25); await page.waitForTimeout(700) } })

await browser.close()
if (missing.size) console.log('UNMOCKED:\n' + [...missing].sort().join('\n'))
