import { createSignal, Show, For } from 'solid-js'

import { createPersonalSpace, listSpaces, type SpaceSummary } from '@/shared/api/spaces-client'
import { createResource } from '@/shared/lib/create-resource-compat'
import { Redirect } from '@/shared/ui/Redirect'

/**
 * Entry point for `/spaces`, so the sidebar has something to link to.
 *
 * `SpacePage` is mounted at `/spaces/:spaceId` and cannot resolve a Space on
 * its own — it reads the ref out of the route. That left the surface reachable
 * only by already knowing a Space id, which is why it never appeared in
 * navigation: there was no href to point at.
 *
 * This resolves one and forwards to it. It renders no cockpit of its own, so
 * there is a single Space surface rather than two that drift.
 */

/**
 * Prefer the caller's Personal Space, then any active one, then whatever came
 * back. A `lifecycle` that is not active still resolves rather than being
 * skipped — an archived or suspended Space must stay openable so its owner can
 * see *why* it is unavailable, and `SpacePage` already renders that state
 * honestly from the server's own membership recheck.
 */
export function pickDefaultSpace(
  spaces: readonly SpaceSummary[],
): SpaceSummary | undefined {
  return (
    spaces.find((space) => space.kind === 'personal' && space.lifecycle === 'active') ??
    spaces.find((space) => space.kind === 'personal') ??
    spaces.find((space) => space.lifecycle === 'active') ??
    spaces[0]
  )
}

export default function SpacesIndexPage() {
  const [spaces, { refetch }] = createResource(listSpaces)
  const [creating, setCreating] = createSignal(false)
  const [createError, setCreateError] = createSignal('')

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
              <div>
                <h1 id="spaces-index-title">Ingen rom ennå</h1>
                <p>
                  Du har ikke et personlig rom i denne organisasjonen. Opprett det her — det
                  blir ditt eget, og du kan invitere andre inn i det senere.
                </p>
                <button type="button" onClick={() => void createRoom()} disabled={creating()}>
                  {creating() ? 'Oppretter rommet …' : 'Opprett mitt rom'}
                </button>
                {/* Honest about the two-step lifecycle: the room exists as soon
                    as this returns, but Control must register it before any
                    Space action is allowed. Saying "ready" here would promise
                    something the next screen would then refuse. */}
                <p>
                  Rommet opprettes med én gang, men må registreres av Control Plane før
                  handlinger i det er tillatt. Du ser statusen i rommet.
                </p>
                <Show when={createError()}>
                  <p role="alert">{createError()}</p>
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
