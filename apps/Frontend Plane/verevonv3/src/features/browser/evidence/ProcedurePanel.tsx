import { For, Show, createSignal } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { browserErrorMessage } from '@/shared/api/browser-client'
import { useI18n } from '@/shared/i18n'
import { recipesEnabled, useBrowserEvidenceScope, useBrowserProcedure, useBrowserReceipts, useSaveRecipe } from './use-browser-evidence'
import { RecipesPanel } from './RecipesPanel'
import { outcomeLabel, procedureActionSummary } from './receipts-view'
import './browser-evidence.css'

/** The longest recipe name the save accepts (the gateway's and the action contract's bound). */
const RECIPE_NAME_MAX_CHARS = 120

/**
 * Evidence drawer "Procedure" section (WS9.D). "Compile procedure" asks the gateway to turn
 * the session's receipts into a replayable procedure and to run the promotion check. The
 * steps show the redacted action (a typed text is only its length) and the quality verdict.
 * A ZDR session gets the copy and a disabled button, and no request is made. A failed
 * compile (409) shows its typed reason as text.
 *
 * With VITE_BROWSER_RECIPES set to `true`, a procedure that passes the check can be saved as a
 * recipe (Q4-E4.2-4b). The panel shows the names of the slots the recipe will ask for, never a
 * typed value, and the saved version once the save answers. The saved recipes are listed under
 * the procedure, and a listed recipe can be run from there (Q4-E4.2-4d).
 */
