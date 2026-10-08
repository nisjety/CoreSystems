/**
 * Wide-research fan-out live panel — one card per item a
 * `subagent.research_fan_out` call spawned, grouped by `batchId`
 * (chat-wave2-specs wide-research fan-out §2.3).
 *
 * Mounted INSIDE `ChatLiveRunPanel`'s rail (its `wideResearch` slot, same
 * `runId`) and renders nothing until it has actually seen a fan-out: the
 * switch-in condition is ≥2 `SubagentAttachedEvent`s sharing one non-empty
 * `batchId` — a standalone delegated `subagent.task` call carries no
 * `batchId` at all (filtered out below) and keeps rendering only through the
 * existing flat timeline (`ChatLiveRunPanel`'s activity disclosure /
 * `AgentRunConsole`), unchanged. Hosting it in the rail (rather than as a
 * sibling of the chat page's flex row) gives the card grid the rail's width
 * and lets the rail's own hide/collapse rules apply to it.
 *
 * It only knows what it watched: sub-agent lifecycle events are not part of
 * the durable replay projection (which carries browser evidence only), so a
 * panel mounted after the fact — a reload mid- or post-run — starts empty
 * rather than reconstructing cards it never saw. (Carrying these events into
 * the durable replay is a session-core change, not attempted here.)
 *
 * Honest-state contract, mirroring `chat-run-watch.ts`'s
 * `screenshotStateFor`: a card never spins forever. It is `running` once its
 * own `SubagentAttachedEvent` arrives, `done`/`failed`/`refused` once its
 * matching `SubagentStoppedEvent` arrives (joined by `childRunId`), and
 * `unknown` ("did not finish") the moment the run itself reaches a terminal
 * status (or the event stream closes) before a stop event for that item ever
 * showed up — never left silently spinning.
 */

import { For, Show, createEffect, createSignal } from 'solid-js'
import { CircleCheck, CircleX, Loader2, Slash } from '@/shared/icons'
import { streamRunEvents } from '@/shared/api/run-console-client'
import { getRun } from '@/shared/api/runs-client'
import { useI18n } from '@/shared/i18n'

/** Mirrors `ChatLiveRunPanel`'s own copy (module-local there, not exported) —
 * both independently decide "this run cannot emit more work" from the same
 * durable run status vocabulary. */
const TERMINAL_RUN_STATUSES = new Set([
  'completed',
  'failed',
  'cancelled',
  'canceled',
  'stopped',
  'rejected',
  'expired',
])

function isTerminalRunStatus(status?: string): boolean {
  return TERMINAL_RUN_STATUSES.has((status ?? '').trim().toLowerCase())
}

export type WideResearchItemStatus = 'running' | 'done' | 'failed' | 'refused' | 'unknown'

export type WideResearchItem = {
  childRunId: string
  batchId: string
  goalSummary: string
  role?: string
  status: WideResearchItemStatus
}

/** `SubagentStoppedEvent.status` is the free-text terminal outcome
 * `settle_delegated_child_run` records ("completed" | "failed" | ...) — map
 * it onto the card's own small status vocabulary rather than passing the
 * raw string through, so the pill styling stays exhaustive. */
function stoppedEventStatusToItemStatus(status: string | undefined): WideResearchItemStatus {
  const normalized = (status ?? '').trim().toLowerCase()
  if (normalized === 'completed') return 'done'
  if (normalized === 'cancelled' || normalized === 'canceled') return 'refused'
  // Anything else that actually stopped (including "failed") is a failure;
  // an event with no status text at all is treated the same way rather than
  // silently kept "running".
  return 'failed'
}

function statusPillClass(status: WideResearchItemStatus): string {
  switch (status) {
    case 'done':
      return 'verevon-wide-research__pill--ok'
    case 'failed':
      return 'verevon-wide-research__pill--error'
    case 'refused':
    case 'unknown':
      return 'verevon-wide-research__pill--warn'
    default:
      return 'verevon-wide-research__pill--neutral'
  }
}

