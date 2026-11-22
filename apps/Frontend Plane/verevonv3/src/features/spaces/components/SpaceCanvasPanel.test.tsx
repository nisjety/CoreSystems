import { createRouter, useNavigate } from '@solidjs/router'
import { cleanup, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceCanvas, SpaceCanvasRead, SpaceCanvasVersion, SpaceRosterMember } from '@/shared/api/spaces-client'
import { fireEvent } from '@/test/fire-event'
import { render } from '@/test/render'

import { CANVAS_REFRESH_MS } from '../lib/space-canvas'
import type { SpaceTabId } from '../lib/space-tab-registry'
import { ActiveSpaceTabContext } from './SpaceCockpit'

// WS19-S5, WS19-S6 and WS19-S7: the room's Canvas tab. The reads are mocked at the spaces client and the writes at the action
// client, so every assertion is about a call the panel made (its arguments and its order) or a line the reader sees. The
// panel reads the router (a move out of the room asks first), so each render is mounted under a router. Copy is the
// Norwegian default, as in the other space component tests.

const spacesClient = vi.hoisted(() => ({
  getSpaceCanvas: vi.fn(),
  getSpaceCanvasVersion: vi.fn(),
  listSpaceCanvasVersions: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const actions = vi.hoisted(() => ({ executeAction: vi.fn() }))
vi.mock('@/shared/actions/action-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/actions/action-client')>()),
  ...actions,
}))

const { ApiError } = await import('@/shared/api/api-error')
const { SpaceCanvasPanel } = await import('./SpaceCanvasPanel')

const ACTOR = { type: 'human', userId: 'user-kari', orgId: 'org-1' }
const ROSTER: SpaceRosterMember[] = [
  { subject_type: 'user', subject_id: 'user-ola', role: 'editor', revision: 1, display_name: 'Ola' },
  { subject_type: 'user', subject_id: 'user-kari', role: 'owner', revision: 1, display_name: 'Kari' },
]
const CREATED = (no: number) => `2026-11-18T10:0${no}:00Z`

/** One version as the history and the head carry it. `content` is set only where the gateway sends the text. */
function version(no: number, over: Partial<SpaceCanvasVersion> = {}): SpaceCanvasVersion {
  return {
    versionId: `v${no}`,
    versionNo: no,
    parentVersionId: no > 1 ? `v${no - 1}` : null,
    authorKind: 'user',
    authorId: 'user-ola',
    onBehalfOf: null,
    runId: null,
    changeKind: no === 1 ? 'create' : 'edit',
    restoredFromVersionId: null,
    summary: null,
    contentBytes: 0,
    contentSha256: `sha-${no}`,
    createdAt: CREATED(no),
    ...over,
  }
}

const canvasOf = (headNo: number): SpaceCanvas => ({
  canvasId: 'canvas-1',
  spaceRef: 'room-1',
  state: 'active',
  headVersionId: `v${headNo}`,
  headVersionNo: headNo,
  createdAt: CREATED(1),
  updatedAt: CREATED(headNo),
})

/** A read that shows head `no` with its text, as the gateway answers it. */
function viewRead(
  no: number,
  content: string,
  options: { readonly canWrite?: boolean; readonly sha?: string; readonly authorId?: string } = {},
): SpaceCanvasRead {
  return {
    unchanged: false,
    canvas: canvasOf(no),
    head: {
      ...version(no, { contentSha256: options.sha ?? `sha-${no}`, authorId: options.authorId ?? 'user-ola' }),
      content,
    },
    canWrite: options.canWrite ?? true,
  }
}

const emptyRead = (canWrite = true): SpaceCanvasRead => ({ unchanged: false, canvas: null, head: null, canWrite })

