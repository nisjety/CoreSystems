import { Code2, FileText, Globe } from '@/shared/icons'
import { Show } from 'solid-js'

import type { ChatArtifact } from '@/features/chat/components/chat-types'
import { useI18n } from '@/shared/i18n'
import './space-artifacts.css'

/**
 * One agent artifact, as a chip under the turn that produced it: icon, title,
 * kind and revision. Pressing it opens the per-thread sheet. The chip carries
 * no content; it states what exists and where to look.
 */
export function SpaceArtifactChip(props: {
  readonly artifact: ChatArtifact
  readonly onOpen: (artifactId: string) => void
}) {
  const i18n = useI18n()
  const kindLabel = () => {
    switch (props.artifact.kind) {
      case 'code':
        return i18n.tr('Kode', 'Code')
      case 'html':
        return 'HTML'
      default:
        return i18n.tr('Dokument', 'Document')
    }
  }
  const title = () => props.artifact.title.trim() || i18n.tr('Uten tittel', 'Untitled')
  return (
    <button
      type="button"
      class="verevon-space-artifact-chip"
      aria-label={i18n.tr(
        `Åpne artefakt ${title()}, versjon ${props.artifact.version}`,
        `Open artifact ${title()}, version ${props.artifact.version}`,
      )}
      onClick={() => props.onOpen(props.artifact.id)}
    >
      <span class="verevon-space-artifact-chip__icon" aria-hidden="true">
        <Show
          when={props.artifact.kind === 'code'}
          fallback={
            <Show when={props.artifact.kind === 'html'} fallback={<FileText size={14} />}>
              <Globe size={14} />
            </Show>
          }
        >
          <Code2 size={14} />
        </Show>
      </span>
      <span class="verevon-space-artifact-chip__title">{title()}</span>
      <span class="verevon-space-artifact-chip__meta">
        {kindLabel()} · v{props.artifact.version}
      </span>
    </button>
  )
}
