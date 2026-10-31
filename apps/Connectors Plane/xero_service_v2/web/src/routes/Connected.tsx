import { For, Show, Switch, Match, createSignal } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { createQuery, useQueryClient } from "@tanstack/solid-query";

import {
  fetchConnections,
  logout,
  LOGIN_URL,
  fetchSyncStatus,
  startSync,
  startBackfill,
} from "../lib/api";

/** Post-auth surface: connected org(s) + on-demand sync controls. */
export default function Connected() {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [actionError, setActionError] = createSignal<string | null>(null);

  const connections = createQuery(() => ({
    queryKey: ["connections"],
    queryFn: fetchConnections,
  }));

  const syncStatus = createQuery(() => ({
    queryKey: ["syncStatus"],
    queryFn: fetchSyncStatus,
    // Poll every 2s while a run is in flight; otherwise idle.
    refetchInterval: (query) =>
      query.state.data?.state === "running" ? 2000 : false,
  }));

  const running = () => syncStatus.data?.state === "running";

  // Backfill date range (YYYY-MM-DD). Defaults to the last 3 years → today.
  const isoDate = (d: Date) => d.toISOString().slice(0, 10);
  const todayIso = isoDate(new Date());
  const threeYearsAgoIso = () => {
    const d = new Date();
    d.setFullYear(d.getFullYear() - 3);
    return isoDate(d);
  };
  const [backfillFrom, setBackfillFrom] = createSignal(threeYearsAgoIso());
  const [backfillTo, setBackfillTo] = createSignal(todayIso);

  const trigger = (fn: () => Promise<void>) => async () => {
    setActionError(null);
    try {
      await fn();
      await qc.invalidateQueries({ queryKey: ["syncStatus"] });
    } catch (e) {
      setActionError(e instanceof Error ? e.message : String(e));
    }
  };

  const onLogout = async () => {
    try {
      await logout();
    } finally {
      navigate("/", { replace: true });
    }
  };

  return (
    <section class="card">
      <h1>Connected organisations</h1>

      <Switch>
        <Match when={connections.isPending}>
          <p class="muted">Loading your connections…</p>
        </Match>
        <Match when={connections.isError}>
          <p class="error">Couldn&apos;t load connections: {String(connections.error)}</p>
        </Match>
        <Match when={connections.isSuccess}>
          <Show
            when={connections.data}
            keyed
            fallback={<p class="muted">No organisations connected yet.</p>}
          >
            {(data) => (
              <Show
                when={data.connections.length > 0}
                fallback={<p class="muted">No organisations connected yet.</p>}
              >
                <ul class="conn-list">
                  <For each={data.connections}>
                    {(conn) => (
                      <li class="conn">
                        <span class="conn__name">{conn.tenantName}</span>
                        <span class="conn__meta">
                          {conn.tenantType} · <code>{conn.tenantId}</code>
                        </span>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            )}
          </Show>
        </Match>
      </Switch>

      {/* ── Sync controls ─────────────────────────────────────────────── */}
      <div class="sync">
        <h2>Sync to the data warehouse</h2>
        <p class="muted">
          Pull your Xero data into the warehouse. Use <strong>Sync now</strong> for
          recent changes, or run a <strong>Backfill</strong> over a date range for
          history (slow — runs in the background).
        </p>
        <div class="actions">
          <button
            class="btn btn--primary"
            type="button"
            disabled={running()}
            onClick={trigger(startSync)}
          >
            {running() ? "Running…" : "Sync now"}
          </button>
        </div>

        <div class="backfill">
          <div class="backfill__range">
            <label class="field">
              <span class="field__label">From</span>
              <input
                class="field__input"
                type="date"
                value={backfillFrom()}
                max={backfillTo()}
                disabled={running()}
                onInput={(e) => setBackfillFrom(e.currentTarget.value)}
              />
            </label>
            <label class="field">
              <span class="field__label">To</span>
              <input
                class="field__input"
                type="date"
                value={backfillTo()}
                max={todayIso}
                disabled={running()}
                onInput={(e) => setBackfillTo(e.currentTarget.value)}
              />
            </label>
            <button
              class="btn"
              type="button"
              disabled={running()}
              onClick={trigger(() => startBackfill(backfillFrom(), backfillTo()))}
            >
              Backfill
            </button>
          </div>
          <p class="muted backfill__hint">
            Defaults to the last 3 years. Pick any range — runs in the background in
            3-month chunks.
          </p>
        </div>

        <Show when={actionError()}>
          <p class="error">{actionError()}</p>
        </Show>

        <Show when={syncStatus.data} keyed>
          {(s) => (
            <p class="sync__status muted">
              {s.state === "running"
                ? `${s.kind} sync running…`
                : s.state === "finished"
                  ? `Last ${s.kind}: ${s.ok ? "✓ done" : "✗ failed"} — ${s.summary}`
                  : "No sync run yet."}
            </p>
          )}
        </Show>
      </div>

      <div class="actions">
        <a class="btn" href={LOGIN_URL} rel="external">
          Connect another org
        </a>
        <button class="btn btn--ghost" type="button" onClick={onLogout}>
          Sign out
        </button>
      </div>
    </section>
  );
}