/** The gateway's `data` for a save (201), as the `spaces.save_canvas` action returns it. */
const stored = (no: number) => ({
  result: {
    canvas_id: 'canvas-1',
    version: {
      version_id: `v${no}`,
      version_no: no,
      parent_version_id: no > 1 ? `v${no - 1}` : null,
      author_kind: 'user',
      author_id: 'user-kari',
      on_behalf_of: null,
      run_id: null,
      change_kind: no === 1 ? 'create' : 'edit',
      restored_from_version_id: null,
      summary: null,
      content_bytes: 0,
      content_sha256: `sha-${no}`,
      created_at: CREATED(no),
    },
    deduplicated: false,
  },
})

/** The gateway's `data` for a restore (201): a new head that copies the version `from`. */
const restored = (no: number, from: number) => ({
  result: {
    canvas_id: 'canvas-1',
    version: {
      version_id: `v${no}`,
      version_no: no,
      parent_version_id: `v${no - 1}`,
      author_kind: 'user',
      author_id: 'user-kari',
      on_behalf_of: null,
      run_id: null,
      change_kind: 'restore',
      restored_from_version_id: `v${from}`,
      summary: null,
      content_bytes: 0,
      content_sha256: `sha-${from}`,
      created_at: CREATED(no),
    },
    deduplicated: false,
  },
})

// The router hands the panel its navigate function, so a test can move the page to another room.
let navigateTo: ((to: string) => void) | undefined
function NavigationProbe() {
  const navigate = useNavigate()
  navigateTo = (to) => {
    void navigate(to)
  }
  return null
}

function renderPanel(options: { readonly tab?: SpaceTabId; readonly canClear?: boolean } = {}) {
  const tab = options.tab ?? 'lerret'
  const Router = createRouter({
    routes: [
      {
        path: '/*all',
        component: () => (
          <>
            <NavigationProbe />
            <ActiveSpaceTabContext value={() => tab}>
              <SpaceCanvasPanel
                spaceRef="room-1"
                roster={() => ROSTER}
                orgId="org-1"
                actorId="user-kari"
                canClear={options.canClear}
              />
            </ActiveSpaceTabContext>
          </>
        ),
      },
    ],
  })
  render(() => <Router />)
}

const textarea = () => screen.getByRole('textbox') as HTMLTextAreaElement

const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0))
async function settle() {
  for (let round = 0; round < 4; round += 1) {
    await tick()
    flush()
  }
}

/** Makes the document visible (or hidden) for the refresh, which reads only while the page is visible. */
function setDocumentVisible(visible: boolean) {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => (visible ? 'visible' : 'hidden') })
}
const announceVisibilityChange = () => document.dispatchEvent(new Event('visibilitychange'))

beforeEach(() => {
  spacesClient.getSpaceCanvas.mockReset()
  spacesClient.getSpaceCanvas.mockResolvedValue(emptyRead(false))
  spacesClient.getSpaceCanvasVersion.mockReset()
  spacesClient.listSpaceCanvasVersions.mockReset()
  spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [], nextCursor: null })
  actions.executeAction.mockReset()
  actions.executeAction.mockRejectedValue(new Error('no answer scripted for this call'))
  setDocumentVisible(true)
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
  delete (document as unknown as { visibilityState?: string }).visibilityState
  navigateTo = undefined
})

