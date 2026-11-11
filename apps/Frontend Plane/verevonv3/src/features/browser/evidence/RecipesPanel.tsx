import { For, Show, createEffect, createSignal } from 'solid-js'
import type { JSX } from '@solidjs/web'
import {
  browserErrorMessage,
  type BrowserRecipeDetail,
  type BrowserRecipeSlot,
  type BrowserSavedRecipe,
} from '@/shared/api/browser-client'
import { useI18n } from '@/shared/i18n'
import { formatMoment } from '@/shared/i18n/format'
import {
  recipesEnabled,
  useRecipeDetail,
  useRecipeRun,
  useSavedRecipes,
  type BrowserRecipeSaved,
} from './use-browser-evidence'
import './browser-evidence.css'

/** The gateway's slot bounds: a name `^[a-z0-9_]{1,40}$`, at most 32 slots, a value of at most 2000 characters. */
const SLOT_NAME_PATTERN = /^[a-z0-9_]{1,40}$/
const RECIPE_SLOTS_MAX = 32
const RECIPE_SLOT_VALUE_MAX_CHARS = 2000

/**
 * The slots a chosen recipe runs with (QF-29, from its detail read), or null when this panel cannot
 * start it: too many slots, a name outside the gateway's bound, or a kind it does not know.
 */
function runnableSlots(detail: BrowserRecipeDetail | null): BrowserRecipeSlot[] | null {
  const slots = detail?.slots
  if (!Array.isArray(slots) || slots.length > RECIPE_SLOTS_MAX) return null
  const kept: BrowserRecipeSlot[] = []
  for (const slot of slots) {
    if (
      !slot ||
      (slot.kind !== 'text' && slot.kind !== 'secret') ||
      typeof slot.name !== 'string' ||
      !SLOT_NAME_PATTERN.test(slot.name)
    )
      return null
    kept.push({ name: slot.name, kind: slot.kind })
  }
  return kept
}

/**
 * The saved browser recipes under the Procedure panel (Q4-E4.2-4d, QF-29): the person's recipes.
 * Choosing one reads its detail (the newest version's slot names and kinds) and shows one text
 * input per text slot and a masked input per secret slot, with Run. Only the literal `true` of
 * VITE_BROWSER_RECIPES builds it. A ZDR session still lists the recipes, but Run and the inputs
 * are hidden. A run starts on the person's click alone, pinned to the version the detail read. The
 * slot values live in this panel's memory only: never prefilled, stored or logged, sent with the
 * start, and cleared once the start is accepted, when another recipe is chosen, or when the session
 * changes.
 */
export function RecipesPanel(props: {
  sessionId: string
  zdr: boolean
  /** The recipe a save just wrote. The list is read again when it changes. */
  savedRecipe?: BrowserRecipeSaved | null
}): JSX.Element {
  return (
    <Show when={recipesEnabled()}>
      <RecipesList sessionId={props.sessionId} zdr={props.zdr} savedRecipe={props.savedRecipe} />
    </Show>
  )
}

