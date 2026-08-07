import { createResource, createSignal, For, onCleanup, onMount, Show } from 'solid-js'

import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { isGateOpen } from '@/shared/context/ownership-gate'
import { translateApiError, useI18n } from '@/shared/i18n'
import {
  listDocumentShares,
  revokeDocumentShare,
  shareDocument,
  type DocumentGrant,
} from '@/shared/api/ownership-client'

/**
 * Share a document with a user (view-only, MVP). The entire dialog is wrapped in
 * `isGateOpen()` so it can NEVER render unless the backend is enforcing per-user
 * privacy in strict mode with a live identity — the PR-6 honesty gate. Writes go
 * to resource_grants (via the gateway shares domain), the SAME authority
 * retrieval + documents-api enforce against.
 */
export function ShareDialog(props: {
  docId: string
  visibility?: 'private' | 'org' | 'shared'
  onClose: () => void
}) {
  const i18n = useI18n()
  const [subject, setSubject] = createSignal('')
  const [busy, setBusy] = createSignal(false)
  const [errorMessage, setErrorMessage] = createSignal('')
  const [shares, { refetch }] = createResource(() => props.docId, listDocumentShares)

  onMount(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') props.onClose()
    }
    window.addEventListener('keydown', closeOnEscape)
    onCleanup(() => window.removeEventListener('keydown', closeOnEscape))
  })

  async function addShare() {
    const target = subject().trim()
    if (!target || busy()) return
    setBusy(true)
    setErrorMessage('')
    try {
      await shareDocument(props.docId, target)
      setSubject('')
      await refetch()
    } catch (error) {
      setErrorMessage(translateApiError(error, i18n.tr, { no: 'Dokumentet kunne ikke deles.', en: 'The document could not be shared.' }))
    } finally {
      setBusy(false)
    }
  }

  async function removeShare(subjectId: string) {
    if (busy()) return
    setBusy(true)
    setErrorMessage('')
    try {
      await revokeDocumentShare(props.docId, subjectId)
      await refetch()
    } catch (error) {
      // revokeDocumentShare can fail (e.g. a transient 502) after the share
      // was already durably removed server-side. Re-fetch and check whether
      // the subject is still listed before asserting failure, instead of
      // trusting the network error alone — otherwise a sharer sees a false
      // "could not be removed" error for a revoke that already went through.
      let fresh: { grants: DocumentGrant[] } | null | undefined
      try {
        fresh = await refetch()
      } catch {
        fresh = undefined
      }
      if (!fresh) {
        setErrorMessage(i18n.tr(
          'Vi fikk ikke bekreftet om delingen ble fjernet. Vent litt før du prøver på nytt.',
          "We couldn't confirm whether the share was removed. Please wait a moment before trying again.",
        ))
      } else if (fresh.grants.some((grant) => grant.subject_id === subjectId)) {
        // Still listed — the removal genuinely didn't land.
        setErrorMessage(translateApiError(error, i18n.tr, { no: 'Delingen kunne ikke fjernes.', en: 'The share could not be removed.' }))
      }
      // Else: confirmed removed — refetch() already synced the shares
      // resource that the list renders from; no further state change needed.
    } finally {
      setBusy(false)
    }
  }

  return (
    <Show when={isGateOpen()} fallback={null}>
      <div class="knowledge-modal" role="dialog" aria-modal="true" aria-label="Share document">
        <button
          class="knowledge-modal__scrim"
          type="button"
          aria-label="Close share dialog"
          onClick={() => props.onClose()}
        />
        <div class="knowledge-modal__panel">
          <header class="knowledge-modal__header">
            <div>
              <h2>Share document</h2>
              <p>
                People you share with can <strong>view</strong> this document. Access is enforced
                everywhere it's read — search, retrieval, and the agent.
              </p>
            </div>
            <button
              class="knowledge-modal__close"
              type="button"
              onClick={() => props.onClose()}
              aria-label="Close share dialog"
            >
              Esc
            </button>
          </header>

          <section class="knowledge-modal-card">
            <div class="knowledge-modal-form-grid knowledge-modal-form-grid--url">
              <label class="knowledge-field">
                <span>Share with (user id or email)</span>
                <VerevonInput
                  value={subject()}
                  placeholder="user-id or name@company.com"
                  onInput={(event) => setSubject(event.currentTarget.value)}
                />
              </label>
              <label class="knowledge-field">
                <span>Role</span>
                <VerevonInput value="Can view" disabled readonly />
              </label>
            </div>
            <Button
              variant="primary"
              size="sm"
              class="knowledge-modal-action"
              disabled={busy() || !subject().trim()}
              onClick={() => void addShare()}
            >
              Share
            </Button>
            <Show when={errorMessage()}>
              <p class="knowledge-share-error" role="alert">
                {errorMessage()}
              </p>
            </Show>
          </section>

          <section class="knowledge-modal-card">
            <h3>Shared with</h3>
            <Show
              when={(shares()?.grants?.length ?? 0) > 0}
              fallback={<p class="knowledge-muted-copy">Not shared with anyone yet.</p>}
            >
              <ul class="knowledge-share-list">
                <For each={shares()?.grants ?? []}>
                  {(grant: DocumentGrant) => (
                    <li class="knowledge-share-row">
                      <span class="knowledge-share-subject">{grant.subject_id}</span>
                      <span class="knowledge-muted-copy">Can view</span>
                      <button
                        type="button"
                        class="knowledge-share-remove"
                        disabled={busy()}
                        onClick={() => void removeShare(grant.subject_id)}
                        aria-label={`Remove ${grant.subject_id}`}
                      >
                        Remove
                      </button>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </section>
        </div>
      </div>
    </Show>
  )
}
