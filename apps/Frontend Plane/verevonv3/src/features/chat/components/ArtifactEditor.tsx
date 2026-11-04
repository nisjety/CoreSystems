/**
 * The Output panel's edit mode for a text artifact (EP-3 S4, action `chat.save_artifact_revision`): a plain
 * source textarea (Markdown, code or HTML as text), Save and Cancel. No rich-text editor in W3.
 *
 * A save is a compare-and-set on the version the editor opened. When another tab, another device or a turn moved
 * the artifact first, the server answers `revision_mismatch` with the head it now holds, and the editor shows a
 * banner: "Load newest" replaces the text with the newest version the panel holds, "Overwrite" saves again over
 * that head, and only after that explicit click. Cancel with unsaved text asks before it discards, and leaving
 * the page while dirty triggers the browser's own prompt. The text lives only in this component's memory: nothing
 * is written to storage, which is what keeps an edit in progress out of the ZDR question.
 */
import { createEffect, createMemo, createSignal, onCleanup, Show, untrack } from 'solid-js'
import {
  artifactConflictHeadVersion,
  saveArtifactRevision,
  type SavedArtifactRevision,
} from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/api-error'
import { useI18n } from '@/shared/i18n'

/** The newest revision the panel holds, offered by "load newest" after a conflict. */
export type ArtifactEditorHead = { content: string; title: string; version: number }

