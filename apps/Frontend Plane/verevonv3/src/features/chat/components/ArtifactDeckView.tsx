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
import { For, Show, createEffect, createMemo, createSignal, createUniqueId, useContext } from 'solid-js'
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
  Maximize2,
  Palette,
  Presentation,
  Type,
} from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import type { DeckTemplate } from '@/shared/api/chat-client'
import { ArtifactOfficePreview } from './ArtifactOfficePreview'
import { DeckTemplateContext, type ArtifactRenderer, type ArtifactRendererProps } from './artifact-renderers'
import type { IconComponent } from './chat-types'
import {
  DECK_ARTIFACT_KIND,
  type DeckSlideLayout,
  type DeckSlideState,
  parseDeckManifest,
  slideLines,
} from './deck-manifest'
import { DeckPresentMode, presentGate } from './DeckPresentMode'
import { DeckTemplatePicker } from './DeckTemplatePicker'

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
  const [presenting, setPresenting] = createSignal(false)
  let presenterRef: HTMLButtonElement | undefined
  const reasonId = `verevon-deck-present-reason-${createUniqueId()}`
  // "Bytt mal" (EP-5 S7): offered only when the chat page provides the templates and the gateway lists at least one.
  // The deck's own template is not in its manifest, so nothing is shown as current until this view makes a pick ('').
  const templates = useContext(DeckTemplateContext)
  const [templatesOffered, setTemplatesOffered] = createSignal(false)
  const [templatePickerOpen, setTemplatePickerOpen] = createSignal(false)
  const [templatePick, setTemplatePick] = createSignal<string | null>('')
  const templatesBusy = () => templates?.busy() === true
  // Solid v2 has no onMount: a two-phase effect with a constant compute runs once after mount.
  createEffect(() => undefined, () => {
    let live = true
    templates?.list()
      .then((listed) => {
        if (live) setTemplatesOffered((listed?.length ?? 0) > 0)
      })
      .catch(() => {
        if (live) setTemplatesOffered(false)
      })
    return () => {
      live = false
    }
  })
  const pickTemplate = (template: DeckTemplate | null) => {
    setTemplatePick(template?.skillId ?? null)
    setTemplatePickerOpen(false)
    templates?.apply(template)
  }
  // A newer revision can have fewer slides than the pick, so the pick stays on a slide that exists.
  const current = createMemo(() => Math.min(pickedIndex(), Math.max(0, (manifest()?.slides.length ?? 0) - 1)))
  const selected = createMemo(() => manifest()?.slides[current()])
  const gate = createMemo(() => presentGate(manifest()?.slides ?? []))

  // Why Presenter is off: the blockers by count, then what to do about them. Slides are changed in the chat.
  const blockReason = () => {
    const counts = gate()
    if (counts.slides === 0) {
      return i18n.tr('En presentasjon trenger minst ett lysbilde.', 'A presentation needs at least one slide.')
    }
    const lines: string[] = []
    if (counts.errors > 0) {
      lines.push(counts.errors === 1
        ? i18n.tr('1 lysbilde har feil.', '1 slide has an error.')
        : i18n.tr(`${counts.errors} lysbilder har feil.`, `${counts.errors} slides have errors.`))
    }
    if (counts.drafts > 0) {
      lines.push(counts.drafts === 1
        ? i18n.tr('1 lysbilde er ikke bygget.', '1 slide is not built.')
        : i18n.tr(`${counts.drafts} lysbilder er ikke bygget.`, `${counts.drafts} slides are not built.`))
    }
    lines.push(i18n.tr(
      'Be om å rette og bygge lysbildene i chatten før du presenterer.',
      'Ask in the chat to fix and build the slides before you present.',
    ))
    return lines.join(' ')
  }
  // A warning never holds presenting back, so it is counted here and not refused.
  const warningLine = () => {
    const warnings = gate().warnings
    return warnings === 1
      ? i18n.tr('1 lysbilde har advarsler', '1 slide has warnings')
      : i18n.tr(`${warnings} lysbilder har advarsler`, `${warnings} slides have warnings`)
  }
  // A picture goes on a card only when the render made one per slide, in slide order. A render whose page count
  // differs from the slides, or that kept only some of its pages, would put a picture on the wrong card or on none.
  const pictures = createMemo<string[]>(() => {
    const preview = props.file?.preview
    const slides = manifest()?.slides.length ?? 0
    return preview && preview.pages === slides && preview.thumbs.length >= slides ? preview.thumbs : []
  })
  const cardClass = () => `verevon-deck__card${pictures().length > 0 ? ' verevon-deck__card--picture' : ''}`

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
                      class={cardClass()}
                      aria-current={index() === current() ? 'true' : undefined}
                      onClick={() => {
                        setPickedIndex(index())
                        setNotesOpen(false)
                      }}
                    >
                      <Show when={pictures()[index()]}>
                        {(picture) => <img class="verevon-deck__thumb" src={picture()} alt="" loading="lazy" />}
                      </Show>
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
          <div class="verevon-deck__present">
            <Show when={gate().warnings > 0}>
              <p class="verevon-deck__warned" role="status">{warningLine()}</p>
            </Show>
            <button
              ref={presenterRef}
              type="button"
              class="verevon-deck__present-button"
              disabled={!gate().allowed}
              aria-describedby={gate().allowed ? undefined : reasonId}
              onClick={() => {
                // Focused first, so the presentation can give the focus back to this button when it closes.
                presenterRef?.focus()
                setPresenting(true)
              }}
            >
              <Maximize2 size={14} />
              {i18n.tr('Presenter', 'Present')}
            </button>
            <Show when={!gate().allowed}>
              <p id={reasonId} class="verevon-deck__reason">{blockReason()}</p>
            </Show>
          </div>
          <Show when={templates && templatesOffered()}>
            <div class="verevon-deck__template">
              <button
                type="button"
                class="verevon-deck__toggle"
                aria-expanded={templatePickerOpen() ? 'true' : 'false'}
                disabled={templatesBusy()}
                onClick={() => setTemplatePickerOpen(!templatePickerOpen())}
              >
                <Palette size={14} />
                {i18n.tr('Bytt mal', 'Change template')}
              </button>
              <Show when={templatePickerOpen()}>
                <DeckTemplatePicker selected={templatePick()} disabled={templatesBusy()} onSelect={pickTemplate} />
              </Show>
            </div>
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
          <Show when={presenting()}>
            <DeckPresentMode
              slides={deck().slides}
              title={props.artifact.title}
              preview={props.file?.preview}
              startIndex={current()}
              onClose={() => setPresenting(false)}
            />
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
