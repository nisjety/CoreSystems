/**
 * The Office previewer in the artifact panel. A generated Word, Excel or
 * PowerPoint file is shown as the PDF the render layer made of it, with a strip
 * of its first pages and a notice when a font was drawn with a stand-in. The
 * download is still the file the sandbox wrote, so this view never replaces it.
 * A file without a preview keeps the panel's own file card.
 */
import type { JSX } from '@solidjs/web'
import { For, Show, createSignal } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import type { ArtifactRenderer, ArtifactRendererProps } from './artifact-renderers'
import type { ChatArtifact, GeneratedFilePreview } from './chat-types'

const OFFICE_OPENXML_PREFIX = 'data:application/vnd.openxmlformats-officedocument.'

/**
 * Claims a generated Word, Excel or PowerPoint file. Whether there is a preview
 * to draw is decided by the component, which is given the file.
 */
function isOfficeDeliverable(artifact: ChatArtifact): boolean {
  return artifact.content.startsWith(OFFICE_OPENXML_PREFIX)
}

export function ArtifactOfficePreview(props: { title: string; preview: GeneratedFilePreview }): JSX.Element {
  const i18n = useI18n()
  const [page, setPage] = createSignal(1)
  // The PDF viewer opens the page named in the fragment, so picking a thumbnail moves the frame.
  const frameSrc = () => `${props.preview.url}#page=${page()}`
  return (
    <div class="verevon-chat-artifact-office">
      <Show when={props.preview.fontsSubstituted}>
        <p class="verevon-chat-artifact-office__notice" role="note">
          {i18n.tr('Skrift erstattet i forhåndsvisningen', 'Fonts substituted in preview')}
        </p>
      </Show>
      <Show when={props.preview.thumbs.length > 0}>
        <ol
          class="verevon-chat-artifact-office__pages"
          aria-label={i18n.tr('Sider i forhåndsvisningen', 'Pages in the preview')}
        >
          <For each={props.preview.thumbs}>
            {(thumb, index) => (
              <li>
                <button
                  type="button"
                  aria-current={page() === index() + 1 ? 'page' : undefined}
                  aria-label={i18n.tr(`Side ${index() + 1}`, `Page ${index() + 1}`)}
                  onClick={() => setPage(index() + 1)}
                >
                  <img src={thumb} alt="" loading="lazy" />
                </button>
              </li>
            )}
          </For>
        </ol>
      </Show>
      <div class="verevon-chat-artifact-frame">
        <iframe
          src={frameSrc()}
          title={i18n.tr(`Forhåndsvisning av ${props.title}`, `Preview of ${props.title}`)}
          referrerpolicy="no-referrer"
        />
      </div>
    </div>
  )
}

/**
 * The panel's extra viewer for generated Office files. It draws the preview of
 * the file when there is one, and the panel's own body otherwise.
 */
export const officePdfRenderer: ArtifactRenderer<'office-pdf'> = {
  id: 'office-pdf',
  priority: 0,
  match: isOfficeDeliverable,
  component: OfficePdfRenderer,
}

function OfficePdfRenderer(props: ArtifactRendererProps): JSX.Element {
  return (
    <Show when={props.file?.preview} fallback={props.fallback?.()}>
      {(preview) => <ArtifactOfficePreview title={props.artifact.title} preview={preview()} />}
    </Show>
  )
}