export function ChatWideResearchPanel(props: {
  /** Orchestration run id for the turn being watched; null hides the panel. */
  runId: string | null
}) {
  const i18n = useI18n()
  const [items, setItems] = createSignal<WideResearchItem[]>([])
  let activeGeneration = 0

  const statusLabel = (status: WideResearchItemStatus): string => {
    switch (status) {
      case 'running':
        return i18n.tr('Kjører', 'Running')
      case 'done':
        return i18n.tr('Ferdig', 'Done')
      case 'failed':
        return i18n.tr('Feilet', 'Failed')
      case 'refused':
        return i18n.tr('Avvist', 'Refused')
      default:
        return i18n.tr('Fullførte ikke', 'Did not finish')
    }
  }

  createEffect(
    () => props.runId,
    (runId) => {
      const generation = ++activeGeneration
      setItems([])
      if (!runId) return

      const controller = new AbortController()
      const update = (apply: (current: WideResearchItem[]) => WideResearchItem[]) => {
        if (generation !== activeGeneration) return
        setItems(apply)
      }
      // Any item still `running` when the run ends did not finish — never
      // left as a perpetual spinner.
      const markUnresolved = () => {
        update((current) => current.map((item) => (
          item.status === 'running' ? { ...item, status: 'unknown' } : item
        )))
      }

      void getRun(runId, controller.signal)
        .then((detail) => {
          if (
            detail
            && isTerminalRunStatus(detail.status)
            && generation === activeGeneration
            && !controller.signal.aborted
          ) {
            markUnresolved()
          }
        })
        .catch(() => {
          // The live stream remains the source of truth when the read model
          // is temporarily unavailable — same posture as `ChatLiveRunPanel`.
        })

      void streamRunEvents(
        runId,
        {
          onSubagentAttached: (event) => {
            if (!event.childRunId || !event.batchId) return
            const childRunId = event.childRunId
            const batchId = event.batchId
            update((current) => (
              current.some((item) => item.childRunId === childRunId)
                ? current
                : [
                  ...current,
                  {
                    childRunId,
                    batchId,
                    goalSummary: event.goalSummary ?? '',
                    role: event.role,
                    status: 'running',
                  },
                ]
            ))
          },
          onSubagentStopped: (event) => {
            if (!event.childRunId) return
            const childRunId = event.childRunId
            const nextStatus = stoppedEventStatusToItemStatus(event.status)
            update((current) => current.map((item) => (
              item.childRunId === childRunId ? { ...item, status: nextStatus } : item
            )))
          },
          onDone: () => markUnresolved(),
          onError: () => markUnresolved(),
        },
        controller.signal,
      )

      return () => controller.abort()
    },
  )

  // Group by batchId, keep only groups with ≥2 items: that is the fan-out
  // signal itself, per plan §2.3 — a batch that only ever had one item
  // registered (e.g. every other slot was refused by the per-run child
  // ceiling before it started) still isn't the "parallel item cards" this
  // panel exists to show.
  const batches = () => {
    const groups = new Map<string, WideResearchItem[]>()
    for (const item of items()) {
      const list = groups.get(item.batchId)
      if (list) list.push(item)
      else groups.set(item.batchId, [item])
    }
    return [...groups.entries()].filter(([, list]) => list.length >= 2)
  }

  return (
    <Show when={batches().length > 0}>
      <section
        class="verevon-wide-research"
        aria-label={i18n.tr('Bred forskning – parallelle deloppgaver', 'Wide research - parallel sub-tasks')}
      >
        {/*
          Keyed by batch id: `batches()` rebuilds its `[id, items]` pairs on every
          event, so unkeyed each attach or stop would remount every batch and
          every card in it — restarting each running card's spinner and dropping
          hover state, dozens of times over a wide fan-out. Keyed, the batch
          stays put and only the card whose item object changed is rebuilt.
        */}
        <For each={batches()} keyed={(entry) => entry[0]}>
          {(entry) => (
            <div class="verevon-wide-research__batch">
              <header class="verevon-wide-research__batch-head">
                <strong>{i18n.tr('Bred forskning', 'Wide research')}</strong>
                <span>
                  {i18n.tr(
                    `${entry()[1].length} deloppgaver`,
                    `${entry()[1].length} sub-tasks`,
                  )}
                </span>
              </header>
              <div class="verevon-wide-research__grid">
                <For each={entry()[1]}>
                  {(item) => (
                    <article class="verevon-wide-research__card" data-status={item.status}>
                      <p class="verevon-wide-research__goal" title={item.goalSummary}>
                        {item.goalSummary || i18n.tr('(uten oppgitt mål)', '(no stated goal)')}
                      </p>
                      <span class={`verevon-wide-research__pill ${statusPillClass(item.status)}`}>
                        <Show when={item.status === 'running'}>
                          <Loader2 size={11} class="verevon-run-spin" />
                        </Show>
                        <Show when={item.status === 'done'}>
                          <CircleCheck size={11} />
                        </Show>
                        <Show when={item.status === 'failed'}>
                          <CircleX size={11} />
                        </Show>
                        <Show when={item.status === 'refused' || item.status === 'unknown'}>
                          <Slash size={11} />
                        </Show>
                        {statusLabel(item.status)}
                      </span>
                    </article>
                  )}
                </For>
              </div>
            </div>
          )}
        </For>
      </section>
    </Show>
  )
}