export function ArtifactEditor(props: {
  threadId: string
  artifactId: string
  title: string
  /** The head text the editor opens on. */
  content: string
  /** The head version the editor opens on: the compare-and-set of the first save. */
  headVersion: number
  onSaved: (revision: SavedArtifactRevision) => void
  onClose: () => void
  /** The newest revision the panel holds now; read when the person asks for the newest after a conflict. */
  newest?: () => ArtifactEditorHead | undefined
}) {
  const i18n = useI18n()
  // The editor owns its text from the moment it opens: a version streaming in for this artifact does not
  // replace what the person is typing. "Load newest" is the only way the base moves.
  const opened = untrack(() => ({ content: props.content, version: props.headVersion }))
  const [baseText, setBaseText] = createSignal(opened.content)
  const [baseVersion, setBaseVersion] = createSignal(opened.version)
  const [text, setText] = createSignal(opened.content)
  const [saving, setSaving] = createSignal(false)
  const [conflictHead, setConflictHead] = createSignal<number | null>(null)
  const [failure, setFailure] = createSignal<string | null>(null)
  const [confirmDiscard, setConfirmDiscard] = createSignal(false)
  const dirty = createMemo(() => text() !== baseText())

  // Solid v2 has no onMount: a two-phase effect with a constant compute runs once after mount.
  const guardUnload = (event: BeforeUnloadEvent) => {
    if (!dirty()) return
    event.preventDefault()
  }
  createEffect(
    () => undefined,
    () => window.addEventListener('beforeunload', guardUnload),
  )
  onCleanup(() => window.removeEventListener('beforeunload', guardUnload))

  const save = async (expectedVersion: number) => {
    if (saving() || text().length === 0) return
    setSaving(true)
    setFailure(null)
    try {
      const revision = await saveArtifactRevision({
        threadId: props.threadId,
        artifactId: props.artifactId,
        content: text(),
        expectedVersion,
      })
      setConflictHead(null)
      setBaseText(revision.content)
      setBaseVersion(revision.version)
      props.onSaved(revision)
    } catch (error) {
      const head = artifactConflictHeadVersion(error)
      if (head != null) {
        setConflictHead(head)
      } else {
        setFailure(failureMessage(error, i18n.tr))
      }
    } finally {
      setSaving(false)
    }
  }

  const loadNewest = () => {
    const head = conflictHead()
    const newest = props.newest?.()
    if (!newest || (head != null && newest.version < head)) {
      setFailure(i18n.tr(
        'Den nyeste versjonen er ikke lastet inn ennå. Last inn samtalen på nytt for å se den.',
        'The newest version is not loaded yet. Reload the conversation to see it.',
      ))
      return
    }
    setBaseText(newest.content)
    setBaseVersion(newest.version)
    setText(newest.content)
    setConflictHead(null)
    setFailure(null)
  }

  const cancel = () => {
    if (dirty()) {
      setConfirmDiscard(true)
      return
    }
    props.onClose()
  }

  return (
    <div class="verevon-chat-artifact-editor">
      <Show when={conflictHead()}>
        {(head) => (
          <div class="verevon-chat-artifact-editor__conflict" role="alert">
            <p>
              {i18n.tr(
                `Artefaktet ble endret etter at du åpnet det (nå versjon ${head()}).`,
                `The artifact changed after you opened it (now version ${head()}).`,
              )}
            </p>
            <div class="verevon-chat-artifact-editor__actions">
              <button type="button" class="verevon-chat-artifact-action" onClick={loadNewest}>
                {i18n.tr('Last inn nyeste', 'Load newest')}
              </button>
              <button
                type="button"
                class="verevon-chat-artifact-action"
                disabled={saving()}
                onClick={() => void save(head())}
              >
                {i18n.tr('Overskriv', 'Overwrite')}
              </button>
            </div>
          </div>
        )}
      </Show>
      <Show when={failure()}>
        {(message) => <p class="verevon-chat-artifact-editor__failure" role="alert">{message()}</p>}
      </Show>
      <textarea
        class="verevon-chat-artifact-editor__source"
        aria-label={i18n.tr(`Rediger ${props.title}`, `Edit ${props.title}`)}
        spellcheck={false}
        value={text()}
        disabled={saving()}
        onInput={(event) => setText(event.currentTarget.value)}
      />
      <Show
        when={!confirmDiscard()}
        fallback={(
          <div class="verevon-chat-artifact-editor__discard" role="alertdialog" aria-label={i18n.tr('Forkaste endringene?', 'Discard your changes?')}>
            <p>{i18n.tr('Du har endringer som ikke er lagret.', 'You have changes that are not saved.')}</p>
            <div class="verevon-chat-artifact-editor__actions">
              <button type="button" class="verevon-chat-artifact-action" onClick={() => props.onClose()}>
                {i18n.tr('Forkast', 'Discard')}
              </button>
              <button type="button" class="verevon-chat-artifact-action" onClick={() => setConfirmDiscard(false)}>
                {i18n.tr('Fortsett å redigere', 'Keep editing')}
              </button>
            </div>
          </div>
        )}
      >
        <div class="verevon-chat-artifact-editor__actions">
          <button type="button" class="verevon-chat-artifact-action" onClick={cancel}>
            {i18n.tr('Avbryt', 'Cancel')}
          </button>
          <button
            type="button"
            class="verevon-chat-artifact-action verevon-chat-artifact-action--primary"
            disabled={saving() || !dirty() || text().length === 0 || conflictHead() != null}
            onClick={() => void save(baseVersion())}
          >
            {saving() ? i18n.tr('Lagrer …', 'Saving …') : i18n.tr('Lagre', 'Save')}
          </button>
        </div>
      </Show>
    </div>
  )
}

/** A refusal in the person's words. The server's message is never shown: the code says what happened. */
function failureMessage(error: unknown, tr: (noText: string, enText: string) => string): string {
  const code = error instanceof ApiError ? error.code : null
  switch (code) {
    case 'artifact_edit_run_active':
      return tr(
        'Verevon jobber fortsatt i samtalen. Lagre når svaret er ferdig.',
        'Verevon is still working in this conversation. Save when the answer is done.',
      )
    case 'artifact_revision_limit':
      return tr(
        'Artefaktet har nådd grensen for antall versjoner. Lagre det i kunnskapsbasen for å jobbe videre.',
        'This artifact has reached its version limit. Save it to knowledge to keep working on it.',
      )
    case 'artifact_edit_space_thread':
    case 'artifact_edit_waived_thread':
    case 'artifact_edit_kind_unsupported':
    case 'artifact_editing_unavailable':
    case 'zdr_durable_mutation_forbidden':
      return tr(
        'Dette artefaktet kan ikke redigeres i denne samtalen.',
        'This artifact cannot be edited in this conversation.',
      )
    default:
      return tr('Kunne ikke lagre. Prøv igjen.', 'Could not save. Please try again.')
  }
}