function RecipesList(props: { sessionId: string; zdr: boolean; savedRecipe?: BrowserRecipeSaved | null }): JSX.Element {
  const i18n = useI18n()
  const list = useSavedRecipes()
  const run = useRecipeRun()
  const detail = useRecipeDetail()
  // The recipe the person chose. Only it shows slot inputs and Run.
  const [chosen, setChosen] = createSignal<string | null>(null)
  // The typed slot values of the chosen recipe. They stay in this component's memory only.
  const [values, setValues] = createSignal<Record<string, string>>({})
  // The recipe the last start was for, so that only its Run button reads as starting.
  const [startedRecipe, setStartedRecipe] = createSignal<string | null>(null)
  const busy = () => run.status() === 'starting' || run.status() === 'live'

  createEffect(
    () => props.savedRecipe,
    (saved) => {
      if (saved) list.reload()
    },
  )
  createEffect(
    () => props.sessionId,
    () => {
      run.reset()
      detail.clear()
      setChosen(null)
      setValues({})
      setStartedRecipe(null)
    },
  )

  /** The chosen recipe's slots, once its detail has arrived for that recipe. */
  const chosenSlots = (recipeId: string): BrowserRecipeSlot[] | null => {
    const read = detail.detail()
    if (chosen() !== recipeId || detail.status() !== 'ready' || !read || read.recipeId !== recipeId) return null
    return runnableSlots(read)
  }

  const choose = (recipe: BrowserSavedRecipe) => {
    if (busy()) return
    // A new choice drops every typed value, secret ones included.
    setValues({})
    if (chosen() === recipe.recipeId) {
      setChosen(null)
      detail.clear()
      return
    }
    setChosen(recipe.recipeId)
    detail.load(recipe.recipeId)
  }

  const valueOf = (name: string): string => values()[name] ?? ''
  const setValue = (name: string, value: string) => setValues((current) => ({ ...current, [name]: value }))

  const slotValue = (slot: BrowserRecipeSlot): string => {
    const raw = valueOf(slot.name)
    // A secret is sent as typed; a text value is trimmed.
    return slot.kind === 'secret' ? raw : raw.trim()
  }

  const canRun = (recipe: BrowserSavedRecipe, slots: BrowserRecipeSlot[] | null): boolean =>
    !props.zdr &&
    !busy() &&
    props.sessionId !== '' &&
    slots !== null &&
    detail.detail() !== null &&
    slots.every((slot) => slotValue(slot) !== '') &&
    chosen() === recipe.recipeId

  const startRecipe = async (recipe: BrowserSavedRecipe, slots: BrowserRecipeSlot[]) => {
    const read = detail.detail()
    if (busy() || props.zdr || props.sessionId === '' || !read || read.recipeId !== recipe.recipeId) return
    const sent: Record<string, string> = {}
    for (const slot of slots) sent[slot.name] = slotValue(slot)
    setStartedRecipe(recipe.recipeId)
    const started = await run.runRecipe(props.sessionId, {
      id: recipe.recipeId,
      name: read.name ?? recipe.name,
      version: read.version,
      slots: sent,
    })
    if (started) setValues({})
  }

  const labelFor = (recipeId: string): string => {
    if (!busy() || startedRecipe() !== recipeId) return i18n.tr('Kjør', 'Run')
    return run.status() === 'live' ? i18n.tr('Kjører…', 'Running…') : i18n.tr('Starter…', 'Starting…')
  }

  const listFailure = () => {
    const failure = list.error()
    if (!failure) return null
    return browserErrorMessage(failure, i18n.tr, {
      no: 'Oppskriftene kunne ikke lastes.',
      en: 'The recipes could not be loaded.',
    })
  }
  const detailFailure = () => {
    const failure = detail.error()
    if (!failure || chosen() === null) return null
    return browserErrorMessage(failure, i18n.tr, {
      no: 'Feltene for oppskriften kunne ikke lastes.',
      en: "The recipe's inputs could not be loaded.",
    })
  }
  const runFailure = () => {
    const failure = run.error()
    if (!failure) return null
    return browserErrorMessage(failure, i18n.tr, {
      no: 'Oppskriften kunne ikke startes eller følges.',
      en: 'The recipe could not be started or followed.',
    })
  }

  return (
    <section class="browser-evidence-recipe" aria-label={i18n.tr('Lagrede oppskrifter', 'Saved recipes')}>
      <p class="browser-evidence-panel__hint">{i18n.tr('Lagrede oppskrifter', 'Saved recipes')}</p>
      <Show when={props.zdr}>
        <span class="browser-evidence-chip browser-evidence-chip--zdr">
          {i18n.tr('Kjøring er ikke tilgjengelig for ZDR-økter', 'Running is not available for ZDR sessions')}
        </span>
      </Show>
      <Show when={listFailure()}>
        {(message) => (
          <p class="browser-evidence-panel__hint" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={list.status() === 'loading' && list.recipes().length === 0}>
        <p class="browser-evidence-panel__hint">{i18n.tr('Laster oppskrifter…', 'Loading recipes…')}</p>
      </Show>
      <Show when={list.status() === 'ready' && list.recipes().length === 0}>
        <p class="browser-evidence-panel__hint">{i18n.tr('Ingen lagrede oppskrifter ennå.', 'No saved recipes yet.')}</p>
      </Show>
      <ol class="browser-evidence-list">
        <For each={list.recipes()}>
          {(recipe) => {
            const isChosen = () => chosen() === recipe.recipeId
            const slots = () => chosenSlots(recipe.recipeId)
            const slotSummary = (): string => {
              if (!isChosen()) return ''
              if (detail.status() === 'loading') return i18n.tr('Laster felt…', 'Loading slots…')
              const known = slots()
              if (detail.status() === 'ready' && known === null) return i18n.tr('felt ukjent', 'slots unknown')
              return known === null ? '' : `${known.length} ${i18n.tr('felt', 'slots')}`
            }
            return (
              <li>
                <div class="browser-evidence-row">
                  <span class="browser-evidence-row__method">v{recipe.latestVersion}</span>
                  <button
                    type="button"
                    class="browser-evidence-row__url"
                    title={recipe.name}
                    aria-expanded={isChosen() ? 'true' : 'false'}
                    disabled={busy()}
                    onClick={() => choose(recipe)}
                  >
                    {recipe.name}
                  </button>
                  <span class="browser-evidence-row__policy">{slotSummary()}</span>
                  <time class="browser-evidence-row__time" datetime={recipe.updatedAt}>
                    {formatMoment(recipe.updatedAt, i18n.locale())}
                  </time>
                  <Show when={!props.zdr && isChosen() && slots() !== null}>
                    <button
                      type="button"
                      class="browser-evidence-panel__action"
                      disabled={!canRun(recipe, slots())}
                      onClick={() => {
                        const known = slots()
                        if (known) void startRecipe(recipe, known)
                      }}
                    >
                      {labelFor(recipe.recipeId)}
                    </button>
                  </Show>
                </div>
                <Show when={!props.zdr && isChosen() && (slots()?.length ?? 0) > 0}>
                  <div class="browser-evidence-recipe">
                    <For each={slots() ?? []}>
                      {(slot) => (
                        <label class="browser-evidence-recipe__label">
                          <span>{slot.name}</span>
                          <input
                            type={slot.kind === 'secret' ? 'password' : 'text'}
                            class="browser-evidence-recipe__input"
                            maxlength={RECIPE_SLOT_VALUE_MAX_CHARS}
                            autocomplete={slot.kind === 'secret' ? 'new-password' : 'off'}
                            spellcheck={false}
                            value={valueOf(slot.name)}
                            disabled={busy()}
                            onInput={(event) => setValue(slot.name, event.currentTarget.value)}
                          />
                        </label>
                      )}
                    </For>
                  </div>
                </Show>
              </li>
            )
          }}
        </For>
      </ol>
      <Show when={detailFailure()}>
        {(message) => (
          <p class="browser-evidence-panel__hint" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={list.nextAfterId()}>
        <button
          type="button"
          class="browser-evidence-panel__action"
          disabled={list.status() === 'loading'}
          onClick={() => list.loadMore()}
        >
          {i18n.tr('Vis flere', 'Show more')}
        </button>
      </Show>
      <Show when={runFailure()}>
        {(message) => (
          <p class="browser-evidence-panel__hint" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={run.status() === 'live'}>
        <p class="browser-evidence-recipe__saved" role="status">
          {i18n.tr('Kjøringen er i gang.', 'The run is under way.')}
        </p>
      </Show>
      <Show when={run.status() === 'done'}>
        <p class="browser-evidence-recipe__saved" role="status">
          {i18n.tr('Kjøringen er avsluttet. Se kjøringen for resultatet.', 'The run has ended. See the run for the result.')}
        </p>
      </Show>
    </section>
  )
}