export function ProcedurePanel(props: { sessionId: string; zdr: boolean }): JSX.Element {
  const i18n = useI18n()
  const scope = useBrowserEvidenceScope(
    () => props.sessionId,
    () => props.zdr,
  )
  const procedure = useBrowserProcedure(scope)
  const receipts = useBrowserReceipts(scope)
  const saveRecipe = useSaveRecipe(scope)
  const [recipeName, setRecipeName] = createSignal('')
  const busy = () => procedure.status() === 'running'
  const saving = () => saveRecipe.status() === 'running'
  const failureMessage = () => {
    const failure = procedure.error()
    if (!failure) return null
    return browserErrorMessage(failure, i18n.tr, {
      no: 'Prosedyren kunne ikke kompileres.',
      en: 'The procedure could not be compiled.',
    })
  }
  const saveFailureMessage = () => {
    const failure = saveRecipe.error()
    if (!failure) return null
    return browserErrorMessage(failure, i18n.tr, {
      no: 'Oppskriften kunne ikke lagres.',
      en: 'The recipe could not be saved.',
    })
  }

  return (
    <section class="browser-evidence-panel" aria-label={i18n.tr('Prosedyre', 'Procedure')}>
      <header class="browser-evidence-panel__head">
        <span>{i18n.tr('Prosedyre', 'Procedure')}</span>
        <Show when={props.zdr}>
          <span class="browser-evidence-chip browser-evidence-chip--zdr">
            {i18n.tr('Ikke tilgjengelig for ZDR-økter', 'Not available for ZDR sessions')}
          </span>
        </Show>
      </header>

      <button
        type="button"
        class="browser-evidence-panel__action"
        disabled={props.zdr || busy() || !scope()}
        onClick={() => procedure.compile()}
      >
        {busy() ? i18n.tr('Kompilerer…', 'Compiling…') : i18n.tr('Kompiler prosedyre', 'Compile procedure')}
      </button>

      <Show when={failureMessage()}>
        {(message) => (
          <p class="browser-evidence-panel__hint" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={procedure.result()}>
        {(response) => (
          <div class="browser-evidence-procedure">
            <p class="browser-evidence-procedure__quality" data-promotion={response().quality.promotionAllowed ? 'allowed' : 'blocked'}>
              {response().quality.promotionAllowed
                ? i18n.tr('Klar for promotering', 'Ready to promote')
                : i18n.tr('Ikke klar for promotering', 'Not ready to promote')}
              {' · '}
              {response().quality.verifiedStepCount}/{response().quality.stepCount}{' '}
              {i18n.tr('steg verifisert', 'steps verified')}
            </p>
            <Show when={(response().quality.reasons ?? []).length > 0}>
              <ul class="browser-evidence-procedure__reasons">
                <For each={response().quality.reasons ?? []}>{(reason) => <li>{reason}</li>}</For>
              </ul>
            </Show>
            <ol class="browser-evidence-list">
              <For each={response().procedure.steps}>
                {(step) => {
                  const summary = procedureActionSummary(step.action, i18n.tr)
                  const expectCount = step.expect?.all.length ?? 0
                  return (
                    <li class="browser-evidence-row">
                      <span class="browser-evidence-row__method">{step.step}</span>
                      <code class="browser-evidence-row__url">{summary.type}</code>
                      <Show when={summary.hidden}>{(hidden) => <span class="browser-evidence-row__policy">{hidden()}</span>}</Show>
                      <span class="browser-evidence-row__policy">{step.expectedOutcome}</span>
                      <Show when={recipesEnabled() && (step.slot || step.expect)}>
                        <span class="browser-evidence-row__badges">
                          <Show when={step.slot}>
                            {(slot) => (
                              <span class="browser-evidence-badge" data-badge="slot">
                                {i18n.tr('Felt', 'Slot')} {slot()}
                              </span>
                            )}
                          </Show>
                          <Show when={step.expect}>
                            <span class="browser-evidence-badge" data-badge="expect">
                              {i18n.tr('Forventer', 'Expect')} {expectCount}
                            </span>
                          </Show>
                        </span>
                      </Show>
                    </li>
                  )
                }}
              </For>
            </ol>
            <Show when={recipesEnabled() && response().quality.promotionAllowed}>
              <div class="browser-evidence-recipe">
                <Show when={(response().procedure.slots ?? []).length > 0}>
                  <p class="browser-evidence-panel__hint">
                    {i18n.tr('Oppskriften spør etter', 'The recipe asks for')}{' '}
                    <For each={response().procedure.slots ?? []}>
                      {(slot, index) => (
                        <>
                          {index() > 0 ? ', ' : ''}
                          <code>{slot.name}</code> (
                          {slot.kind === 'secret' ? i18n.tr('hemmelig', 'secret') : i18n.tr('tekst', 'text')})
                        </>
                      )}
                    </For>
                  </p>
                </Show>
                <label class="browser-evidence-recipe__label">
                  <span>{i18n.tr('Navn på oppskriften', 'Recipe name')}</span>
                  <input
                    type="text"
                    class="browser-evidence-recipe__input"
                    maxlength={RECIPE_NAME_MAX_CHARS}
                    value={recipeName()}
                    disabled={props.zdr || saving()}
                    onInput={(event) => setRecipeName(event.currentTarget.value)}
                  />
                </label>
                <button
                  type="button"
                  class="browser-evidence-panel__action"
                  disabled={props.zdr || saving() || !scope() || recipeName().trim() === ''}
                  onClick={() => saveRecipe.save({ name: recipeName().trim() })}
                >
                  {saving() ? i18n.tr('Lagrer…', 'Saving…') : i18n.tr('Lagre som oppskrift', 'Save as recipe')}
                </button>
                <Show when={saveFailureMessage()}>
                  {(message) => (
                    <p class="browser-evidence-panel__hint" role="alert">
                      {message()}
                    </p>
                  )}
                </Show>
                <Show when={saveRecipe.result()}>
                  {(saved) => (
                    <p class="browser-evidence-recipe__saved" role="status">
                      {i18n.tr('Lagret', 'Saved')} · {i18n.tr('versjon', 'version')} {saved().version}
                    </p>
                  )}
                </Show>
              </div>
            </Show>
          </div>
        )}
      </Show>

      <RecipesPanel sessionId={props.sessionId} zdr={props.zdr} savedRecipe={saveRecipe.result()} />

      <Show when={!props.zdr}>
        <div class="browser-evidence-procedure__sources">
          <p class="browser-evidence-panel__hint">
            {i18n.tr('Kilder', 'Sources')}: {receipts.receipts().length}
            {receipts.truncated() ? ` (${i18n.tr('de siste', 'most recent')})` : ''}
          </p>
          <ol class="browser-evidence-list">
            <For each={receipts.receipts().slice(-5).reverse()}>
              {(receipt) => (
                <li class="browser-evidence-row">
                  <span class="browser-evidence-row__method">#{receipt.step}</span>
                  <span class="browser-evidence-row__url">{receipt.actionLabel || receipt.actionType}</span>
                  <span class="browser-evidence-row__policy">{outcomeLabel(receipt.outcome?.status, i18n.tr)}</span>
                </li>
              )}
            </For>
          </ol>
        </div>
      </Show>
    </section>
  )
}
