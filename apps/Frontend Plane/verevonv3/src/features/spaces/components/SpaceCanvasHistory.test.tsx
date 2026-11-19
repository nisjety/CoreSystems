import { cleanup, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceCanvasVersion, SpaceRosterMember } from '@/shared/api/spaces-client'
import { fireEvent } from '@/test/fire-event'
import { render } from '@/test/render'

// WS19-S7: the version history of the room canvas. The reads are mocked at the spaces client; the tests assert what is
// asked for (the page size, the cursor, the texts of two picks) and what the reader sees (no duplicate rows, restore
// only where it is allowed, a declined confirm sends nothing).

const spacesClient = vi.hoisted(() => ({
  listSpaceCanvasVersions: vi.fn(),
  getSpaceCanvasVersion: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceCanvasHistory } = await import('./SpaceCanvasHistory')

const ROSTER: SpaceRosterMember[] = [
  { subject_type: 'user', subject_id: 'user-ola', role: 'editor', revision: 1, display_name: 'Ola' },
]

/** One version without its text, as the history page lists it. */
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
    contentBytes: 10,
    contentSha256: `sha-${no}`,
    createdAt: `2026-11-18T10:0${no}:00Z`,
    ...over,
  }
}

function renderHistory(
  options: {
    readonly head?: () => string
    readonly canRestore?: boolean
    readonly onRestore?: (version: SpaceCanvasVersion) => void
  } = {},
) {
  const head = options.head ?? (() => 'v3')
  render(() => (
    <SpaceCanvasHistory
      spaceRef="room-1"
      roster={() => ROSTER}
      headVersionId={head()}
      canRestore={options.canRestore ?? true}
      restoring={false}
      onRestore={options.onRestore ?? vi.fn()}
      onClose={vi.fn()}
    />
  ))
}

/** The history row that names version `no`. */
function rowOf(no: number): HTMLElement {
  const row = [...document.querySelectorAll<HTMLElement>('li.verevon-space-canvas__history-row')].find((item) =>
    item.textContent?.includes(`Versjon ${no},`),
  )
  if (!row) throw new Error(`no history row for version ${no}`)
  return row
}

const pick = (no: number) => fireEvent.click(rowOf(no).querySelector('input[type="checkbox"]') as HTMLInputElement)