describe('SpaceCanvasPanel: reading the canvas (WS19-S5)', () => {
  it('makes no request while the Canvas tab is not the active one', async () => {
    renderPanel({ tab: 'chat' })
    await settle()
    expect(spacesClient.getSpaceCanvas).not.toHaveBeenCalled()
  })

  it('shows loading, then the empty state for a Space with no canvas', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(emptyRead(false))
    renderPanel()
    expect(screen.getByText('Laster lerretet …')).toBeTruthy()
    expect(await screen.findByText('Rommet har ikke et lerret ennå.')).toBeTruthy()
    expect(screen.queryByText('Laster lerretet …')).toBeNull()
    expect(spacesClient.getSpaceCanvas).toHaveBeenCalledWith('room-1', null)
  })

  it('shows the head as Markdown with the line that says who saved it and which version it is', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(3, '# Mål\n\nLevér torsdag.'))
    renderPanel()
    expect(await screen.findByText('Levér torsdag.')).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Mål' })).toBeTruthy()
    expect(screen.getByText(/Versjon 3, lagret av Ola/)).toBeTruthy()
  })

  it('a canvas read refused as not authorized says the canvas sentence, not a Space denial', async () => {
    spacesClient.getSpaceCanvas.mockRejectedValue(
      new ApiError('denied', 403, 'canvas_read_not_authorized', { details: { reason: 'space_entitlement_off' } }),
    )
    renderPanel()
    expect((await screen.findByRole('alert')).textContent).toContain(
      'Lerret er ikke slått på for denne organisasjonen, eller du kan ikke lese det.',
    )
    expect(screen.getByRole('button', { name: 'Prøv igjen' })).toBeTruthy()
  })

  it('a zero-retention refusal of the canvas says the canvas ZDR sentence', async () => {
    spacesClient.getSpaceCanvas.mockRejectedValue(
      new ApiError('zdr', 412, 'zdr_durable_mutation_forbidden', { details: { surface: 'room_canvas' } }),
    )
    renderPanel()
    const alert = await screen.findByRole('alert')
    expect(alert.querySelector('p')?.textContent).toBe('Lerret er ikke tilgjengelig når Zero Data Retention er slått på.')
    expect(screen.getByRole('button', { name: 'Prøv igjen' })).toBeTruthy()
  })

  it('a refresh that fails with 503 keeps the view on screen', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(3, 'Levér torsdag.'))
      .mockRejectedValueOnce(new ApiError('down', 503, 'canvas_unavailable'))
    renderPanel()
    expect(await screen.findByText('Levér torsdag.')).toBeTruthy()

    announceVisibilityChange()
    await settle()

    expect(spacesClient.getSpaceCanvas).toHaveBeenCalledTimes(2)
    expect(screen.getByText('Levér torsdag.')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('an unchanged refresh keeps the view and asks with the head the page holds', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(3, 'Levér torsdag.'))
      .mockResolvedValueOnce({ unchanged: true, headVersionId: 'v3' })
    renderPanel()
    expect(await screen.findByText('Levér torsdag.')).toBeTruthy()

    announceVisibilityChange()
    await settle()

    expect(spacesClient.getSpaceCanvas).toHaveBeenLastCalledWith('room-1', 'v3')
    expect(screen.getByText('Levér torsdag.')).toBeTruthy()
  })

  it('a moved head replaces the view with the new head', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(3, 'Levér torsdag.'))
      .mockResolvedValueOnce(viewRead(4, 'Ny tekst fra Ola.'))
    renderPanel()
    expect(await screen.findByText('Levér torsdag.')).toBeTruthy()

    announceVisibilityChange()

    expect(await screen.findByText('Ny tekst fra Ola.')).toBeTruthy()
    expect(screen.getByText(/Versjon 4, lagret av Ola/)).toBeTruthy()
    expect(screen.queryByText('Levér torsdag.')).toBeNull()
  })

  it('Try again reads the canvas again after a failed read', async () => {
    spacesClient.getSpaceCanvas
      .mockRejectedValueOnce(new ApiError('down', 503, 'canvas_unavailable'))
      .mockResolvedValueOnce(viewRead(3, 'Tekst etter nytt forsøk.'))
    renderPanel()
    expect((await screen.findByRole('alert')).textContent).toContain('Lerretet er midlertidig utilgjengelig')

    fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))

    expect(await screen.findByText('Tekst etter nytt forsøk.')).toBeTruthy()
    expect(spacesClient.getSpaceCanvas).toHaveBeenCalledTimes(2)
  })

  it('the refresh interval and the visibility listener are removed when the panel leaves the page', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(1, 'Plan A'))
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
    renderPanel()
    await settle()
    expect(spacesClient.getSpaceCanvas).toHaveBeenCalledTimes(1)

    vi.advanceTimersByTime(CANVAS_REFRESH_MS)
    await settle()
    expect(spacesClient.getSpaceCanvas).toHaveBeenCalledTimes(2)

    cleanup()
    vi.advanceTimersByTime(CANVAS_REFRESH_MS * 3)
    announceVisibilityChange()
    await settle()
    expect(spacesClient.getSpaceCanvas).toHaveBeenCalledTimes(2)
  })
})

