'use client';

import { useCallback, useEffect, useState, type ReactElement } from 'react';
import { Loader2, RefreshCcw } from 'lucide-react';

import { useRunEvents } from '@/lib/hooks/useRunEvents';

import type { PersistedAgent } from './types';

/**
 * Operator inbox for deployed agents (Intercom/Zendesk-style).
 *
 * Lists recent runs for the agent and, on selection, shows the shared
 * run-event feed (useRunEvents) plus HITL approve/reject controls when the run
 * is paused awaiting an approval. Only rendered for the `deployed_agent`
 * profile — chat-profile agents have no operator surface.
 */

interface AgentRun {
  runId: string;
  agentId?: string;
  status: 'started' | 'completed' | 'failed' | 'cancelled';
  rating?: 'good' | 'acceptable' | 'poor';
  startedAt: number;
  completedAt?: number;
  error?: string;
}

const STATUS_STYLE: Record<AgentRun['status'], string> = {
  started: 'bg-blue-50 text-blue-700',
  completed: 'bg-green-50 text-green-700',
  failed: 'bg-rose-50 text-rose-700',
  cancelled: 'bg-gray-100 text-gray-500',
};

interface AgentInboxProps {
  agent: PersistedAgent;
}

export function AgentInbox({ agent }: AgentInboxProps): ReactElement {
  const [runs, setRuns] = useState<AgentRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const res = await fetch(`/api/agents/runs?agentId=${encodeURIComponent(agent.id)}&limit=50`, {
        cache: 'no-store',
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = (await res.json()) as { runs?: AgentRun[] };
      setRuns(data.runs ?? []);
      setError(null);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'failed to load runs');
    } finally {
      setLoading(false);
    }
  }, [agent.id]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="flex h-full min-h-0 bg-white">
      {/* Run list */}
      <div className="flex w-[320px] shrink-0 flex-col border-r border-[#E5E7EB]">
        <div className="flex items-center justify-between border-b border-[#E5E7EB] px-4 py-3">
          <h3 className="text-[13px] font-semibold text-[#111827]">Inbox</h3>
          <button
            type="button"
            onClick={() => void load()}
            className="rounded-full p-1.5 text-gray-400 hover:bg-gray-100 hover:text-gray-600"
            aria-label="Refresh"
          >
            <RefreshCcw className="size-4" />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto">
          {loading ? (
            <div className="flex items-center justify-center py-10 text-gray-400">
              <Loader2 className="size-4 animate-spin" />
            </div>
          ) : error ? (
            <p className="px-4 py-6 text-[12px] text-rose-600">{error}</p>
          ) : runs.length === 0 ? (
            <p className="px-4 py-6 text-[12px] text-gray-400">No runs yet.</p>
          ) : (
            <ul>
              {runs.map((run) => {
                const active = run.runId === selectedRunId;
                return (
                  <li key={run.runId}>
                    <button
                      type="button"
                      onClick={() => setSelectedRunId(run.runId)}
                      className={`flex w-full flex-col gap-1 border-b border-[#F3F4F6] px-4 py-3 text-left transition ${
                        active ? 'bg-[#F9FAFB]' : 'hover:bg-[#FAFAFA]'
                      }`}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="truncate font-mono text-[11px] text-gray-500">
                          {run.runId}
                        </span>
                        <span
                          className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${STATUS_STYLE[run.status]}`}
                        >
                          {run.status}
                        </span>
                      </div>
                      <span className="text-[11px] text-gray-400">
                        {new Date(run.startedAt).toLocaleString()}
                        {run.rating ? ` · rated ${run.rating}` : ''}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </div>

      {/* Detail */}
      <div className="flex-1 overflow-y-auto">
        {selectedRunId ? (
          <RunDetail runId={selectedRunId} />
        ) : (
          <div className="flex h-full items-center justify-center text-[13px] text-gray-400">
            Select a run to inspect its activity.
          </div>
        )}
      </div>
    </div>
  );
}

function RunDetail({ runId }: { runId: string }): ReactElement {
  const events = useRunEvents(runId);
  const [deciding, setDeciding] = useState(false);
  const [decision, setDecision] = useState<string | null>(null);

  const decide = useCallback(
    async (verb: 'approve' | 'reject') => {
      if (!events.pendingApprovalId) return;
      setDeciding(true);
      try {
        const res = await fetch(
          `/api/agents/approvals/${encodeURIComponent(events.pendingApprovalId)}/decide`,
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ decision: verb }),
          },
        );
        setDecision(res.ok ? `${verb}d` : 'failed');
      } catch {
        setDecision('failed');
      } finally {
        setDeciding(false);
      }
    },
    [events.pendingApprovalId],
  );

  const plans = Object.values(events.plans);
  const approvals = Object.values(events.approvals);

  return (
    <div className="p-6">
      <div className="mb-4 flex items-center gap-2">
        <span className="font-mono text-[12px] text-gray-500">{runId}</span>
        <span
          className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${
            events.status === 'open' ? 'bg-green-50 text-green-700' : 'bg-gray-100 text-gray-500'
          }`}
        >
          {events.status}
        </span>
      </div>

      {events.paused && (
        <div className="mb-4 rounded-lg border border-amber-200 bg-amber-50 p-3">
          <p className="text-[12px] font-medium text-amber-800">
            Paused — awaiting your approval
          </p>
          {events.pendingApprovalId ? (
            <p className="mt-0.5 font-mono text-[10px] text-amber-600">
              {events.pendingApprovalId}
            </p>
          ) : null}
          <div className="mt-3 flex gap-2">
            <button
              type="button"
              disabled={deciding}
              onClick={() => void decide('approve')}
              className="rounded-full bg-[#111111] px-3 py-1.5 text-[12px] font-medium text-white disabled:opacity-50"
            >
              Approve
            </button>
            <button
              type="button"
              disabled={deciding}
              onClick={() => void decide('reject')}
              className="rounded-full border border-[#e5e7eb] px-3 py-1.5 text-[12px] font-medium text-[#374151] disabled:opacity-50"
            >
              Reject
            </button>
            {decision ? (
              <span className="self-center text-[11px] text-gray-500">{decision}</span>
            ) : null}
          </div>
        </div>
      )}

      <Section title="Plan" empty={plans.length === 0}>
        {plans.map((p) => (
          <li key={p.planId} className="flex justify-between gap-2">
            <span className="truncate font-mono text-[10px] text-gray-400">{p.planId}</span>
            <span className="text-gray-700">{p.to}</span>
          </li>
        ))}
      </Section>

      <Section title="Approvals" empty={approvals.length === 0}>
        {approvals.map((a) => (
          <li key={a.approvalId} className="flex justify-between gap-2">
            <span className="text-gray-700">{a.kind}</span>
            <span className="text-gray-500">{a.state}</span>
          </li>
        ))}
      </Section>
    </div>
  );
}

function Section({
  title,
  empty,
  children,
}: {
  title: string;
  empty: boolean;
  children: React.ReactNode;
}): ReactElement {
  return (
    <div className="mb-4">
      <h4 className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-gray-400">
        {title}
      </h4>
      {empty ? (
        <p className="text-[11px] text-gray-400">—</p>
      ) : (
        <ul className="space-y-1 text-[12px]">{children}</ul>
      )}
    </div>
  );
}
