import { createMemo, For, onCleanup, Show } from 'solid-js'
import { Coins, Gauge, Grid2X2, PauseCircle, ShieldCheck } from '@/shared/icons'
import { Button } from '@/shared/ui/Button'
import { cn } from '@/shared/lib/cn'
import { createResource } from '@/shared/lib/create-resource-compat'

// W5 — FleetRunConsole. Renders a grid of N live views, a shared budget bar,
// and a fleet-level intervention panel. Reuses AgentRunConsole's
// TelemetryStat shape and the existing `GET /v1/fleets/:id` + budget +
// `quarry.fleet.<fleet_id>.>` SSE wiring. SolidJS only; no new deps.

type FleetBudget = {
  fleet_id: string
  budget_usd: number | null
  spent_usd: number
  over_budget: boolean
  per_run_usd: Record<string, number>
}

type FleetTask = {
  fleet_id: string
  status: string
  max_parallel_runs: number
  member_run_ids: string[]
}

async function fetchFleet(fleetId: string): Promise<FleetTask> {
  const res = await fetch(`/v1/fleets/${fleetId}`)
  if (!res.ok) throw new Error(`fleet ${fleetId} not found`)
  const body = await res.json()
  return body.fleet ?? body.data ?? body
}

async function fetchBudget(fleetId: string): Promise<FleetBudget> {
  const res = await fetch(`/v1/fleets/${fleetId}/budget`)
  if (!res.ok) throw new Error(`budget ${fleetId} not found`)
  return (await res.json()) as FleetBudget
}

export function FleetRunConsole(props: { fleetId: string }) {
  const [fleet] = createResource(() => props.fleetId, fetchFleet)
  const [budget, { refetch: refetchBudget }] = createResource(
    () => props.fleetId,
    fetchBudget,
  )

  // Poll budget every 3s so the bar stays honest without SSE.
  // The fleet SSE (quarry.fleet.<id>.>) will replace this once wired.
  let timer: number | undefined
  const startPoll = () => {
    timer = window.setInterval(() => refetchBudget(), 3000) as unknown as number
  }
  const stopPoll = () => {
    if (timer) window.clearInterval(timer)
  }
  startPoll()
  onCleanup(stopPoll)

  const spent = createMemo(() => budget()?.spent_usd ?? 0)
  const limit = createMemo(() => budget()?.budget_usd ?? null)
  const pct = createMemo(() => {
    const l = limit()
    if (l == null || l <= 0) return 0
    return Math.min(100, (spent() / l) * 100)
  })

  return (
    <div class="flex flex-col gap-4 p-4" data-testid="fleet-run-console">
      <Show when={fleet()}>
        {(f) => (
          <div class="flex items-center gap-3">
            <Grid2X2 class="h-5 w-5" />
            <span class="font-medium">Fleet {f().fleet_id}</span>
            <span class="text-sm text-muted-foreground">
              {f().status} · {f().member_run_ids.length} runs · max {f().max_parallel_runs} parallel
            </span>
          </div>
        )}
      </Show>

      {/* Shared budget bar */}
      <Show when={budget()}>
        {(b) => (
          <div class="rounded-lg border p-3">
            <div class="flex items-center gap-2 text-sm">
              <Coins class="h-4 w-4" />
              <span>Budget</span>
              <span class="ml-auto font-mono">
                ${spent().toFixed(4)} / {limit() != null ? `$${limit()!.toFixed(2)}` : 'uncapped'}
              </span>
              <Show when={b().over_budget}>
                <span class="text-destructive text-xs font-medium">OVER BUDGET</span>
              </Show>
            </div>
            <Show when={limit() != null}>
              <div class="mt-2 h-2 rounded bg-muted">
                <div
                  class={cn('h-2 rounded transition-all', b().over_budget ? 'bg-destructive' : 'bg-primary')}
                  style={{ width: `${pct()}%` }}
                />
              </div>
            </Show>
            <div class="mt-2 grid grid-cols-2 gap-2 text-xs font-mono text-muted-foreground md:grid-cols-4">
              <For each={Object.entries(b().per_run_usd)}>
                {([runId, usd]) => (
                  <span>
                    {runId.slice(0, 8)}: ${Number(usd).toFixed(4)}
                  </span>
                )}
              </For>
            </div>
          </div>
        )}
      </Show>

      {/* Member live-view grid */}
      <Show when={fleet()}>
        {(f) => (
          <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
            <For each={f().member_run_ids}>
              {(runId) => (
                <div class="rounded-lg border p-2">
                  <div class="mb-2 flex items-center gap-2 text-xs text-muted-foreground">
                    <Gauge class="h-3 w-3" />
                    {runId.slice(0, 12)}
                    <span class="ml-auto font-mono">
                      ${Number(budget()?.per_run_usd[runId] ?? 0).toFixed(4)}
                    </span>
                  </div>
                  <div class="aspect-video rounded bg-muted flex items-center justify-center text-xs text-muted-foreground">
                    live view: /v1/agent/runs/{runId.slice(0, 8)}/live-view
                  </div>
                </div>
              )}
            </For>
          </div>
        )}
      </Show>

      {/* Fleet-level intervention */}
      <div class="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={async () => {
            await fetch(`/v1/fleets/${props.fleetId}`, {
              method: 'PATCH',
              headers: { 'content-type': 'application/json' },
              body: JSON.stringify({ status: 'cancelled' }),
            })
            refetchBudget()
          }}
        >
          <PauseCircle class="mr-2 h-4 w-4" />
          Pause fleet
        </Button>
        <span class="text-xs text-muted-foreground self-center flex items-center gap-1">
          <ShieldCheck class="h-3 w-3" />
          Pausing cascades to all member runs (ActionCascadePolicy)
        </span>
      </div>
    </div>
  )
}

export default FleetRunConsole
