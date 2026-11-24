import { Loader2 } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { hasErrorCode } from '@/shared/api/api-error'
import { listSpaceMemoryShares, type SpaceMemoryShare } from '@/shared/api/memory-client'
import { listSpaces, type SpaceSummary } from '@/shared/api/spaces-client'
import type { ActionActor } from '@/shared/actions/types'
import { getSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'
import { SettingsButton } from '@/features/settings/components/settings-ui'
import { memoryRoomErrorMessage, takeMemoryBackFromRoom } from '@/features/spaces/lib/space-memory-share'

/**
 * W3 D21g: "Shared to <room>, take back" in the memory list. The person's own live shares into rooms, read from
 * GET /api/v1/memory/space-shares (the server lists only the caller's own), each with a take-back that runs the
 * `memory.retract_from_space` action. Shown only while sharing is offered (presentation only, never authority); the
 * gateway and Control decide every take-back. Nothing here stores anything.
 */
export interface MemorySpaceSharesProps {
  /** Bumped by the memory list after a share, so the list is read again. */
  readonly version: number
}

const TAKE_BACK_FALLBACK = {
  no: 'Minnet ble ikke tatt tilbake. Prøv igjen.',
  en: 'The memory was not taken back. Try again.',
}

/** The signed-in actor for an action, or `null` while the session has no user or organisation (nothing is sent then). */
function currentActor(): ActionActor | null {
  const session = getSession()
  const userId = session.user?.id ?? ''
  const orgId = session.activeOrg?.id ?? ''
  return userId && orgId ? { type: 'human', userId, orgId } : null
}

/** The room names the person can see, by reference. A failed read leaves every name unknown, never the list. */
async function roomNames(): Promise<ReadonlyMap<string, string>> {
  try {
    const spaces: readonly SpaceSummary[] = await listSpaces()
    return new Map(spaces.map((space) => [space.space_ref, space.name]))
  } catch {
    return new Map()
  }
}

export function MemorySpaceShares(props: MemorySpaceSharesProps) {
  const i18n = useI18n()
  const [shares, { refetch }] = createResource(
    () => props.version,
    () => listSpaceMemoryShares(),
  )
  const [names] = createResource(() => roomNames())
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [failure, setFailure] = createSignal<string | null>(null)
  // One request id per room copy, so a retry of the same take-back is answered once.
  const requestIds = new Map<string, string>()

  const rows = createMemo<SpaceMemoryShare[]>(() => (shares.error ? [] : (shares() ?? [])))
  // A gateway with sharing off answers 404 `feature_disabled`: the section is simply not shown then.
  const unavailable = createMemo(() => shares.error !== undefined && !hasErrorCode(shares.error, 'feature_disabled'))

  const roomName = (spaceRef: string) =>
    names()?.get(spaceRef) ?? i18n.tr('et rom du ikke lenger ser', 'a room you no longer see')

  const takeBack = async (share: SpaceMemoryShare) => {
    const who = currentActor()
    if (!who || busyId() !== null) return
    let clientRequestId = requestIds.get(share.roomMemoryId)
    if (!clientRequestId) {
      clientRequestId = crypto.randomUUID()
      requestIds.set(share.roomMemoryId, clientRequestId)
    }
    setBusyId(share.roomMemoryId)
    setFailure(null)
    try {
      const result = await takeMemoryBackFromRoom(who, {
        memoryId: share.roomMemoryId,
        spaceRef: share.spaceRef,
        clientRequestId,
      })
      if (!result.deleted) setFailure(i18n.tr(TAKE_BACK_FALLBACK.no, TAKE_BACK_FALLBACK.en))
    } catch (error) {
      // A copy already gone from the room is the outcome asked for.
      if (!hasErrorCode(error, 'memory_not_found')) {
        setFailure(memoryRoomErrorMessage(error, i18n.tr, 'take_back', TAKE_BACK_FALLBACK))
      }
    } finally {
      setBusyId(null)
    }
    // The list is read afresh either way; a failed read shows as the section's own error, never as a failed take-back.
    await Promise.resolve(refetch()).catch(() => undefined)
  }

  return (
    <>
      <Show when={unavailable()}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste minnene du har delt i rom.', 'Could not load the memories you shared to rooms.')}
        </p>
      </Show>
      <Show when={failure()}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {failure()}
        </p>
      </Show>
      <Show when={rows().length > 0}>
        <section class="verevon-settings-list-card" aria-labelledby="memory-space-shares-title">
          <div class="verevon-settings-integration-row">
            <div>
              <p id="memory-space-shares-title">{i18n.tr('Delt i rom', 'Shared to rooms')}</p>
              <span>
                {i18n.tr(
                  'Kopier av minnene dine som du har delt i rom. Ta en tilbake for å fjerne den fra rommet; ditt eget minne beholdes.',
                  'Copies of your memories you shared to rooms. Take one back to remove it from the room; your own memory is kept.',
                )}
              </span>
            </div>
          </div>
          <For each={rows()}>
            {(share) => (
              <div class="verevon-settings-integration-row" data-room-memory-id={share.roomMemoryId}>
                <div>
                  <p>
                    {i18n.tr('Delt i', 'Shared to')} {roomName(share.spaceRef)}
                  </p>
                  <Show when={share.sharedAt}>
                    {(at) => (
                      <span>
                        {i18n.tr('Delt', 'Shared')}: {new Date(at()).toLocaleString()}
                      </span>
                    )}
                  </Show>
                </div>
                <div>
                  <SettingsButton
                    settingsSize="sm"
                    disabled={busyId() !== null}
                    onClick={() => void takeBack(share)}
                    aria-label={`${i18n.tr('Ta tilbake fra', 'Take back from')} ${roomName(share.spaceRef)}`}
                  >
                    <Show when={busyId() === share.roomMemoryId} fallback={i18n.tr('Ta tilbake', 'Take back')}>
                      <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Tar tilbake…', 'Taking back…')}
                    </Show>
                  </SettingsButton>
                </div>
              </div>
            )}
          </For>
        </section>
      </Show>
    </>
  )
}
