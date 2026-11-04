import { createSignal, For, Show } from 'solid-js'
import {
  parseThreadWorkspaceListing,
  parseThreadWorkspaceReset,
  threadWorkspacePath,
  type ThreadWorkspaceFile,
} from '@/shared/actions/chat-workspace'
import { requestJson } from '@/shared/api/http'
import { useI18n } from '@/shared/i18n'

/**
 * The thread files control is shown only when the build turns it on. The sandbox keeps a
 * thread's files only where the thread workspace switch is on as well, so the control stays
 * off until both are.
 */
export const threadWorkspaceEnabled = () => import.meta.env.VITE_CHAT_THREAD_WORKSPACE === 'true'

/**
 * "Thread files" (WS22-S5, DEC-23): the files this chat's sandbox kept across runs, and a reset
 * that erases them for good. Listing and reset go through the `chat.workspace` contract. The reset
 * takes a second press, because nothing can bring the files back once they are erased.
 */
export function ThreadWorkspacePanel(props: { threadId: string }) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [loading, setLoading] = createSignal(false)
  const [files, setFiles] = createSignal<ThreadWorkspaceFile[]>([])
  const [failed, setFailed] = createSignal(false)
  const [confirming, setConfirming] = createSignal(false)
  const [resetCount, setResetCount] = createSignal<number | null>(null)
  const [resetFailed, setResetFailed] = createSignal(false)

  async function load() {
    setLoading(true)
    setFailed(false)
    try {
      const listing = parseThreadWorkspaceListing(await requestJson<unknown>(threadWorkspacePath(props.threadId)))
      setFiles(listing.files)
    } catch {
      setFailed(true)
    } finally {
      setLoading(false)
    }
  }

  function toggle() {
    const next = !open()
    setOpen(next)
    setResetCount(null)
    if (next) void load()
  }

  async function reset() {
    setConfirming(false)
    setResetFailed(false)
    try {
      const outcome = parseThreadWorkspaceReset(
        await requestJson<unknown>(threadWorkspacePath(props.threadId), { method: 'DELETE' }),
      )
      setFiles([])
      setResetCount(outcome.filesDeleted)
    } catch {
      setResetFailed(true)
    }
  }

  return (
    <Show when={threadWorkspaceEnabled() && props.threadId}>
      <section aria-label={i18n.tr('Tråds filer', 'Thread files')}>
        <button type="button" aria-expanded={open() ? 'true' : 'false'} onClick={toggle}>
          {i18n.tr('Tråds filer', 'Thread files')}
        </button>
        <Show when={open()}>
          <Show when={!loading()} fallback={<p>{i18n.tr('Laster …', 'Loading …')}</p>}>
            <Show
              when={!failed()}
              fallback={<p role="alert">{i18n.tr('Filene kunne ikke hentes.', 'The files could not be loaded.')}</p>}
            >
              <Show
                when={files().length > 0}
                fallback={<p>{i18n.tr('Ingen filer er lagret i denne tråden.', 'No files are kept for this thread.')}</p>}
              >
                <ul>
                  <For each={files()}>
                    {(file) => (
                      <li>
                        <span>{file.path}</span> <span>{formatBytes(file.sizeBytes)}</span>
                      </li>
                    )}
                  </For>
                </ul>
                <Show
                  when={confirming()}
                  fallback={
                    <button type="button" onClick={() => setConfirming(true)}>
                      {i18n.tr('Nullstill tråds filer', 'Reset thread files')}
                    </button>
                  }
                >
                  <p>
                    {i18n.tr(
                      'Alle filene i denne tråden slettes permanent.',
                      'Every file in this thread is erased for good.',
                    )}
                  </p>
                  <button type="button" onClick={() => void reset()}>
                    {i18n.tr('Bekreft nullstilling', 'Confirm reset')}
                  </button>
                </Show>
              </Show>
            </Show>
          </Show>
          <Show when={resetCount() !== null}>
            <p>{i18n.tr('Tråds filer er nullstilt.', 'Thread files were reset.')}</p>
          </Show>
          <Show when={resetFailed()}>
            <p role="alert">{i18n.tr('Filene kunne ikke nullstilles.', 'The files could not be reset.')}</p>
          </Show>
        </Show>
      </section>
    </Show>
  )
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  return `${(bytes / 1024).toFixed(1)} KB`
}
