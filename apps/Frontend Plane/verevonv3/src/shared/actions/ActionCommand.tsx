import { createSignal, Show, type JSX } from 'solid-js'
import { previewAction, executeAction } from '@/shared/actions/action-client'
import { actionExecutionSummary } from '@/shared/actions/action-execution-summary'
import type { ActionId } from '@/shared/actions/action-registry'
import type { ActionActor, ActionExecution, ActionPreview } from '@/shared/actions/types'
import { Badge } from '@/shared/ui/Badge'
import { Button } from '@/shared/ui/Button'
import { translateApiError, useI18n } from '@/shared/i18n'

type ActionCommandProps = {
  actionId: ActionId
  actor: ActionActor
  input: unknown
  icon?: JSX.Element
  children: JSX.Element
}

// Human clicks and model-initiated commands both pass through this component contract.
export function ActionCommand(props: ActionCommandProps) {
  const i18n = useI18n()
  const [preview, setPreview] = createSignal<ActionPreview>()
  const [execution, setExecution] = createSignal<ActionExecution>()
  const [error, setError] = createSignal<string>()

  async function handlePreview() {
    setError(undefined)
    setExecution(undefined)
    setPreview(await previewAction(props.actionId, props.input))
  }

  async function handleExecute() {
    setError(undefined)

    try {
      setExecution(await executeAction(props.actionId, props.actor, props.input))
    } catch (err) {
      setError(
        translateApiError(err, i18n.tr, { no: 'Handlingen feilet.', en: 'Action failed.' }),
      )
    }
  }

  const canExecute = () => preview()?.missingInputs.length === 0

  return (
    <div class="action-command" data-action-id={props.actionId}>
      <div class="action-command__controls">
        <Button variant="secondary" onClick={() => void handlePreview()}>
          {props.icon}
          <span>{props.children}</span>
        </Button>

        <Show when={preview()}>
          {(actionPreview) => (
            <Button variant="primary" disabled={!canExecute()} onClick={() => void handleExecute()}>
              Run
              <span class="sr-only">{actionPreview().actionId}</span>
            </Button>
          )}
        </Show>
      </div>

      <Show when={preview()}>
        {(actionPreview) => (
          <div class="action-command__preview">
            <Badge tone={actionPreview().risk === 'high' ? 'risk' : 'accent'}>
              {actionPreview().requiresApproval ? 'approval gated' : 'direct run'}
            </Badge>
            <span>{actionPreview().estimatedCost}</span>
            <Show when={actionPreview().missingInputs.length > 0}>
              <small>Missing: {actionPreview().missingInputs.join(', ')}</small>
            </Show>
          </div>
        )}
      </Show>

      <Show when={execution()}>
        {(actionExecution) => (
          <p class="action-command__result">
            {actionExecutionSummary(actionExecution())}
          </p>
        )}
      </Show>

      <Show when={error()}>
        {(message) => <p class="action-command__error">{message()}</p>}
      </Show>
    </div>
  )
}
