/**
 * The present mode of a deck artifact (EP-5 S5): the slides full screen, one at a time, in a dialog over the page.
 *
 * A slide is drawn from the rendered PDF when the render has one page per slide, so page N is slide N. Any other
 * count would put a page under the wrong slide, so the slides are drawn from the manifest at 16:9 instead, and say
 * they are a draft. Either way this is a viewer, not a check: nothing here says the deck was verified in PowerPoint,
 * and a warning does not hold presenting back. Errors and slides that were never built do (EP-5 decision D7).
 */
import type { JSX } from '@solidjs/web'
import { Portal } from '@solidjs/web'
import { For, Show, createEffect, createSignal, onCleanup, untrack } from 'solid-js'
import { ChevronLeft, ChevronRight, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import type { GeneratedFilePreview } from './chat-types'
import { type DeckSlide, type DeckSlideState, slideLines } from './deck-manifest'
import './DeckPresentMode.css'

/** What Tab can land on inside the dialog. The rendered frame is left out: it takes no key presses (see the CSS). */
const FOCUSABLE = 'button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])'

/** What the present gate counts on a deck. Errors and never-built slides hold presenting back; warnings do not. */
export type PresentGate = {
  slides: number
  errors: number
  drafts: number
  warnings: number
  /** True when the deck has a slide and none has an error or has never been built. */
  allowed: boolean
}

export function presentGate(slides: readonly DeckSlide[]): PresentGate {
  const count = (state: DeckSlideState) => slides.filter((slide) => slide.state === state).length
  const errors = count('error')
  const drafts = count('draft')
  return {
    slides: slides.length,
    errors,
    drafts,
    warnings: count('warn'),
    allowed: slides.length > 0 && errors === 0 && drafts === 0,
  }
}

/**
 * The preview whose pages draw the slides, or `undefined` when its page count is not the slide count. A slide is
 * never drawn with another slide's page.
 */
export function pagesForSlides(
  preview: GeneratedFilePreview | undefined,
  slideCount: number,
): GeneratedFilePreview | undefined {
  if (!preview || slideCount <= 0 || preview.pages !== slideCount) return undefined
  // Only a PDF data URI is framed, so the frame is never handed a link of another kind.
  return preview.url.startsWith('data:application/pdf;') ? preview : undefined
}

export type DeckPresentModeProps = {
  /** The slides in order, as the panel holds them, so a newer revision shows while the deck is up. */
  slides: readonly DeckSlide[]
  /** The artifact's title, for the dialog's name and the rendered frame's. */
  title: string
  /** The rendered file, when the turn has one. */
  preview?: GeneratedFilePreview
  /** The slide to open on: the one the strip had picked. */
  startIndex?: number
  onClose: () => void
}

export function DeckPresentMode(props: DeckPresentModeProps): JSX.Element {
  const i18n = useI18n()
  const [index, setIndex] = createSignal(untrack(() => props.startIndex ?? 0))
  const [notesOpen, setNotesOpen] = createSignal(false)
  let dialogRef: HTMLDivElement | undefined
  let closeRef: HTMLButtonElement | undefined
  // The control that opened the presentation gets the focus back when it closes. The caller focuses it first,
  // because Safari does not focus a button on click.
  const opener = document.activeElement as HTMLElement | null

  const count = () => props.slides.length
  const current = () => Math.min(index(), Math.max(0, count() - 1))
  const slide = () => props.slides[current()]
  const pages = () => pagesForSlides(props.preview, count())
  const go = (target: number) => setIndex(Math.max(0, Math.min(target, count() - 1)))

  // Tab stays inside the dialog: from the last control it goes to the first, and back from the first to the last.
  const keepFocusInside = (event: KeyboardEvent) => {
    const focusable = dialogRef ? Array.from(dialogRef.querySelectorAll<HTMLElement>(FOCUSABLE)) : []
    const first = focusable[0]
    const last = focusable[focusable.length - 1]
    if (!first || !last) {
      event.preventDefault()
      return
    }
    const active = document.activeElement
    const outside = !dialogRef?.contains(active)
    if (event.shiftKey) {
      if (active !== first && !outside) return
      event.preventDefault()
      last.focus()
    } else {
      if (active !== last && !outside) return
      event.preventDefault()
      first.focus()
    }
  }

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Tab') {
      keepFocusInside(event)
      return
    }
    // Ctrl, Cmd and Alt belong to the browser and the system, not to the presentation.
    if (event.ctrlKey || event.metaKey || event.altKey) return
    switch (event.key) {
      case 'Escape':
        event.preventDefault()
        // The page closes its artifact panel on Escape too; stopping here means one Escape closes one thing.
        event.stopPropagation()
        props.onClose()
        return
      case 'ArrowRight':
      case 'ArrowDown':
      case 'PageDown':
        event.preventDefault()
        go(current() + 1)
        return
      case 'ArrowLeft':
      case 'ArrowUp':
      case 'PageUp':
        event.preventDefault()
        go(current() - 1)
        return
      case 'Home':
        event.preventDefault()
        go(0)
        return
      case 'End':
        event.preventDefault()
        go(count() - 1)
        return
      case 'n':
      case 'N':
        if (slide()?.notes) setNotesOpen(!notesOpen())
        return
      default:
        return
    }
  }

  // Capture on the document, so the presentation sees each key first, wherever the focus is.
  document.addEventListener('keydown', onKeyDown, true)
  onCleanup(() => {
    document.removeEventListener('keydown', onKeyDown, true)
    if (opener?.isConnected) opener.focus()
  })
  createEffect(() => undefined, () => closeRef?.focus())

  return (
    <Portal>
      <div
        ref={dialogRef}
        class="verevon-deck-present"
        role="dialog"
        aria-modal="true"
        aria-label={i18n.tr(`Presentasjon: ${props.title}`, `Presentation: ${props.title}`)}
      >
        <div class="verevon-deck-present__bar">
          <p class="verevon-deck-present__counter" aria-live="polite">
            {count() > 0 ? `${current() + 1} / ${count()}` : ''}
          </p>
          <button
            ref={closeRef}
            type="button"
            class="verevon-deck-present__icon"
            aria-label={i18n.tr('Lukk presentasjon', 'Close presentation')}
            onClick={() => props.onClose()}
          >
            <X size={18} />
          </button>
        </div>
        <div class="verevon-deck-present__stage">
          <Show
            when={slide()}
            fallback={<p class="verevon-deck-present__empty">{i18n.tr('Ingen lysbilder ennå', 'No slides yet')}</p>}
          >
            {(drawn) => (
              <Show
                when={pages()}
                fallback={(
                  <article class="verevon-deck-present__slide">
                    <p class="verevon-deck-present__draft">{i18n.tr('Utkast fra manifest', 'Draft from manifest')}</p>
                    <h2 class="verevon-deck-present__title">{drawn().title}</h2>
                    <ul class="verevon-deck-present__lines">
                      <For each={slideLines(drawn())}>{(line) => <li>{line}</li>}</For>
                    </ul>
                  </article>
                )}
              >
                {(preview) => (
                  <iframe
                    class="verevon-deck-present__page"
                    src={`${preview().url}#page=${current() + 1}`}
                    title={i18n.tr(`Forhåndsvisning av ${props.title}`, `Preview of ${props.title}`)}
                    referrerpolicy="no-referrer"
                    tabindex="-1"
                  />
                )}
              </Show>
            )}
          </Show>
        </div>
        <Show when={notesOpen() ? slide()?.notes : null}>
          {(notes) => (
            <section class="verevon-deck-present__notes" aria-label={i18n.tr('Notater', 'Notes')}>
              <p>{notes()}</p>
            </section>
          )}
        </Show>
        <div class="verevon-deck-present__controls">
          <button
            type="button"
            class="verevon-deck-present__step"
            disabled={current() <= 0}
            aria-label={i18n.tr('Forrige lysbilde', 'Previous slide')}
            onClick={() => go(current() - 1)}
          >
            <ChevronLeft size={20} />
          </button>
          <Show when={slide()?.notes}>
            <button
              type="button"
              class="verevon-deck-present__notes-toggle"
              aria-expanded={notesOpen() ? 'true' : 'false'}
              onClick={() => setNotesOpen(!notesOpen())}
            >
              {notesOpen() ? i18n.tr('Skjul notater (N)', 'Hide notes (N)') : i18n.tr('Vis notater (N)', 'Show notes (N)')}
            </button>
          </Show>
          <button
            type="button"
            class="verevon-deck-present__step"
            disabled={current() >= count() - 1}
            aria-label={i18n.tr('Neste lysbilde', 'Next slide')}
            onClick={() => go(current() + 1)}
          >
            <ChevronRight size={20} />
          </button>
        </div>
        <p class="verevon-deck-present__hint">
          {i18n.tr(
            'Piltene, PageUp og PageDown bytter lysbilde · N viser notater · Esc lukker',
            'Arrow keys, PageUp and PageDown change slide · N shows notes · Esc closes',
          )}
        </p>
      </div>
    </Portal>
  )
}