beforeEach(() => {
  spacesClient.listSpaceCanvasVersions.mockReset()
  spacesClient.getSpaceCanvasVersion.mockReset()
  spacesClient.getSpaceCanvasVersion.mockImplementation(async (_spaceRef: string, versionId: string) => {
    const no = Number(versionId.slice(1))
    return version(no, { content: `tekst ${no}\nfelles` })
  })
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('SpaceCanvasHistory (WS19-S7)', () => {
  it('reads the first page with a limit of 50 and lists the versions newest first, naming who saved each', async () => {
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(3), version(2), version(1)], nextCursor: null })
    renderHistory()

    expect(await screen.findByText(/Versjon 3, lagret av Ola/)).toBeTruthy()
    expect(spacesClient.listSpaceCanvasVersions).toHaveBeenCalledWith('room-1', { limit: 50 })
    const rows = [...document.querySelectorAll('li.verevon-space-canvas__history-row')].map((row) => row.textContent ?? '')
    expect(rows[0]).toContain('Versjon 3')
    expect(rows[2]).toContain('Versjon 1')
    expect(rows[0]).toContain('nåværende')
  })

  it('Load more appends the next page by its cursor, and never repeats a version it already shows', async () => {
    spacesClient.listSpaceCanvasVersions
      .mockResolvedValueOnce({ versions: [version(3), version(2)], nextCursor: '2' })
      .mockResolvedValueOnce({ versions: [version(2), version(1)], nextCursor: null })
    renderHistory()

    fireEvent.click(await screen.findByRole('button', { name: 'Last flere' }))

    await vi.waitFor(() => expect(screen.queryByRole('button', { name: 'Last flere' })).toBeNull())
    expect(spacesClient.listSpaceCanvasVersions).toHaveBeenLastCalledWith('room-1', { limit: 50, cursor: '2' })
    const rows = document.querySelectorAll('li.verevon-space-canvas__history-row')
    expect(rows).toHaveLength(3)
    expect([...rows].map((row) => row.textContent ?? '').filter((text) => text.includes('Versjon 2,'))).toHaveLength(1)
  })

  it('offers Restore on every version but the head, and on none for a viewer', async () => {
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(3), version(2), version(1)], nextCursor: null })
    renderHistory({ canRestore: true })
    await screen.findByText(/Versjon 3, lagret av Ola/)
    expect(screen.getAllByRole('button', { name: 'Gjenopprett' })).toHaveLength(2)
    expect(rowOf(3).querySelector('button')).toBeNull()
    cleanup()

    renderHistory({ canRestore: false })
    await screen.findByText(/Versjon 3, lagret av Ola/)
    expect(screen.queryByRole('button', { name: 'Gjenopprett' })).toBeNull()
  })

  it('a declined confirm sends no restore; a confirmed one hands the version over', async () => {
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(3), version(2)], nextCursor: null })
    const onRestore = vi.fn()
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
    renderHistory({ onRestore })
    await screen.findByText(/Versjon 2, lagret av Ola/)

    fireEvent.click(screen.getByRole('button', { name: 'Gjenopprett' }))
    expect(confirm).toHaveBeenCalledOnce()
    expect(onRestore).not.toHaveBeenCalled()

    confirm.mockReturnValue(true)
    fireEvent.click(screen.getByRole('button', { name: 'Gjenopprett' }))
    expect(onRestore).toHaveBeenCalledOnce()
    expect(onRestore).toHaveBeenCalledWith(expect.objectContaining({ versionId: 'v2', versionNo: 2 }))
  })

  it('two picks read both texts, older first and as a pair; a third pick drops the oldest', async () => {
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(3), version(2), version(1)], nextCursor: null })
    renderHistory()
    await screen.findByText(/Versjon 3, lagret av Ola/)

    pick(3)
    expect(screen.getByText('Velg to versjoner for å se forskjellen.')).toBeTruthy()
    expect(spacesClient.getSpaceCanvasVersion).not.toHaveBeenCalled()

    pick(1)
    expect(await screen.findByText('Versjon 1 til versjon 3')).toBeTruthy()
    expect(spacesClient.getSpaceCanvasVersion).toHaveBeenCalledWith('room-1', 'v1')
    expect(spacesClient.getSpaceCanvasVersion).toHaveBeenCalledWith('room-1', 'v3')
    expect(screen.getByText('Lagt til: 1 · Fjernet: 1')).toBeTruthy()

    pick(2)
    expect(await screen.findByText('Versjon 1 til versjon 2')).toBeTruthy()
    expect(screen.queryByText('Versjon 1 til versjon 3')).toBeNull()
    expect((rowOf(3).querySelector('input') as HTMLInputElement).checked).toBe(false)
    expect((rowOf(2).querySelector('input') as HTMLInputElement).checked).toBe(true)
  })

  it('a new head reads page one again, so a save or a restore shows at once', async () => {
    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(3)], nextCursor: null })
    const [head, setHead] = createSignal('v3')
    renderHistory({ head })
    await screen.findByText(/Versjon 3, lagret av Ola/)
    expect(spacesClient.listSpaceCanvasVersions).toHaveBeenCalledTimes(1)

    spacesClient.listSpaceCanvasVersions.mockResolvedValue({ versions: [version(4), version(3)], nextCursor: null })
    setHead('v4')
    flush()

    expect(await screen.findByText(/Versjon 4, lagret av Ola/)).toBeTruthy()
    expect(spacesClient.listSpaceCanvasVersions).toHaveBeenCalledTimes(2)
    expect(spacesClient.listSpaceCanvasVersions).toHaveBeenLastCalledWith('room-1', { limit: 50 })
  })

  it('a failed first read is not an empty history, and Try again reads the versions again', async () => {
    spacesClient.listSpaceCanvasVersions
      .mockRejectedValueOnce(new TypeError('offline'))
      .mockResolvedValueOnce({ versions: [version(1)], nextCursor: null })
    renderHistory()

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.queryByText('Lerretet har ingen versjoner ennå.')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
    expect(await screen.findByText(/Versjon 1, lagret av Ola/)).toBeTruthy()
    expect(spacesClient.listSpaceCanvasVersions).toHaveBeenCalledTimes(2)
  })
})
