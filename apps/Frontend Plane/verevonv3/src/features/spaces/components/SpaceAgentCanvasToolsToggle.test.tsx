import { cleanup, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { fireEvent } from '@/test/fire-event'
import { render } from '@/test/render'

// W3-DEC-CANVAS (D2): the owner's and managers' switch for one bound agent's room canvas tools. The write goes through the
// action registry, so the test mocks `executeAction` and asserts the one call that carries the change. The copy is the
// Norwegian default, as in the other space component tests (no I18nProvider wraps these renders).

const actions = vi.hoisted(() => ({ executeAction: vi.fn() }))
vi.mock('@/shared/actions/action-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/actions/action-client')>()),
  ...actions,
}))

const { ApiError } = await import('@/shared/api/api-error')
const { SpaceAgentCanvasToolsToggle } = await import('./SpaceAgentCanvasToolsToggle')

const ROOM = 'room-1'
const BINDING = 'bind-1'
const ACTOR = { type: 'human', userId: 'user-kari', orgId: 'org-1' }

/** The gateway's `data` for `spaces.set_agent_canvas_tools`, as the action returns it. */
const confirmed = (over: Record<string, unknown> = {}) => ({
  result: { binding_ref: BINDING, canvas_tools_enabled: true, projection_version: 4, changed: true, ...over },
})

function renderToggle(
  options: { readonly enabled?: boolean; readonly orgId?: string; readonly actorId?: string; readonly onChanged?: () => void } = {},
) {
  const onChanged = options.onChanged ?? vi.fn()
  render(() => (
    <SpaceAgentCanvasToolsToggle
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

const box = () => screen.getByRole('checkbox') as HTMLInputElement

beforeEach(() => {
  actions.executeAction.mockReset()
})

afterEach(() => cleanup())

describe('SpaceAgentCanvasToolsToggle (W3-DEC-CANVAS)', () => {
  it('is unchecked while the agent list says the switch is off, and says what it grants', () => {
    renderToggle({ enabled: false })
    expect(box().checked).toBe(false)
    expect(screen.getByText('La agenten bruke rommets lerret: lese det og lagre endringer')).toBeTruthy()
    expect(screen.getByText(/Av som standard/)).toBeTruthy()
  })

  it('sends the change through spaces.set_agent_canvas_tools with the room, the binding and the new value, then says so', async () => {
    actions.executeAction.mockResolvedValue(confirmed())
    const { onChanged } = renderToggle({ enabled: false })

    fireEvent.click(box())

    expect(await screen.findByText('Lagret.')).toBeTruthy()
    expect(actions.executeAction).toHaveBeenCalledTimes(1)
    expect(actions.executeAction).toHaveBeenCalledWith('spaces.set_agent_canvas_tools', ACTOR, {
      spaceRef: ROOM,
      bindingRef: BINDING,
      enabled: true,
    })
    expect(onChanged).toHaveBeenCalledOnce()
    expect(box().checked).toBe(true)
  })

  it('sends a switch off as enabled false', async () => {
    actions.executeAction.mockResolvedValue(confirmed({ canvas_tools_enabled: false }))
    renderToggle({ enabled: true })
    expect(box().checked).toBe(true)

    fireEvent.click(box())

    expect(await screen.findByText('Lagret.')).toBeTruthy()
    expect(actions.executeAction).toHaveBeenCalledWith('spaces.set_agent_canvas_tools', ACTOR, {
      spaceRef: ROOM,
      bindingRef: BINDING,
      enabled: false,
    })
    expect(box().checked).toBe(false)
  })

  // The agent list is read again after a change. When it states the other value (another owner turned the switch off), the
  // switch shows that value, not the one this card confirmed earlier.
  it('after the agent list is read again it shows the value that list states, not an earlier confirmed change', async () => {
    actions.executeAction.mockResolvedValue(confirmed())
    const [enabled, setEnabled] = createSignal(false)
    render(() => (
      <SpaceAgentCanvasToolsToggle spaceRef={ROOM} bindingRef={BINDING} enabled={enabled()} orgId="org-1" actorId="user-kari" />
    ))
    fireEvent.click(box())
    expect(await screen.findByText('Lagret.')).toBeTruthy()
    expect(box().checked).toBe(true)

    setEnabled(true)
    flush()
    setEnabled(false)
    flush()

    expect(box().checked).toBe(false)
  })

  it('a refusal puts the box back to the value the gateway last confirmed, and says why in its own words', async () => {
    actions.executeAction.mockRejectedValue(new ApiError('forbidden', 403, 'space_role_cannot_manage_agent'))
    const { onChanged } = renderToggle({ enabled: false })

    fireEvent.click(box())

    expect((await screen.findByRole('alert')).textContent).toBe(
      'Bare en eier eller leder kan endre om agenten kan bruke rommets lerret. Ingenting er endret.',
    )
    expect(box().checked).toBe(false)
    expect(onChanged).not.toHaveBeenCalled()
  })

  it('a refusal with another typed code reaches its shared sentence, not the wake-policy words', async () => {
    actions.executeAction.mockRejectedValue(new ApiError('paused', 409, 'agent_binding_not_active'))
    renderToggle({ enabled: false })

    fireEvent.click(box())

    expect((await screen.findByRole('alert')).textContent).toBe(
      'Agenten er ikke aktiv i rommet akkurat nå, så innstillingen kan ikke endres. Ingenting er endret.',
    )
    expect(box().checked).toBe(false)
  })

  it('a reply the contract does not describe is an error, and nothing is shown as saved', async () => {
    actions.executeAction.mockResolvedValue({ result: { binding_ref: BINDING } })
    const { onChanged } = renderToggle({ enabled: false })

    fireEvent.click(box())

    expect((await screen.findByRole('alert')).textContent).toBe(
      'Innstillingen for lerretet for agenten kunne ikke endres. Ingenting er endret.',
    )
    expect(screen.queryByText('Lagret.')).toBeNull()
    expect(onChanged).not.toHaveBeenCalled()
  })

  it('sends nothing without the signed-in organization and member', () => {
    renderToggle({ enabled: false, orgId: '' })
    fireEvent.click(box())
    expect(actions.executeAction).not.toHaveBeenCalled()
  })

  it('the list names reading and saving the canvas, and never a rename', () => {
    renderToggle({ enabled: false })
    const toggle = screen.getByRole('button', { name: 'Hva kan agenten gjøre?' })
    expect(toggle.getAttribute('aria-expanded')).toBe('false')

    fireEvent.click(toggle)

    expect(screen.getByRole('button', { name: 'Skjul' }).getAttribute('aria-expanded')).toBe('true')
    const list = screen.getByRole('list', { name: 'Lerretshandlinger agenten kan utføre' })
    expect(list.textContent).toContain('Les rommets lerret')
    expect(list.textContent).toContain('Lagre en ny versjon av lerretet')
    expect(list.textContent).toContain('Inntil 10 ganger per kjøring.')
    expect(list.textContent).toContain('Inntil 20 ganger per kjøring.')
    expect(list.textContent).not.toMatch(/navn|rename/i)
  })
})