describe('SpaceCanvasPanel: creating and saving (WS19-S6)', () => {
  it('a viewer sees neither Edit on a view nor Create canvas on an empty room', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValueOnce(viewRead(1, 'Plan A', { canWrite: false }))
    renderPanel()
    expect(await screen.findByRole('button', { name: 'Historikk' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Rediger' })).toBeNull()
    cleanup()

    spacesClient.getSpaceCanvas.mockResolvedValueOnce(emptyRead(false))
    renderPanel()
    expect(await screen.findByText('Rommet har ikke et lerret ennå.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Opprett lerret' })).toBeNull()
  })

  it('an editor sees Create canvas on an empty room, and Edit on a view', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValueOnce(viewRead(1, 'Plan A', { canWrite: true }))
    renderPanel()
    expect(await screen.findByRole('button', { name: 'Rediger' })).toBeTruthy()
    cleanup()

    spacesClient.getSpaceCanvas.mockResolvedValueOnce(emptyRead(true))
    renderPanel()
    expect(await screen.findByRole('button', { name: 'Opprett lerret' })).toBeTruthy()
  })

  it('Save is disabled on an empty draft, with the hint, and is enabled once there is text', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(emptyRead(true))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Opprett lerret' }))

    const save = screen.getByRole('button', { name: 'Lagre' }) as HTMLButtonElement
    expect(save.disabled).toBe(true)
    expect(screen.getByText('Skriv minst ett tegn for å lagre.')).toBeTruthy()

    fireEvent.input(textarea(), { target: { value: 'x' } })
    expect(save.disabled).toBe(false)
    expect(screen.queryByText('Skriv minst ett tegn for å lagre.')).toBeNull()
  })

  it('a save that is stored shows Saved as version N and the new head, and sends one spaces.save_canvas', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValueOnce(emptyRead(true)).mockResolvedValue(viewRead(1, '# Plan'))
    actions.executeAction.mockResolvedValue(stored(1))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Opprett lerret' }))
    fireEvent.input(textarea(), { target: { value: '# Plan' } })

    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

    expect(await screen.findByText('Lagret som versjon 1.')).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Plan' })).toBeTruthy()
    expect(actions.executeAction).toHaveBeenCalledTimes(1)
    expect(actions.executeAction).toHaveBeenCalledWith(
      'spaces.save_canvas',
      ACTOR,
      { spaceRef: 'room-1', expectedVersionId: null, content: '# Plan', clientSaveId: expect.any(String) },
      { idempotencyKey: expect.any(String) },
    )
  })

  it('a save refused as a moved head shows the latest text in the conflict banner, and Save my text writes it over the latest with a new save id', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(1, 'Plan A'))
      .mockResolvedValueOnce(viewRead(2, 'Ola skrev dette.'))
      .mockResolvedValue(viewRead(3, 'Plan A og B'))
    actions.executeAction
      .mockRejectedValueOnce(new ApiError('moved', 409, 'canvas_head_moved'))
      .mockResolvedValueOnce(stored(3))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Rediger' }))
    fireEvent.input(textarea(), { target: { value: 'Plan A og B' } })

    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

    expect(await screen.findByText('Noen andre har lagret en nyere versjon. Teksten din er fortsatt her.')).toBeTruthy()
    expect(screen.getByText(/Versjon 2, lagret av Ola/)).toBeTruthy()
    expect(textarea().value).toBe('Plan A og B')

    fireEvent.click(screen.getByRole('button', { name: 'Lagre min tekst som ny versjon' }))

    expect(await screen.findByText('Lagret som versjon 3.')).toBeTruthy()
    const [first, second] = actions.executeAction.mock.calls
    expect(first?.[2]).toMatchObject({ expectedVersionId: 'v1', content: 'Plan A og B' })
    expect(second?.[2]).toMatchObject({ expectedVersionId: 'v2', content: 'Plan A og B' })
    expect(second?.[2].clientSaveId).not.toBe(first?.[2].clientSaveId)
  })

  it('a create whose retry is refused as a digest mismatch, with the head equal to the draft, closes as saved and is never a failure', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValueOnce(emptyRead(true)).mockResolvedValue(viewRead(1, '# Mål'))
    actions.executeAction.mockRejectedValue(new ApiError('digest', 403, 'space_decision_digest_mismatch'))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Opprett lerret' }))
    fireEvent.input(textarea(), { target: { value: '# Mål' } })

    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

    expect(await screen.findByText('Lagret som versjon 1.')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.queryByRole('textbox')).toBeNull()
  })

  it('the same digest mismatch with a different head shows the conflict banner and keeps the draft', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(emptyRead(true))
      .mockResolvedValue(viewRead(1, 'Ola skrev først.'))
    actions.executeAction.mockRejectedValue(new ApiError('digest', 403, 'space_decision_digest_mismatch'))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Opprett lerret' }))
    fireEvent.input(textarea(), { target: { value: '# Mål' } })

    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

    expect(await screen.findByText('Noen andre har lagret en nyere versjon. Teksten din er fortsatt her.')).toBeTruthy()
    expect(textarea().value).toBe('# Mål')
  })

  it('a save that failed on a 503 keeps the draft, and the retry sends the same save id', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(emptyRead(true))
    actions.executeAction
      .mockRejectedValueOnce(new ApiError('down', 503, 'canvas_unavailable'))
      .mockResolvedValueOnce(stored(1))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Opprett lerret' }))
    fireEvent.input(textarea(), { target: { value: 'Plan' } })
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

    expect(await screen.findByText('Lerretet er midlertidig utilgjengelig. Ingenting er endret. Prøv igjen om litt.')).toBeTruthy()
    expect(textarea().value).toBe('Plan')

    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

    expect(await screen.findByText('Lagret som versjon 1.')).toBeTruthy()
    const [first, second] = actions.executeAction.mock.calls
    expect(typeof first?.[3].idempotencyKey).toBe('string')
    expect(second?.[3]).toEqual({ idempotencyKey: first?.[3].idempotencyKey })
  })

  it('a save refused because the canvas was removed keeps the draft, says so, and the next save is a create', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(1, 'Plan A'))
    actions.executeAction
      .mockRejectedValueOnce(new ApiError('gone', 404, 'canvas_not_found'))
      .mockResolvedValueOnce(stored(1))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Rediger' }))
    fireEvent.input(textarea(), { target: { value: 'Plan B' } })
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

    expect(await screen.findByText(/Lerretet ble fjernet mens du redigerte/)).toBeTruthy()
    expect(textarea().value).toBe('Plan B')
    // Not a conflict: no banner, and the draft is no longer written over the removed version.
    expect(screen.queryByRole('button', { name: 'Lagre min tekst som ny versjon' })).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    expect(await screen.findByText('Lagret som versjon 1.')).toBeTruthy()
    const [first, second] = actions.executeAction.mock.calls
    expect(first?.[2]).toMatchObject({ expectedVersionId: 'v1' })
    expect(second?.[2]).toMatchObject({ expectedVersionId: null })
    expect(second?.[3].idempotencyKey).not.toBe(first?.[3].idempotencyKey)
  })

  it('a quiet refresh during an edit keeps the draft and says a newer version was saved', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(1, 'Plan A'))
      .mockResolvedValueOnce(viewRead(2, 'Ola skrev dette.'))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Rediger' }))
    fireEvent.input(textarea(), { target: { value: 'Plan A, min redigering' } })

    announceVisibilityChange()

    expect(await screen.findByText('En nyere versjon er lagret mens du redigerer.')).toBeTruthy()
    expect(textarea().value).toBe('Plan A, min redigering')
    expect(screen.getByRole('button', { name: 'Vis nyeste' })).toBeTruthy()
  })

  it('Load latest and discard drops the draft and shows the latest version', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(1, 'Plan A'))
      .mockResolvedValueOnce(viewRead(2, 'Ola skrev dette.'))
      .mockResolvedValue(viewRead(2, 'Ola skrev dette.'))
    actions.executeAction.mockRejectedValueOnce(new ApiError('moved', 409, 'canvas_head_moved'))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Rediger' }))
    fireEvent.input(textarea(), { target: { value: 'Min tekst' } })
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    expect(await screen.findByText('Noen andre har lagret en nyere versjon. Teksten din er fortsatt her.')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Last nyeste og forkast min tekst' }))

    expect(await screen.findByText('Ola skrev dette.')).toBeTruthy()
    expect(screen.queryByRole('textbox')).toBeNull()
  })

  it('Cancel asks before dropping a changed draft, and drops it only on a yes', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(1, 'Plan A'))
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Rediger' }))
    fireEvent.input(textarea(), { target: { value: 'Endret' } })

    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))
    expect(confirm).toHaveBeenCalledWith('Forkaste teksten din?')
    expect(textarea().value).toBe('Endret')

    confirm.mockReturnValue(true)
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))
    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.getByRole('button', { name: 'Historikk' })).toBeTruthy()
  })
})

