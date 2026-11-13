import { For, Show, createMemo, createSignal } from 'solid-js'
import { formatContextWindow, formatModelListPrice, type ModelInfo } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { SectionHeader, SettingsButton, SettingsSelect } from '@/features/settings/components/settings-ui'
import {
  CATALOGUE_OVERLAY_MAX_ALIASES,
  hasOverlayAlias,
  inCatalogueOrder,
  moveCandidate,
  withNewOverlayAlias,
  withOverlayAliasTarget,
  withOverlayHidden,
  withOverlayOrder,
  withRenamedOverlayAlias,
  withoutOverlayAlias,
  type CatalogueOverlayIssue,
  type RouterPolicyCatalogueOverlay,
} from '@/features/router-policy/lib/router-policy-model'

/** The overlay editor shows only when the build sets `VITE_CATALOGUE_OVERLAY` to the literal `true`. */
export const catalogueOverlayEnabled = () => import.meta.env.VITE_CATALOGUE_OVERLAY === 'true'

/** A row of the model list: a chat model the listing shows, or a hidden id the listing leaves out. */
interface ModelRow {
  readonly id: string
  readonly model: ModelInfo | undefined
}

/** The alias an issue is about, when it is about one. Those issues show on that alias's own row. */
function aliasOf(issue: CatalogueOverlayIssue): string | undefined {
  switch (issue.reason) {
    case 'malformed_alias':
    case 'shadows_model':
    case 'unknown_target':
    case 'hidden_target':
      return issue.alias
    case 'reserved_id':
      return issue.list === 'aliases' ? issue.id : undefined
    default:
      return undefined
  }
}

/** What one refusal of the overlay says, in both languages. */
function issueText(
  issue: CatalogueOverlayIssue,
  i18n: { readonly tr: (noText: string, enText: string) => string },
): string {
  switch (issue.reason) {
    case 'too_many':
      if (issue.list === 'hidden') {
        return i18n.tr(
          `For mange skjulte modeller: ${issue.count}, høyst ${issue.max}.`,
          `Too many hidden models: ${issue.count}, the most is ${issue.max}.`,
        )
      }
      if (issue.list === 'order') {
        return i18n.tr(
          `For mange modeller i rekkefølgen: ${issue.count}, høyst ${issue.max}.`,
          `Too many models in the order: ${issue.count}, the most is ${issue.max}.`,
        )
      }
      return i18n.tr(`For mange alias: ${issue.count}, høyst ${issue.max}.`, `Too many aliases: ${issue.count}, the most is ${issue.max}.`)
    case 'blank_id':
      return i18n.tr('En modell-id er tom.', 'A model id is blank.')
    case 'reserved_id':
      return i18n.tr(
        `«${issue.id}» er en ruteid, så den kan verken skjules eller få et alias.`,
        `«${issue.id}» is a routing id, so it cannot be hidden or given an alias.`,
      )
    case 'malformed_alias':
      if (issue.alias === '') return i18n.tr('Gi aliaset et navn.', 'Give the alias a name.')
      return i18n.tr(
        `«${issue.alias}» er ikke en gyldig alias-id. Bruk små bokstaver, siffer, «.», «_» og «-», høyst 64 tegn.`,
        `«${issue.alias}» is not a valid alias id. Use lower-case letters, digits, «.», «_» and «-», at most 64 characters.`,
      )
    case 'shadows_model':
      return i18n.tr(
        `«${issue.alias}» er allerede en modell i katalogen, eller den er skjult.`,
        `«${issue.alias}» is already a model in the catalogue, or it is hidden.`,
      )
    case 'unknown_target':
      return issue.target === ''
        ? i18n.tr(`Velg en modell for «${issue.alias}».`, `Choose a model for «${issue.alias}».`)
        : i18n.tr(`«${issue.target}» finnes ikke i chatkatalogen.`, `«${issue.target}» is not in the chat catalogue.`)
    case 'hidden_target':
      return i18n.tr(
        `«${issue.target}» er skjult, så «${issue.alias}» kan ikke peke til den.`,
        `«${issue.target}» is hidden, so «${issue.alias}» cannot point to it.`,
      )
  }
}

/**
 * The catalogue overlay section of the router-policy page: which chat models the listing shows, in what order, and
 * which alias ids stand for a model. It edits the overlay it is given and reports the whole overlay after each change;
 * the page writes that into the document it saves, so every other field of the policy is saved as it was loaded.
 */
