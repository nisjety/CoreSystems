import { describe, expect, it } from 'vitest'
import { MODEL_ACCESS_NAV_ITEM_ID, applyDemoModeNavGate, sidebarSections } from './sidebar-navigation'

const itemIds = (sections = applyDemoModeNavGate(sidebarSections)) =>
  sections.flatMap((section) => section.panelGroups.flatMap((group) => group.items.map((item) => item.id)))

// The Model access tab follows the gateway's admin switch, never a build flag:
// the nav gate hides it unless the caller says the switch is on.
describe('the Model access settings tab', () => {
  it('is defined in the settings panel', () => {
    expect(itemIds(sidebarSections)).toContain(MODEL_ACCESS_NAV_ITEM_ID)
  })

  it('is hidden by default and when the gateway reports the switch off', () => {
    expect(itemIds(applyDemoModeNavGate(sidebarSections))).not.toContain(MODEL_ACCESS_NAV_ITEM_ID)
    expect(itemIds(applyDemoModeNavGate(sidebarSections, { modelAccessEnabled: false }))).not.toContain(MODEL_ACCESS_NAV_ITEM_ID)
  })

  it('is shown when the gateway reports the switch on, beside the other settings', () => {
    const shown = itemIds(applyDemoModeNavGate(sidebarSections, { modelAccessEnabled: true }))
    expect(shown).toContain(MODEL_ACCESS_NAV_ITEM_ID)
    expect(shown).toContain('settings-mcp')
  })
})
