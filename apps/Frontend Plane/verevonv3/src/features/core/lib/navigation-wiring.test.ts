import { describe, expect, it } from 'vitest'

import { getNavbarLabels, routeFromPath } from '@/features/core/lib/shell-data'
import {
  applyDemoModeNavGate,
  getSidebarSectionForPath,
  isSidebarPathActive,
  sidebarSections,
  type SidebarPanelItem,
} from '@/features/core/lib/sidebar-navigation'

/**
 * Guards the link between the sidebar and the route table.
 *
 * The Space surface was routed at `/spaces/:spaceId` and rendered fine, but it
 * appeared nowhere in navigation and `routeFromPath` had no case for it. Two
 * separate silences: nothing linked to the page, and had anything linked, the
 * path would have collapsed onto the `/dashboard` default so the item never
 * highlighted and the navbar named the wrong surface.
 *
 * Neither is a crash, which is why they lasted. These assertions turn both into
 * failures.
 */

function allItems(): SidebarPanelItem[] {
  return sidebarSections.flatMap((section) =>
    section.panelGroups.flatMap((group) => group.items),
  )
}

/** `SidebarHref` allows a query string; routing only ever sees the path. */
function pathOf(href: string): string {
  return href.split('?')[0] ?? href
}

describe('sidebar navigation wiring', () => {
  it('resolves every sidebar destination to a known route', () => {
    // A path routeFromPath does not recognize falls through to '/dashboard'.
    // That is indistinguishable from a real dashboard link, so the item simply
    // never lights up and nobody files a bug.
    const unresolved = allItems()
      .map((item) => ({ id: item.id, path: pathOf(item.href) }))
      .filter(({ path }) => path !== '/dashboard' && routeFromPath(path) === '/dashboard')

    expect(
      unresolved,
      `these sidebar items point at paths routeFromPath does not know, so they collapse onto /dashboard and never highlight: ${JSON.stringify(unresolved)}`,
    ).toEqual([])
  })

  it('keeps sidebar item ids unique', () => {
    // Ids drive the demo-mode gate's denylist and the active-item lookup, so a
    // collision silently hides or mis-highlights one of the pair.
    const ids = allItems().map((item) => item.id)
    expect(ids.length, `duplicate sidebar item id: ${ids.filter((id, i) => ids.indexOf(id) !== i).join(', ')}`)
      .toBe(new Set(ids).size)
  })

  it('resolves Pulse without falling back to Studio canvas', () => {
    expect(routeFromPath('/studio/pulse')).toBe('/engagement/visibility')
    expect(routeFromPath('/studio/pulse/')).toBe('/engagement/visibility')
    expect(routeFromPath('/studio/canvas')).toBe('/studio/canvas')
  })

  it('makes Pulse the Marketing entry point and keeps it discoverable', () => {
    const marketing = applyDemoModeNavGate(sidebarSections).find((section) => section.id === 'engagement')
    expect(marketing?.href).toBe('/engagement')
    expect(marketing?.panelGroups.flatMap((group) => group.items)).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: 'engagement-visibility', label: 'Synlighet', href: '/engagement/visibility' }),
    ]))
    expect(getSidebarSectionForPath('/engagement/visibility', '/engagement/visibility').id).toBe('visibility')
  })

  it('uses the Visibility context for every new analysis page', () => {
    for (const view of ['pages','groups','links','competitors']) {
      const section=getSidebarSectionForPath(`/engagement/visibility/${view}`,routeFromPath(`/engagement/visibility/${view}`))
      expect(section.id).toBe('visibility'); expect(section.parentSectionId).toBe('engagement')
    }
    expect(getSidebarSectionForPath('/engagement','/engagement').id).toBe('engagement')
  })

  it('names the real Marketing Pulse surface in localized navbar breadcrumbs', () => {
    const route = routeFromPath('/studio/pulse')
    expect(getNavbarLabels(route, 'no')).toEqual({ moduleLabel: 'Engasjement', tabLabel: 'Synlighet' })
    expect(getNavbarLabels(route, 'en')).toEqual({ moduleLabel: 'Engagement', tabLabel: 'Visibility' })
  })

  it('opens the Anbud context under the Engagement rail without a Radar shortcut', () => {
    const sections = applyDemoModeNavGate(sidebarSections)
    const engagement = sections.find(section => section.id === 'engagement')!
    const entries = engagement.panelGroups.flatMap(group => group.items)
    expect(entries.some(item => item.href === '/anbud')).toBe(true)
    expect(entries.some(item => item.href === '/anbud/radar')).toBe(false)
    for (const path of ['/anbud', '/anbud/radar']) {
      const context = getSidebarSectionForPath(path, routeFromPath(path), sections)
      expect(context.id).toBe('anbud')
      expect(context.parentSectionId).toBe('engagement')
    }
    expect(sections.filter(section => !section.parentSectionId).some(section => section.id === 'anbud')).toBe(false)
    expect(isSidebarPathActive('/anbud/radar', '/anbud', [], true)).toBe(false)
    expect(getNavbarLabels('/anbud', 'no')).toEqual({moduleLabel:'Engasjement',tabLabel:'Anbud'})
  })

  it('offers a way into the Space surface', () => {
    // The regression this file exists for. `/spaces/:spaceId` cannot be a
    // navigation target on its own — it needs a ref the sidebar does not have —
    // so the resolver route is what navigation must carry.
    const spaceItems = allItems().filter((item) => pathOf(item.href).startsWith('/spaces'))

    expect(spaceItems.length, 'no sidebar item reaches the Space surface').toBeGreaterThan(0)
    expect(routeFromPath('/spaces/space_personal_1')).toBe('/spaces')
  })

  it('gives Rom its own rail section rather than only a Hjem shortcut', () => {
    // The Hjem panel is a customizable shortcut list, so an entry there is not a
    // home — remove the shortcut and the surface becomes unreachable again. The
    // rail section is what makes it permanent, and it must survive the
    // demo-mode gate to actually render.
    const rail = applyDemoModeNavGate(sidebarSections)
    const spaces = rail.find((section) => section.href === '/spaces')

    expect(spaces, 'no top-level sidebar section reaches /spaces').toBeDefined()
  })

  it('keeps the Rom section active while a specific room is open', () => {
    // Section lookup matches on section.href, and the Hjem section owns
    // /dashboard — so a room must resolve to Rom, not fall back to Hjem and
    // show the wrong panel next to an open room.
    const section = getSidebarSectionForPath('/spaces/space_personal_1', '/spaces')
    expect(section.href).toBe('/spaces')
  })
})
