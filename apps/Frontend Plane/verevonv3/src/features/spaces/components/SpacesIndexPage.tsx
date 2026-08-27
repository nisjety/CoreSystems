import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'

import {
  createPersonalSpace,
  ensureOrganizationRoom,
  listSpaces,
  type SpaceSummary,
} from '@/shared/api/spaces-client'
import { createResource } from '@/shared/lib/create-resource-compat'
import { Loader2 } from '@/shared/icons'
import { Redirect } from '@/shared/ui/Redirect'

/**
 * Entry point for `/spaces`, so the sidebar has something to link to.
 *
 * `SpacePage` is mounted at `/spaces/:spaceId` and cannot resolve a Space on
 * its own — it reads the ref out of the route. This resolves one and forwards
 * to it, and it is also where the organization's shared room self-heals: when
 * the listing shows no channel at all, the (idempotent) org-room ensure runs,
 * covering organizations created before onboarding started provisioning it
 * and any Convex outage during onboarding.
 */

/**
 * Where `/spaces` lands you, Slack-shaped: the organization's channel first,
 * then any other active channel, then your personal room.
 *
 * Only ACTIVE channels can capture the landing — a channel still awaiting
 * Control registration would greet the user with a refusal, which is a worse
 * landing than their own room. A non-active personal Space still resolves
 * rather than being skipped: an archived or suspended room must stay openable
 * so its owner can see *why* it is unavailable, and `SpacePage` renders that
 * state honestly from the server's own membership recheck.
 */
export function pickDefaultSpace(
  spaces: readonly SpaceSummary[],
): SpaceSummary | undefined {
  return (
    spaces.find((space) => space.kind === 'room' && space.is_organization_room === true && space.lifecycle === 'active') ??
    spaces.find((space) => space.kind === 'room' && space.lifecycle === 'active') ??
    spaces.find((space) => space.kind === 'personal' && space.lifecycle === 'active') ??
    spaces.find((space) => space.kind === 'personal') ??
    spaces.find((space) => space.lifecycle === 'active') ??
    spaces[0]
  )
}

type OrgRoomProvisioning = 'idle' | 'ensuring' | 'awaiting_registration' | 'failed' | 'timed_out'

/** How long the resolver waits for Control to register a freshly ensured org
 * room before it stops polling and says so. The room keeps registering in the
 * background either way — this only bounds how long this page holds a spinner. */
const REGISTRATION_POLL_ATTEMPTS = 8
const REGISTRATION_POLL_INTERVAL_MS = 2_500

