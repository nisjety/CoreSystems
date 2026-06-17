import { createMemo, createSignal, For } from 'solid-js'
import { OnboardingLinkButton } from '@/features/onboarding/components/shared/OnboardingLinkButton'
import {
  type ConnectorCategory,
  type ConnectorOption,
  type GraphDisplayNode,
  type OnboardingState,
  onboardingConnectorOptions,
} from '@/features/onboarding/lib/model'
import { truncateGraphLabel } from '@/features/onboarding/lib/view'
import { Badge } from '@/shared/ui/Badge'
import { Button } from '@/shared/ui/Button'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'
import { VelionSelectableRow } from '@/shared/ui/velion/VelionSelectableRow'

const connectorTabs: Array<{
  id: ConnectorCategory
  label: string
  description: string
}> = [
  {
    id: 'work',
    label: 'Work systems',
    description: 'Slack, Microsoft, Google, Notion, GitHub',
  },
  {
    id: 'social',
    label: 'Social channels',
    description: 'Instagram, LinkedIn, TikTok, X, Facebook',
  },
  {
    id: 'other',
    label: 'Other apps',
    description: 'Commerce, billing and operational sources',
  },
]

type ConnectStepContentProps = {
  connectedSources: OnboardingState['connectors']
  connectingId?: string
  onConnect: (option: ConnectorOption) => void | Promise<void>
  onContinue: () => void
  onSkip: () => void
}

export function ConnectStepContent(props: ConnectStepContentProps) {
  const [activeTab, setActiveTab] = createSignal<ConnectorCategory>('work')
  const visibleConnectors = createMemo(() =>
    onboardingConnectorOptions.filter((item) => item.category === activeTab()),
  )

  return (
    <section class="onboarding-copy onboarding-copy--connect">
      <p class="onboarding-eyebrow">Integrations</p>
      <h1>Koble systemer</h1>
      <p>Velg systemene Velion skal lære fra, svare på vegne av, eller bruke som signaler for automasjon.</p>

      <div class="onboarding-connector-tabs" role="tablist" aria-label="Integration categories">
        <For each={connectorTabs}>
          {(tab) => (
            <button
              type="button"
              role="tab"
              aria-selected={activeTab() === tab.id}
              class="onboarding-connector-tab"
              classList={{ 'onboarding-connector-tab--active': activeTab() === tab.id }}
              onClick={() => setActiveTab(tab.id)}
            >
              <span>{tab.label}</span>
              <small>{tab.description}</small>
            </button>
          )}
        </For>
      </div>

      <div class="onboarding-connector-group" role="tabpanel">
        <h3>{connectorTabs.find((tab) => tab.id === activeTab())?.label}</h3>
        <div class="onboarding-connector-list">
          <For each={visibleConnectors()}>
            {(item) => {
              const status = () => props.connectedSources.find((connector) => connector.id === item.id)?.status

              return (
                <VelionSelectableRow
                  compact
                  onClick={() => void props.onConnect(item)}
                  disabled={props.connectingId === item.id}
                  title={item.label}
                  description={item.hint}
                  meta={
                    <Badge tone={status() === 'connected' ? 'accent' : 'neutral'}>
                      {connectorStatusLabel(status(), props.connectingId === item.id)}
                    </Badge>
                  }
                />
              )
            }}
          </For>
        </div>
      </div>

      <div class="onboarding-actions onboarding-actions--connect">
        <Button variant="primary" size="sm" onClick={props.onContinue}>
          Fortsett
        </Button>
        <OnboardingLinkButton onClick={props.onSkip}>Hopp over</OnboardingLinkButton>
      </div>
    </section>
  )
}

function connectorStatusLabel(
  status: OnboardingState['connectors'][number]['status'] | undefined,
  connecting: boolean,
) {
  if (connecting) return 'Åpner'
  if (status === 'connected') return 'Tilkoblet'
  if (status === 'partial') return 'Delvis'
  if (status === 'pending') return 'Venter'
  return 'Legg til'
}

export function ConnectStepVisual(props: { graphNodes: GraphDisplayNode[] }) {
  return (
    <div class="onboarding-source-graph" aria-label="Interaktiv kildegraf">
      <div class="onboarding-source-graph__controls">
        <VelionIconButton aria-label="Zoom ut" size="sm" shape="rounded" tone="inverted">
          −
        </VelionIconButton>
        <span>100%</span>
        <VelionIconButton aria-label="Zoom inn" size="sm" shape="rounded" tone="inverted">
          +
        </VelionIconButton>
        <VelionIconButton aria-label="Tilbakestill" size="sm" shape="rounded" tone="inverted">
          ↻
        </VelionIconButton>
      </div>
      <div class="onboarding-source-graph__nodes">
        <For each={props.graphNodes}>
          {(node) => (
            <div
              class="onboarding-source-graph__node"
              classList={{
                'onboarding-source-graph__node--org': node.group === 'org',
                'onboarding-source-graph__node--integration': node.group === 'integration',
              }}
              style={{ left: node.position.left, top: node.position.top }}
            >
              <span />
              <small>{truncateGraphLabel(node.label)}</small>
            </div>
          )}
        </For>
      </div>
    </div>
  )
}
