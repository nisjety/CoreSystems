import { For, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import type { ChangedFile } from '../lib/changed-files'
import './ChangedFilesCard.css'

/**
 * `VITE_CHAT_CHANGED_FILES=true` mounts the changed-files card under each answer (F-t3-076). Unset, or any value but
 * `true` (the default), no answer shows a card. Exact string, as the other chat switches.
 */
export const chatChangedFilesEnabled = () => import.meta.env.VITE_CHAT_CHANGED_FILES === 'true'

/**
 * The changed-files card (F-t3-076): the files a run saved to its workspace, and whether each was new or edited. It
 * reads and changes nothing. There is no accept or promote control, because nothing moves an overlay into the Space
 * yet (DEC-24), so the note says the change belongs to this run alone. It renders nothing for an empty list.
 */
export function ChangedFilesCard(props: { files: readonly ChangedFile[] }) {
  const i18n = useI18n()
  const count = () => props.files.length

  return (
    <Show when={count() > 0}>
      <section class="verevon-changed-files" aria-label={i18n.tr('Endrede filer', 'Changed files')}>
        <div class="verevon-changed-files__head">
          <strong>{i18n.tr('Endrede filer', 'Changed files')}</strong>
          <span>{count() === 1 ? i18n.tr('1 fil', '1 file') : i18n.tr(`${count()} filer`, `${count()} files`)}</span>
        </div>
        <ul class="verevon-changed-files__list">
          <For each={props.files}>
            {(file) => (
              <li class="verevon-changed-files__item" data-change={file.created ? 'created' : 'edited'}>
                <span class="verevon-changed-files__path">{file.path}</span>
                <span class="verevon-changed-files__kind">
                  {file.created ? i18n.tr('Ny', 'New') : i18n.tr('Endret', 'Edited')}
                </span>
                <Show when={file.saves > 1}>
                  <span class="verevon-changed-files__saves">
                    {i18n.tr(`${file.saves} endringer`, `${file.saves} changes`)}
                  </span>
                </Show>
              </li>
            )}
          </For>
        </ul>
        <p class="verevon-changed-files__note">
          {i18n.tr(
            'Lagret bare for denne kjøringen. Andre kjøringer i arbeidsområdet ser ikke endringene ennå.',
            'Saved for this run only. Other runs in the workspace do not see these changes yet.',
          )}
        </p>
      </section>
    </Show>
  )
}