export default function SpacesIndexPage() {
  const [spaces, { refetch }] = createResource(listSpaces)
  const [creating, setCreating] = createSignal(false)
  const [createError, setCreateError] = createSignal('')
  const [provisioning, setProvisioning] = createSignal<OrgRoomProvisioning>('idle')
  let orgEnsureStarted = false
  let cancelled = false
  onCleanup(() => {
    cancelled = true
  })

  // Self-heal the organization's channel. Runs at most once per mount, only
  // when a RESOLVED listing proves there is no channel — a failed listing is
  // "membership could not be read", which must not trigger provisioning.
  // Fire-and-forget on purpose: if the caller already has a personal room the
  // redirect below wins the race and the ensure completes silently.
  createEffect(
    () => (spaces.error ? undefined : spaces()),
    (resolved) => {
      if (!resolved || cancelled) return
      if (resolved.some((space) => space.kind === 'room')) return
      if (orgEnsureStarted) return
      orgEnsureStarted = true
      void provisionOrganizationRoom()
    },
  )

  async function provisionOrganizationRoom() {
    setProvisioning('ensuring')
    try {
      await ensureOrganizationRoom()
    } catch {
      if (!cancelled) setProvisioning('failed')
      return
    }
    if (cancelled) return
    setProvisioning('awaiting_registration')
    // The listing intersects Control's index, so the ensured room only appears
    // once Control registers it. Poll briefly for that moment; on timeout the
    // registration continues server-side and this page says so honestly.
    for (let attempt = 0; attempt < REGISTRATION_POLL_ATTEMPTS; attempt += 1) {
      await delay(REGISTRATION_POLL_INTERVAL_MS)
      if (cancelled) return
      try {
        const next = await refetch()
        if (Array.isArray(next) && next.some((space) => space.kind === 'room')) {
          setProvisioning('idle')
          return
        }
      } catch {
        // The listing itself failed; keep polling — the resource's own error
        // state governs what the page reports if it stays down.
      }
    }
    if (!cancelled) setProvisioning('timed_out')
  }

  function retryOrganizationRoom() {
    if (provisioning() === 'ensuring' || provisioning() === 'awaiting_registration') return
    void provisionOrganizationRoom()
  }

  async function createRoom() {
    if (creating()) return
    setCreateError('')
    setCreating(true)
    try {
      await createPersonalSpace()
      // Refetch rather than navigating straight to the returned ref: the list
      // is the single source this page resolves from, so one path decides where
      // you land whether the room was just made or already existed.
      await refetch()
    } catch {
      setCreateError(
        'Rommet kunne ikke opprettes. Ingenting ble klargjort — prøv igjen, eller last siden på nytt.',
      )
    } finally {
      setCreating(false)
    }
  }

  const provisioningBusy = () =>
    provisioning() === 'ensuring' || provisioning() === 'awaiting_registration'

  return (
    <section class="space-page" aria-labelledby="spaces-index-title">
      <Show when={spaces.loading}>
        <p role="status" aria-live="polite">Åpner rommet ditt …</p>
      </Show>

      {/* A failed list is NOT "no rooms": membership could not be read, which is
          a different fact and must not be reported as an empty workspace. */}
      <Show when={spaces.error}>
        <div role="alert">
          <h1 id="spaces-index-title">Rom er utilgjengelig</h1>
          <p>
            Medlemskapet ditt kunne ikke bekreftes, så vi kan ikke vise hvilke rom du har
            tilgang til. Ingen romhandlinger er tilgjengelige akkurat nå.
          </p>
        </div>
      </Show>

      <Show when={spaces.error ? undefined : spaces()}>
        {(resolved) => (
          <Show
            when={pickDefaultSpace(resolved())}
            fallback={
              <div class="spaces-index-provisioning">
                <Show
                  when={provisioningBusy()}
                  fallback={
                    <div>
                      <h1 id="spaces-index-title">Ingen rom ennå</h1>
                      <Show
                        when={provisioning() === 'timed_out'}
                        fallback={
                          <p>
                            Organisasjonens rom kunne ikke opprettes akkurat nå. Ingenting ble
                            klargjort — prøv igjen, eller opprett ditt personlige rom først.
                          </p>
                        }
                      >
                        <p>
                          Organisasjonens rom er opprettet og registreres fortsatt hos Control
                          Plane. Det dukker opp i listen så snart registreringen er fullført —
                          du kan vente her, eller opprette ditt personlige rom i mellomtiden.
                        </p>
                      </Show>
                      <div class="spaces-index-actions">
                        <button type="button" onClick={retryOrganizationRoom}>
                          {provisioning() === 'timed_out' ? 'Sjekk igjen' : 'Prøv igjen'}
                        </button>
                        <button type="button" onClick={() => void createRoom()} disabled={creating()}>
                          {creating() ? 'Oppretter rommet …' : 'Opprett mitt personlige rom'}
                        </button>
                      </div>
                      {/* Honest about the two-step lifecycle: a room exists as soon
                          as creation returns, but Control must register it before any
                          Space action is allowed. Saying "ready" here would promise
                          something the next screen would then refuse. */}
                      <p>
                        Rom opprettes med én gang, men må registreres av Control Plane før
                        handlinger i dem er tillatt. Du ser statusen i rommet.
                      </p>
                      <Show when={createError()}>
                        <p role="alert">{createError()}</p>
                      </Show>
                    </div>
                  }
                >
                  <div role="status" aria-live="polite">
                    <h1 id="spaces-index-title">Gjør klar organisasjonens rom</h1>
                    <p class="spaces-index-provisioning__status">
                      <Loader2 size={15} class="onboarding-phase-spinner" />
                      {provisioning() === 'ensuring'
                        ? 'Oppretter organisasjonens rom …'
                        : 'Rommet er opprettet — venter på registrering hos Control Plane …'}
                    </p>
                  </div>
                </Show>
              </div>
            }
          >
            {(target) => (
              <>
                {/* replace, not push: /spaces is a resolver, and leaving it on the
                    back stack would bounce the user forward again on every Back. */}
                <Redirect href={`/spaces/${encodeURIComponent(target().space_ref)}`} />
                {/* Rendered only if navigation is blocked, so the resolution is
                    never a blank screen. */}
                <noscript>
                  <ul>
                    <For each={resolved()}>
                      {(space) => (
                        <li>
                          <a href={`/spaces/${encodeURIComponent(space.space_ref)}`} link>
                            {space.name}
                          </a>
                        </li>
                      )}
                    </For>
                  </ul>
                </noscript>
              </>
            )}
          </Show>
        )}
      </Show>
    </section>
  )
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms)
  })
}
