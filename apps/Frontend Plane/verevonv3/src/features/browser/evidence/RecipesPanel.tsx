import { For, Show, createEffect, createSignal } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { browserErrorMessage, type BrowserSavedRecipe } from '@/shared/api/browser-client'
import { useI18n } from '@/shared/i18n'
import { formatMoment } from '@/shared/i18n/format'
import { recipesEnabled, useRecipeRun, useSavedRecipes, type BrowserRecipeSaved } from './use-browser-evidence'
import './browser-evidence.css'

/** The gateway's slot bounds: a name `^[a-z0-9_]{1,40}$`, at most 32 slots, a value of at most 2000 characters. */
const SLOT_NAME_PATTERN = /^[a-z0-9_]{1,40}$/
const RECIPE_SLOTS_MAX = 32
const RECIPE_SLOT_VALUE_MAX_CHARS = 2000

/**
 * The slot names a recipe runs with, or null when this panel cannot start it. The list does not
 * send the slots yet (see `BrowserSavedRecipe.slots`), so every recipe reads as unknown until it
 * does. A secret slot is also unknown here: execution-core refuses a secret slot, and a person
 * supplies it by takeover, not in this panel.
 */
function runnableSlots(recipe: BrowserSavedRecipe): string[] | null {
  const slots = recipe.slots
  if (!Array.isArray(slots) || slots.length > RECIPE_SLOTS_MAX) return null
  const names: string[] = []
  for (const slot of slots) {
    if (!slot || slot.kind !== 'text' || typeof slot.name !== 'string' || !SLOT_NAME_PATTERN.test(slot.name)) return null
    names.push(slot.name)
  }
  return names
}

/**
 * The saved browser recipes under the Procedure panel (Q4-E4.2-4d): the person's recipes, each with
 * one text input per slot and a Run button. Only the literal `true` of VITE_BROWSER_RECIPES builds
 * it. A ZDR session still lists the recipes, but Run and the inputs are disabled. A run starts on
 * the person's click alone. The slot values live in this panel's state, are sent with the start,
 * and are cleared once the start is accepted.
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
  // The typed slot values, per recipe id. They stay in this component.
  const [values, setValues] = createSignal<Record<string, Record<string, string>>>({})
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
      setValues({})
      setStartedRecipe(null)
    },
  )

  const valueOf = (recipeId: string, name: string): string => values()[recipeId]?.[name] ?? ''
  const setValue = (recipeId: string, name: string, value: string) =>
    setValues((current) => ({ ...current, [recipeId]: { ...current[recipeId], [name]: value } }))
  const clearValues = (recipeId: string) =>
    setValues((current) => {
      const next = { ...current }
      delete next[recipeId]
      return next
    })

  const canRun = (recipe: BrowserSavedRecipe, names: string[] | null): boolean =>
    !props.zdr &&
    !busy() &&
    props.sessionId !== '' &&
    names !== null &&
    names.every((name) => valueOf(recipe.recipeId, name).trim() !== '')

  const startRecipe = async (recipe: BrowserSavedRecipe, names: string[]) => {
    if (busy() || props.zdr || props.sessionId === '') return
    const typed: Record<string, string> = values()[recipe.recipeId] ?? {}
    const slots: Record<string, string> = {}
    for (const name of names) slots[name] = (typed[name] ?? '').trim()
    setStartedRecipe(recipe.recipeId)
    const started = await run.runRecipe(props.sessionId, {
      id: recipe.recipeId,
      name: recipe.name,
      version: recipe.latestVersion,
      slots,
    })
    if (started) clearValues(recipe.recipeId)
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
            const names = runnableSlots(recipe)
            return (
              <li>
                <div class="browser-evidence-row">
                  <span class="browser-evidence-row__method">v{recipe.latestVersion}</span>
                  <span class="browser-evidence-row__url" title={recipe.name}>
                    {recipe.name}
                  </span>
                  <span class="browser-evidence-row__policy">
                    {names === null ? i18n.tr('felt ukjent', 'slots unknown') : `${names.length} ${i18n.tr('felt', 'slots')}`}
                  </span>
                  <time class="browser-evidence-row__time" datetime={recipe.updatedAt}>
                    {formatMoment(recipe.updatedAt, i18n.locale())}
                  </time>
                  <button
                    type="button"
                    class="browser-evidence-panel__action"
                    disabled={!canRun(recipe, names)}
                    onClick={() => void startRecipe(recipe, names ?? [])}
                  >
                    {labelFor(recipe.recipeId)}
                  </button>
                </div>
                <Show when={names !== null && names.length > 0}>
                  <div class="browser-evidence-recipe">
                    <For each={names ?? []}>
                      {(name) => (
                        <label class="browser-evidence-recipe__label">
                          <span>{name}</span>
                          <input
                            type="text"
                            class="browser-evidence-recipe__input"
                            maxlength={RECIPE_SLOT_VALUE_MAX_CHARS}
                            autocomplete="off"
                            spellcheck={false}
                            value={valueOf(recipe.recipeId, name)}
                            disabled={props.zdr || busy()}
                            onInput={(event) => setValue(recipe.recipeId, name, event.currentTarget.value)}
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
      <Show when={list.recipes().some((recipe) => runnableSlots(recipe) === null)}>
        <p class="browser-evidence-panel__hint">
          {i18n.tr(
            'Feltene for en oppskrift vises ikke her ennå, så den kan ikke kjøres herfra.',
            "A recipe's inputs are not shown here yet, so it cannot be run from here.",
          )}
        </p>
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
