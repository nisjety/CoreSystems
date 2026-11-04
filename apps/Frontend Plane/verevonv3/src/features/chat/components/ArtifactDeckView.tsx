/**
 * The slide strip of a deck artifact (EP-5 S4): one 16:9 card per slide, drawn from the manifest the model
 * gateway keeps, with the lines, state, findings and speaker notes of the slide the person picks. It is the
 * panel's extra viewer for `deck` artifacts, so the header and the built-in kinds stay as they are.
 *
 * The strip is a draft until the deck has been rendered, so it says so: a card shows the manifest, not the
 * final look. Slide text is model-authored and is drawn as text only; no picture is loaded from a slide's
 * path. The generated file's own card, and its rendered preview when the file has one, sit below the strip.
 */
import type { JSX } from '@solidjs/web'
import { Dynamic } from '@solidjs/web'
import { For, Show, createMemo, createSignal } from 'solid-js'
import {
  AlertTriangle,
  ChevronDown,
  ChevronUp,
  CircleCheck,
  CircleDashed,
  CircleX,
  Columns3,
  Image as ImageIcon,
  LayoutTemplate,
  List,
  Presentation,
  Type,
} from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { ArtifactOfficePreview } from './ArtifactOfficePreview'
import type { ArtifactRenderer, ArtifactRendererProps } from './artifact-renderers'
import type { IconComponent } from './chat-types'
import {
  DECK_ARTIFACT_KIND,
  type DeckSlideLayout,
  type DeckSlideState,
  parseDeckManifest,
  slideLines,
} from './deck-manifest'

type Translate = (noText: string, enText: string) => string

const LAYOUT_ICONS: Record<DeckSlideLayout, IconComponent> = {
  title: Presentation,
  title_bullets: List,
  title_only: Type,
  section: LayoutTemplate,
  two_column: Columns3,
  image: ImageIcon,
}

const STATE_ICONS: Record<DeckSlideState, IconComponent> = {
  draft: CircleDashed,
  ok: CircleCheck,
  warn: AlertTriangle,
  error: CircleX,
}

/** The word on a slide's chip. A draft has never been built, or has changed since its last build. */
function stateLabel(state: DeckSlideState, tr: Translate): string {
  switch (state) {
    case 'draft':
      return tr('Ikke bygget', 'Not built')
    case 'ok':
      return tr('OK', 'OK')
    case 'warn':
      return tr('Advarsel', 'Warning')
    case 'error':
      return tr('Feil', 'Error')
  }
}

export function ArtifactDeckView(props: ArtifactRendererProps): JSX.Element {
  const i18n = useI18n()
  const manifest = createMemo(() => parseDeckManifest(props.artifact.content))
  const [pickedIndex, setPickedIndex] = createSignal(0)
  const [notesOpen, setNotesOpen] = createSignal(false)
  // A newer revision can have fewer slides than the pick, so the pick stays on a slide that exists.
  const current = createMemo(() => Math.min(pickedIndex(), Math.max(0, (manifest()?.slides.length ?? 0) - 1)))
  const selected = createMemo(() => manifest()?.slides[current()])

  return (
    <Show
      when={manifest()}
      fallback={(
        <div class="verevon-deck">
          <p class="verevon-deck__notice" role="status">
            {i18n.tr('Kunne ikke lese lysbildene', 'Could not read the slides')}
          </p>
          <pre class="verevon-deck__raw"><code>{props.artifact.content}</code></pre>
        </div>
      )}
    >
      {(deck) => (
        <div class="verevon-deck">
          <Show when={!props.file?.preview}>
            <p class="verevon-deck__draft">{i18n.tr('Utkast fra manifest', 'Draft from manifest')}</p>
          </Show>
          <Show
            when={deck().slides.length > 0}
            fallback={<p class="verevon-deck__notice">{i18n.tr('Ingen lysbilder ennå', 'No slides yet')}</p>}
          >
            <ol class="verevon-deck__strip" aria-label={i18n.tr('Lysbilder', 'Slides')}>
              <For each={deck().slides}>
                {(slide, index) => (
                  <li>
                    <button
                      type="button"
                      class="verevon-deck__card"
                      aria-current={index() === current() ? 'true' : undefined}
                      onClick={() => {
                        setPickedIndex(index())
                        setNotesOpen(false)
                      }}
                    >
                      <span class="verevon-deck__head">
                        <span class="verevon-deck__number">{index() + 1}</span>
                        <Dynamic component={LAYOUT_ICONS[slide.layout]} size={14} />
                        <span class={`verevon-deck__chip verevon-deck__chip--${slide.state}`}>
                          <Dynamic component={STATE_ICONS[slide.state]} size={12} />
                          {stateLabel(slide.state, i18n.tr)}
                        </span>
                      </span>
                      <span class="verevon-deck__title">{slide.title}</span>
                      <For each={slideLines(slide).slice(0, 2)}>
                        {(line) => <span class="verevon-deck__line">{line}</span>}
                      </For>
                    </button>
                  </li>
                )}
              </For>
            </ol>
          </Show>
          <Show when={selected()}>
            {(slide) => (
              <div class="verevon-deck__detail">
                <Show when={slideLines(slide()).length > 0}>
                  <ul class="verevon-deck__lines" aria-label={i18n.tr('Innhold på lysbildet', 'Slide content')}>
                    <For each={slideLines(slide())}>{(line) => <li>{line}</li>}</For>
                  </ul>
                </Show>
                <Show
                  when={slide().findings.length > 0}
                  fallback={(
                    <p class="verevon-deck__none">
                      {i18n.tr('Ingen funn på dette lysbildet', 'No findings on this slide')}
                    </p>
                  )}
                >
                  <ul class="verevon-deck__findings" aria-label={i18n.tr('Funn', 'Findings')}>
                    <For each={slide().findings}>{(finding) => <li>{finding.message}</li>}</For>
                  </ul>
                </Show>
                <Show when={slide().notes}>
                  <button
                    type="button"
                    class="verevon-deck__toggle"
                    aria-expanded={notesOpen() ? 'true' : 'false'}
                    onClick={() => setNotesOpen(!notesOpen())}
                  >
                    <Dynamic component={notesOpen() ? ChevronUp : ChevronDown} size={14} />
                    {notesOpen() ? i18n.tr('Skjul notater', 'Hide notes') : i18n.tr('Vis notater', 'Show notes')}
                  </button>
                  <Show when={notesOpen()}>
                    <p class="verevon-deck__notes">{slide().notes}</p>
                  </Show>
                </Show>
              </div>
            )}
          </Show>
          {props.fallback?.()}
          <Show when={props.file?.preview}>
            {(preview) => <ArtifactOfficePreview title={props.artifact.title} preview={preview()} />}
          </Show>
        </div>
      )}
    </Show>
  )
}

/**
 * The panel's extra viewer for `deck` artifacts. It takes the body of the artifact; the header stays on the
 * built-in kind, as it does for every extra viewer.
 */
export const deckManifestRenderer: ArtifactRenderer<'deck-manifest'> = {
  id: 'deck-manifest',
  priority: 0,
  match: (artifact) => artifact.kind === DECK_ARTIFACT_KIND,
  component: ArtifactDeckView,
}
