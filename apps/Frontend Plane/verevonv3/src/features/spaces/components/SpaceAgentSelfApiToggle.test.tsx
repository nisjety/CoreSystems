import { cleanup, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { I18nProvider, localeStorageKey } from '@/shared/i18n'
import { fireEvent } from '@/test/fire-event'
import { render } from '@/test/render'

// WS18-S6 and W3-DEC-CARD: the owner's and managers' switch for one bound agent's own-thread tools (rename, pin, post
// cards). The write goes through the action registry, so the test mocks `executeAction` and asserts the one call that
// carries the change. The Norwegian copy is the default; the English label is read through an I18nProvider with the
// locale stored first.

const actions = vi.hoisted(() => ({ executeAction: vi.fn() }))
vi.mock('@/shared/actions/action-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/actions/action-client')>()),
  ...actions,
}))

const { ApiError } = await import('@/shared/api/api-error')
const { SpaceAgentSelfApiToggle } = await import('./SpaceAgentSelfApiToggle')

const ROOM = 'room-1'
const BINDING = 'bind-1'
const ACTOR = { type: 'human', userId: 'user-kari', orgId: 'org-1' }

/** The gateway's `data` for `spaces.set_agent_self_api`, as the action returns it. */
const confirmed = (over: Record<string, unknown> = {}) => ({
  result: { binding_ref: BINDING, self_api_enabled: true, projection_version: 7, changed: true, ...over },
})

function renderToggle(
  options: { readonly enabled?: boolean; readonly orgId?: string; readonly actorId?: string; readonly onChanged?: () => void } = {},
) {
  const onChanged = options.onChanged ?? vi.fn()
  render(() => (
    <SpaceAgentSelfApiToggle
      spaceRef={ROOM}
      bindingRef={BINDING}
      enabled={options.enabled ?? false}
      orgId={options.orgId ?? 'org-1'}
      actorId={options.actorId ?? 'user-kari'}
      onChanged={onChanged}
    />
  ))
  return { onChanged }
}

function renderToggleInEnglish() {
  window.localStorage.setItem(localeStorageKey, 'en')
  render(() => (
    <I18nProvider>
      <SpaceAgentSelfApiToggle spaceRef={ROOM} bindingRef={BINDING} enabled={false} orgId="org-1" actorId="user-kari" />
    </I18nProvider>
  ))
}

const box = () => screen.getByRole('checkbox') as HTMLInputElement

beforeEach(() => {
  actions.executeAction.mockReset()
})

afterEach(() => cleanup())

describe('SpaceAgentSelfApiToggle (WS18-S6, W3-DEC-CARD)', () => {
  it('is unchecked while the agent list says the switch is off', () => {
    renderToggle({ enabled: false })
    expect(box().checked).toBe(false)
    expect(screen.getByText('La agenten organisere tråder: gi nytt navn, fest og legg inn kort')).toBeTruthy()
  })

  it('sends the change through spaces.set_agent_self_api with the room, the binding and the new value, then says so', async () => {
    actions.executeAction.mockResolvedValue(confirmed())
    const { onChanged } = renderToggle({ enabled: false })

    fireEvent.click(box())

    expect(await screen.findByText('Lagret.')).toBeTruthy()
    expect(actions.executeAction).toHaveBeenCalledTimes(1)
    expect(actions.executeAction).toHaveBeenCalledWith('spaces.set_agent_self_api', ACTOR, {
      spaceRef: ROOM,
      bindingRef: BINDING,
      enabled: true,
    })
    expect(onChanged).toHaveBeenCalledOnce()
    expect(box().checked).toBe(true)
  })

  it('a refusal puts the checkbox back and says why in the switch’s own words', async () => {
    actions.executeAction.mockRejectedValue(new ApiError('forbidden', 403, 'space_role_cannot_manage_agent'))
    const { onChanged } = renderToggle({ enabled: false })

    fireEvent.click(box())

    expect((await screen.findByRole('alert')).textContent).toBe(
      'Bare en eier eller leder kan endre om agenten kan handle i sin egen tråd. Ingenting er endret.',
    )
    expect(box().checked).toBe(false)
    expect(onChanged).not.toHaveBeenCalled()
  })

  it('a refusal for an inactive binding reaches its shared sentence', async () => {
    actions.executeAction.mockRejectedValue(new ApiError('paused', 409, 'agent_binding_not_active'))
    renderToggle({ enabled: true })

    fireEvent.click(box())

    expect((await screen.findByRole('alert')).textContent).toBe(
      'Agenten er ikke aktiv i rommet akkurat nå, så innstillingen kan ikke endres. Ingenting er endret.',
    )
    expect(box().checked).toBe(true)
  })

  it('sends nothing without the signed-in organization and member', () => {
    renderToggle({ enabled: false, actorId: '' })
    fireEvent.click(box())
    expect(actions.executeAction).not.toHaveBeenCalled()
  })

  it('the list offers rename, pin and post cards, in that order, with no canvas action', () => {
    renderToggle({ enabled: false })
    fireEvent.click(screen.getByRole('button', { name: 'Hva kan agenten gjøre?' }))

    const list = screen.getByRole('list', { name: 'Handlinger agenten kan utføre' })
    const rows = [...list.querySelectorAll('li')].map((row) => row.textContent ?? '')
    expect(rows).toHaveLength(3)
    expect(rows[0]).toContain('Gi tråden nytt navn')
    expect(rows[0]).toContain('Inntil 2 ganger per kjøring.')
    expect(rows[1]).toContain('Fest eller løsne tråden')
    expect(rows[2]).toContain('Legg inn et kort i tråden')
    expect(rows[2]).toContain('Inntil 3 ganger per kjøring.')
    expect(list.textContent).not.toMatch(/lerret|canvas/i)
  })

  it('in English the label reads rename, pin and post cards, and names no canvas tool', () => {
    renderToggleInEnglish()
    expect(screen.getByText('Let this agent organise threads: rename, pin and post cards')).toBeTruthy()
    expect(document.body.textContent ?? '').not.toMatch(/canvas/i)

    fireEvent.click(screen.getByRole('button', { name: 'What can the agent do?' }))
    const rows = [...screen.getByRole('list', { name: 'Actions the agent can take' }).querySelectorAll('li')].map(
      (row) => row.textContent ?? '',
    )
    expect(rows.map((row) => row.split(' ').slice(0, 3).join(' '))).toEqual([
      'Rename the thread',
      'Pin or unpin',
      'Post a card',
    ])
    expect(document.body.textContent ?? '').not.toMatch(/canvas/i)
  })
})