export function CatalogueOverlayEditor(props: {
  /** The overlay as the draft holds it. */
  overlay: RouterPolicyCatalogueOverlay
  /** The chat models the listing shows, without the overlay's own alias entries. */
  models: readonly ModelInfo[]
  /** The refusals to show: the page's own checks, and the ones the server gave the last save. */
  issues: readonly CatalogueOverlayIssue[]
  /** Receives the whole overlay after each change. */
  onOverlay: (overlay: RouterPolicyCatalogueOverlay) => void
}) {
  const i18n = useI18n()
  const [refusal, setRefusal] = createSignal<string | null>(null)

  const hiddenIds = createMemo(() => new Set(props.overlay.hidden.map((id) => id.toLowerCase())))
  const isHidden = (id: string) => hiddenIds().has(id.toLowerCase())
  const choices = createMemo(() => props.models.filter((model) => !isHidden(model.id)))
  const aliasRows = createMemo(() => Object.entries(props.overlay.aliases).map(([alias, target]) => ({ alias, target })))

  // The listing leaves out a hidden model, so the hidden ids it does not show are listed here as well: otherwise the
  // person could not show them again. The overlay's order sets the sequence of every row.
  const rows = createMemo((): ModelRow[] => {
    const shown = props.models.map((model): ModelRow => ({ id: model.id, model }))
    const unshown = props.overlay.hidden
      .filter((id) => !props.models.some((model) => model.id.toLowerCase() === id.toLowerCase()))
      .map((id): ModelRow => ({ id, model: undefined }))
    return inCatalogueOrder([...shown, ...unshown], props.overlay.order)
  })

  const aliasIssues = (alias: string) => props.issues.filter((issue) => aliasOf(issue) === alias)
  const sectionIssues = () => props.issues.filter((issue) => aliasOf(issue) === undefined)
  const addDisabled = () =>
    hasOverlayAlias(props.overlay, '') || Object.keys(props.overlay.aliases).length >= CATALOGUE_OVERLAY_MAX_ALIASES

  const detailOf = (row: ModelRow): string => {
    const model = row.model
    if (!model) return row.id
    const parts = [
      row.id,
      formatContextWindow(model.contextWindowTokens),
      model.price ? formatModelListPrice(model.price, i18n.locale()) : undefined,
    ]
    return parts.filter((part): part is string => part !== undefined).join(' · ')
  }

  // The chat models that are not hidden. A target no longer offered stays in the select, so the select keeps the stored
  // value and the issue on its row says why it is refused.
  const targetOptions = (target: string) => {
    const options = choices().map((model) => ({ value: model.id, label: model.name || model.id }))
    if (target === '') return [{ value: '', label: i18n.tr('Velg en modell…', 'Choose a model…') }, ...options]
    if (options.some((option) => option.value === target)) return options
    return [{ value: target, label: `${target} (${i18n.tr('ikke tilgjengelig', 'unavailable')})` }, ...options]
  }

  const moveRow = (index: number, step: -1 | 1) => {
    props.onOverlay(withOverlayOrder(props.overlay, moveCandidate(rows().map((row) => row.id), index, step)))
  }

  const toggleHidden = (id: string) => {
    props.onOverlay(withOverlayHidden(props.overlay, id, !isHidden(id)))
  }

  // An alias is renamed on change (blur or Enter), not on each keystroke, so a name can pass through its intermediate
  // spellings. A name another alias already holds is refused, and the field goes back to the name it had.
  const renameAlias = (event: { currentTarget: HTMLInputElement }, from: string) => {
    const to = event.currentTarget.value.trim()
    const taken = to !== from && hasOverlayAlias(props.overlay, to)
    setRefusal(taken ? i18n.tr(`Aliaset «${to}» finnes allerede.`, `The alias «${to}» already exists.`) : null)
    event.currentTarget.value = taken ? from : to
    if (!taken && to !== from) props.onOverlay(withRenamedOverlayAlias(props.overlay, from, to))
  }

  const retarget = (alias: string, target: string) => {
    props.onOverlay(withOverlayAliasTarget(props.overlay, alias, target))
  }

  const addAlias = () => {
    setRefusal(null)
    props.onOverlay(withNewOverlayAlias(props.overlay, choices()[0]?.id ?? ''))
  }

  return (
    <>
      <SectionHeader
        title={i18n.tr('Katalogoversikt', 'Catalogue overlay')}
        description={i18n.tr(
          'Skjul modeller, sett rekkefølgen i listen og gi en modell et alias. Endringene gjelder når katalogoversikten er slått på i tjenesten.',
          'Hide models, set the order of the list and give a model an alias. The changes apply once the catalogue overlay is switched on in the service.',
        )}
      />
      <div class="verevon-router-policy__table">
        <For each={sectionIssues()}>
          {(issue) => (
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {issueText(issue, i18n)}
            </p>
          )}
        </For>
        <Show when={refusal()}>
          {(message) => (
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {message()}
            </p>
          )}
        </Show>

        <div class="verevon-router-policy__mode">
          <div class="verevon-router-policy__mode-head">
            <p>{i18n.tr('Modeller', 'Models')}</p>
            <span>
              {i18n.tr(
                'Skjulte modeller vises ikke i chatten. Rekkefølgen her er rekkefølgen i listen.',
                'Hidden models are not offered in chat. The order here is the order of the list.',
              )}
            </span>
          </div>
          <Show
            when={rows().length > 0}
            fallback={
              <p class="verevon-settings-subnote">
                {i18n.tr('Ingen chatmodeller i katalogen.', 'No chat models in the catalogue.')}
              </p>
            }
          >
            <ol class="verevon-trust-chips" aria-label={i18n.tr('Modeller i katalogen', 'Models in the catalogue')}>
              <For each={rows()} keyed={(row) => row.id}>
                {(row, index) => {
                  const name = () => row().model?.name || row().id
                  return (
                    <li class="verevon-trust-chip">
                      <span>
                        {index() + 1}. {name()}
                      </span>
                      <span class="verevon-settings-help">{detailOf(row())}</span>
                      <Show when={isHidden(row().id)}>
                        <span class="verevon-settings-help">{i18n.tr('Skjult', 'Hidden')}</span>
                      </Show>
                      <SettingsButton
                        type="button"
                        settingsSize="xs"
                        aria-label={
                          isHidden(row().id)
                            ? i18n.tr(`Vis ${name()}`, `Show ${name()}`)
                            : i18n.tr(`Skjul ${name()}`, `Hide ${name()}`)
                        }
                        onClick={() => toggleHidden(row().id)}
                      >
                        {isHidden(row().id) ? i18n.tr('Vis', 'Show') : i18n.tr('Skjul', 'Hide')}
                      </SettingsButton>
                      <SettingsButton
                        type="button"
                        settingsSize="xs"
                        disabled={index() === 0}
                        aria-label={i18n.tr(`Flytt ${name()} opp`, `Move ${name()} up`)}
                        onClick={() => moveRow(index(), -1)}
                      >
                        ↑
                      </SettingsButton>
                      <SettingsButton
                        type="button"
                        settingsSize="xs"
                        disabled={index() === rows().length - 1}
                        aria-label={i18n.tr(`Flytt ${name()} ned`, `Move ${name()} down`)}
                        onClick={() => moveRow(index(), 1)}
                      >
                        ↓
                      </SettingsButton>
                    </li>
                  )
                }}
              </For>
            </ol>
          </Show>
        </div>

        <div class="verevon-router-policy__mode">
          <div class="verevon-router-policy__mode-head">
            <p>{i18n.tr('Alias', 'Aliases')}</p>
            <span>
              {i18n.tr(
                'Et alias er et annet navn på en modell i listen. Navnet kan ikke være id-en til en modell som allerede finnes.',
                'An alias is another name for a listed model. It cannot be the id of a model that already exists.',
              )}
            </span>
          </div>
          <Show
            when={aliasRows().length > 0}
            fallback={<p class="verevon-settings-subnote">{i18n.tr('Ingen alias.', 'No aliases.')}</p>}
          >
            <For each={aliasRows()} keyed={false}>
              {(row, index) => (
                <div>
                  <div class="verevon-router-policy__cells">
                    <label for={`catalogue-alias-${index}`} class="verevon-settings-field">
                      <span class="verevon-settings-label">{i18n.tr('Alias', 'Alias')}</span>
                      <span class="verevon-settings-input-wrap">
                        <VerevonInput
                          id={`catalogue-alias-${index}`}
                          type="text"
                          value={row().alias}
                          placeholder={i18n.tr('Navn på aliaset', 'Alias name')}
                          autocomplete="off"
                          spellcheck={false}
                          onChange={(event) => renameAlias(event, row().alias)}
                        />
                      </span>
                    </label>
                    <SettingsSelect
                      id={`catalogue-alias-target-${index}`}
                      label={i18n.tr('Peker til', 'Points to')}
                      value={row().target}
                      options={targetOptions(row().target)}
                      onChange={(event) => retarget(row().alias, event.currentTarget.value)}
                    />
                    <div>
                      <SettingsButton
                        type="button"
                        settingsSize="xs"
                        aria-label={i18n.tr(`Fjern aliaset ${row().alias}`, `Remove the alias ${row().alias}`)}
                        onClick={() => props.onOverlay(withoutOverlayAlias(props.overlay, row().alias))}
                      >
                        {i18n.tr('Fjern', 'Remove')}
                      </SettingsButton>
                    </div>
                  </div>
                  <For each={aliasIssues(row().alias)}>
                    {(issue) => (
                      <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                        {issueText(issue, i18n)}
                      </p>
                    )}
                  </For>
                </div>
              )}
            </For>
          </Show>
          <div>
            <SettingsButton type="button" settingsSize="xs" disabled={addDisabled()} onClick={addAlias}>
              {i18n.tr('Legg til alias', 'Add alias')}
            </SettingsButton>
          </div>
        </div>
      </div>
    </>
  )
}