describe('SpaceCanvasPanel: differences, history and restore (WS19-S7)', () => {
  it('Review differences in a conflict shows the difference between the latest text and the draft, and hides it again', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(1, 'alfa\nbeta'))
      .mockResolvedValueOnce(viewRead(2, 'alfa\nbeta\ndelta'))
    actions.executeAction.mockRejectedValueOnce(new ApiError('moved', 409, 'canvas_head_moved'))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Rediger' }))
    fireEvent.input(textarea(), { target: { value: 'alfa\ngamma' } })
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    expect(await screen.findByText('Noen andre har lagret en nyere versjon. Teksten din er fortsatt her.')).toBeTruthy()

    const review = screen.getByRole('button', { name: 'Se over endringene' })
    expect(review.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(review)
    expect(screen.getByText('Lagt til: 1 · Fjernet: 2')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Skjul endringene' }).getAttribute('aria-expanded')).toBe('true')

    fireEvent.click(screen.getByRole('button', { name: 'Skjul endringene' }))
    expect(screen.queryByText('Lagt til: 1 · Fjernet: 2')).toBeNull()
  })

  it('Restore sends spaces.restore_canvas_version with a canvas-restore- id, and never spaces.save_canvas', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(2, 'Nyeste tekst'))
      .mockResolvedValue(viewRead(3, 'Gammel tekst'))
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(2), version(1)], nextCursor: null })
    actions.executeAction.mockResolvedValue(restored(3, 1))
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true)
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Historikk' }))
    await screen.findByText(/Versjon 1, lagret av Ola/)

    fireEvent.click(screen.getByRole('button', { name: 'Gjenopprett' }))

    expect(await screen.findByText('Versjon 1 er gjenopprettet som versjon 3.')).toBeTruthy()
    expect(confirm).toHaveBeenCalledOnce()
    const restores = actions.executeAction.mock.calls.filter((call) => call[0] === 'spaces.restore_canvas_version')
    expect(restores).toHaveLength(1)
    expect(restores[0]?.[1]).toEqual(ACTOR)
    expect(restores[0]?.[2]).toMatchObject({ spaceRef: 'room-1', versionId: 'v1', expectedVersionId: 'v2' })
    expect(String(restores[0]?.[2].clientSaveId)).toMatch(/^canvas-restore-/)
    expect(restores[0]?.[3]).toEqual({ idempotencyKey: restores[0]?.[2].clientSaveId })
    expect(actions.executeAction.mock.calls.some((call) => call[0] === 'spaces.save_canvas')).toBe(false)
  })

  it('a restore refused because the canvas was removed reads the room again and shows it has no canvas', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValueOnce(viewRead(2, 'Nyeste tekst')).mockResolvedValue(emptyRead(true))
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(2), version(1)], nextCursor: null })
    actions.executeAction.mockRejectedValue(new ApiError('gone', 404, 'canvas_not_found'))
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Historikk' }))
    await screen.findByText(/Versjon 1, lagret av Ola/)

    fireEvent.click(screen.getByRole('button', { name: 'Gjenopprett' }))

    expect(await screen.findByRole('button', { name: 'Opprett lerret' })).toBeTruthy()
    expect(screen.getByText('Rommet har ikke et lerret ennå.')).toBeTruthy()
  })

  it('a restore refused as a moved head loads the latest and shows the banner with the version it was to restore', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(2, 'Nyeste tekst'))
      .mockResolvedValue(viewRead(3, 'alfa\ngamma', { authorId: 'user-ola' }))
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(2), version(1)], nextCursor: null })
    spacesClient.getSpaceCanvasVersion.mockResolvedValue(version(1, { content: 'alfa\nbeta' }))
    actions.executeAction.mockRejectedValue(new ApiError('moved', 409, 'canvas_head_moved'))
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Historikk' }))
    await screen.findByText(/Versjon 1, lagret av Ola/)

    fireEvent.click(screen.getByRole('button', { name: 'Gjenopprett' }))

    expect(
      await screen.findByText('Noen andre har lagret en nyere versjon. Gjenopprettingen ble ikke lagret.'),
    ).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Gjenopprett versjon 1 over den nyeste' })).toBeTruthy()
    expect(spacesClient.getSpaceCanvasVersion).toHaveBeenCalledWith('room-1', 'v1')

    fireEvent.click(screen.getByRole('button', { name: 'Se over endringene' }))
    expect(screen.getByText('Lagt til: 1 · Fjernet: 1')).toBeTruthy()
  })

  it('a latest version that already holds the restored text settles as unchanged, with no banner', async () => {
    spacesClient.getSpaceCanvas
      .mockResolvedValueOnce(viewRead(2, 'Nyeste tekst'))
      .mockResolvedValue(viewRead(3, 'alfa\nbeta', { sha: 'sha-1' }))
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(2), version(1)], nextCursor: null })
    actions.executeAction.mockRejectedValue(new ApiError('moved', 409, 'canvas_head_moved'))
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Historikk' }))
    await screen.findByText(/Versjon 1, lagret av Ola/)

    fireEvent.click(screen.getByRole('button', { name: 'Gjenopprett' }))

    expect(await screen.findByText('Lerretet har allerede denne teksten, så ingen ny versjon ble lagret.')).toBeTruthy()
    expect(screen.queryByText('Noen andre har lagret en nyere versjon. Gjenopprettingen ble ikke lagret.')).toBeNull()
    expect(spacesClient.getSpaceCanvasVersion).not.toHaveBeenCalled()
  })

  it('an ambiguous restore failure keeps the restore id, so the retry replays the same write', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(2, 'Nyeste tekst'))
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(2), version(1)], nextCursor: null })
    actions.executeAction
      .mockRejectedValueOnce(new ApiError('down', 503, 'canvas_unavailable'))
      .mockResolvedValueOnce(restored(3, 1))
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Historikk' }))
    await screen.findByText(/Versjon 1, lagret av Ola/)

    fireEvent.click(screen.getByRole('button', { name: 'Gjenopprett' }))
    expect(await screen.findByText('Lerretet er midlertidig utilgjengelig. Ingenting er endret. Prøv igjen om litt.')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Gjenopprett' }))
    expect(await screen.findByText('Versjon 1 er gjenopprettet som versjon 3.')).toBeTruthy()

    const restores = actions.executeAction.mock.calls.filter((call) => call[0] === 'spaces.restore_canvas_version')
    expect(restores).toHaveLength(2)
    expect(restores[1]?.[2].clientSaveId).toBe(restores[0]?.[2].clientSaveId)
    expect(String(restores[0]?.[2].clientSaveId)).toMatch(/^canvas-restore-/)
  })

  it('a dirty draft asks before the page moves to another room, and a declined ask keeps the page; a move within the room asks nothing', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(1, 'Plan A'))
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Rediger' }))
    fireEvent.input(textarea(), { target: { value: 'Plan A, endret' } })
    const before = window.location.pathname

    navigateTo?.('/spaces/room-2')

    await vi.waitFor(() =>
      expect(confirm).toHaveBeenCalledWith('Teksten din i lerretet er ikke lagret. Forlate rommet og forkaste den?'),
    )
    await settle()
    expect(window.location.pathname).toBe(before)
    expect(textarea().value).toBe('Plan A, endret')

    confirm.mockClear()
    navigateTo?.('/spaces/room-1#lerret')
    await settle()
    expect(confirm).not.toHaveBeenCalled()
    expect(textarea().value).toBe('Plan A, endret')
  })
})

