// @vitest-environment jsdom

import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ActionCard } from '@/features/agents/components/ChatbotStudioCards'
import { __resetOrgAgentPolicyAvailabilityForTests } from '@/features/customize/AgentModesSection'
import { I18nProvider } from '@/shared/i18n'
import { Zap } from '@/shared/icons'

function stubPolicyApi(status: number) {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(
    JSON.stringify(status === 200 ? { org_id: 'org_1', revision: 0, digest: '', settings: {} } : { error: { code: 'not_found', message: 'off' } }),
    { headers: { 'Content-Type': 'application/json' }, status },
  )))
}

const card = () => (
  <I18nProvider>
    <ActionCard Icon={Zap} color="" enabled subtitle="Calls the shell" title="Shell" />
  </I18nProvider>
)

beforeEach(() => {
  window.localStorage.setItem('verevon.locale', 'en')
  __resetOrgAgentPolicyAvailabilityForTests()
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('ActionCard Customize', () => {
  it('opens the Customize hub once the org agent policy API is on', async () => {
    stubPolicyApi(200)

    render(card)

    const link = await screen.findByRole('link', { name: 'Customize' })
    expect(link.getAttribute('href')).toBe('/agents/customize')
  })

  it('stays a disabled placeholder while the org agent policy API is off', async () => {
    stubPolicyApi(404)

    render(card)

    expect(await screen.findByRole('button', { name: 'Customize' })).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Customize' })).toBeNull()
    expect((screen.getByRole('button', { name: 'Customize' }) as HTMLButtonElement).disabled).toBe(true)
  })
})