// D22aq: a room manager or owner clears the canvas by saving one newline over the head on screen.
describe('Clear canvas (D22aq)', () => {
  const clearButton = () => screen.queryByRole('button', { name: 'Tøm lerretet' })

  it('is offered only to a viewer the room marks as manager or owner, on a canvas that is not already cleared', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(2, 'Plan'))
    renderPanel({ canClear: false })
    await screen.findByRole('button', { name: 'Rediger' })
    expect(clearButton()).toBeNull()
    cleanup()

    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(2, 'Plan'))
    renderPanel({ canClear: true })
    await screen.findByRole('button', { name: 'Rediger' })
    expect(clearButton()).not.toBeNull()
    cleanup()

    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(3, '\n'))
    renderPanel({ canClear: true })
    await screen.findByRole('button', { name: 'Rediger' })
    expect(clearButton()).toBeNull()
  })

  it('asks to confirm, then saves one newline over the head through spaces.save_canvas', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValue(viewRead(2, 'Plan'))
    actions.executeAction.mockResolvedValue(stored(3))
    renderPanel({ canClear: true })
    fireEvent.click(await screen.findByRole('button', { name: 'Tøm lerretet' }))
    expect(actions.executeAction).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft at lerretet tømmes' }))

    expect(await screen.findByText('Lagret som versjon 3.')).toBeTruthy()
    expect(actions.executeAction).toHaveBeenCalledTimes(1)
    const [action, actor, input, options] = actions.executeAction.mock.calls[0] ?? []
    expect(action).toBe('spaces.save_canvas')
    expect(actor).toEqual(ACTOR)
    expect(input).toMatchObject({ spaceRef: 'room-1', expectedVersionId: 'v2', content: '\n' })
    expect(options).toEqual({ idempotencyKey: input.clientSaveId })
  })

  it('a newer head refuses the clear, says nothing was cleared, and reads the canvas again', async () => {
    spacesClient.getSpaceCanvas.mockResolvedValueOnce(viewRead(2, 'Plan')).mockResolvedValue(viewRead(3, 'Ola skrev dette.'))
    actions.executeAction.mockRejectedValue(new ApiError('moved', 409, 'canvas_head_moved'))
    renderPanel({ canClear: true })
    fireEvent.click(await screen.findByRole('button', { name: 'Tøm lerretet' }))
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft at lerretet tømmes' }))

    expect(await screen.findByText(/Lerretet ble ikke tømt/)).toBeTruthy()
    await vi.waitFor(() => expect(spacesClient.getSpaceCanvas).toHaveBeenCalledTimes(2))
  })
})

