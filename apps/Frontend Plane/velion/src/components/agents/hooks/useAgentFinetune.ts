'use client';

import { useCallback, useEffect, useState } from 'react';

import type { PersistedAgent } from '../types';

/**
 * Wave 7 (verevon ui-ux-verevon-gap.md §17): per-agent fine-tuning jobs.
 *
 * Drives the `FinetuneTab` in `AgentWorkspaceView`. The Convex side is
 * NOT subscribed (jobs live in capability-core's Postgres, not Convex);
 * we poll every 30s while the tab is mounted and after every mutation.
 */

export type FinetuneStatus =
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

/**
 * Wire-shape returned by the Model Plane gateway's
 * `/v1/finetune/jobs` surface (see `apps/Model Plane/rust/services/
 * model-gateway/src/finetune_routes.rs::job_value`). The proxy
 * (`/api/agents/[agentId]/finetune`) is a pass-through, so this
 * interface tracks the backend's field names exactly — keep them in
 * sync when the proto evolves.
 *
 * Snake-case is intentional: matching the gateway envelope verbatim
 * means no per-row transform in the proxy or hook (cheap + immune to
 * naming drift in JSON deserialisation).
 */
export interface FinetuneJob {
  job_id: string;
  org_id: string;
  agent_id: string;
  created_by: string;
  base_model: string;
  azure_file_id: string;
  azure_job_id: string;
  fine_tuned_model: string;
  deployment_name: string;
  status: FinetuneStatus;
  error_message: string;
  hyperparameters?: string;
  training_example_count: number;
  estimated_cost_usd: number;
  actual_cost_usd: number;
  created_at: number | null;
  updated_at: number | null;
  completed_at: number | null;
}

interface UseAgentFinetuneReturn {
  jobs: FinetuneJob[];
  isLoading: boolean;
  error: string | null;
  createJob: (input: {
    file: File;
    baseModel: string;
    hyperparameters?: Record<string, unknown>;
  }) => Promise<void>;
  cancelJob: (jobId: string) => Promise<void>;
  reload: () => Promise<void>;
}

const POLL_MS = 30_000;

export function useAgentFinetune(
  agent: PersistedAgent | null,
): UseAgentFinetuneReturn {
  const [jobs, setJobs] = useState<FinetuneJob[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async (): Promise<void> => {
    if (!agent) {
      setJobs([]);
      setIsLoading(false);
      return;
    }
    try {
      const res = await fetch(`/api/agents/${agent.id}/finetune`, {
        cache: 'no-store',
      });
      if (!res.ok) {
        throw new Error(`list failed (${res.status})`);
      }
      // Gateway envelope: `{ jobs: FinetuneJob[], total: number }`
      // (see finetune_routes.rs::list_jobs). Tolerate two legacy shapes
      // (`items`, bare array) so a stale proxy mid-deploy doesn't break
      // the UI, but the canonical key is `jobs`.
      const payload = (await res.json()) as
        | { jobs?: FinetuneJob[]; items?: FinetuneJob[] }
        | FinetuneJob[];
      setJobs(
        Array.isArray(payload)
          ? payload
          : (payload.jobs ?? payload.items ?? []),
      );
      setError(null);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'unknown');
      setJobs([]);
    } finally {
      setIsLoading(false);
    }
  }, [agent]);

  useEffect(() => {
    void reload();
    const t = setInterval(reload, POLL_MS);
    return () => clearInterval(t);
  }, [reload]);

  const createJob = useCallback(
    async (input: {
      file: File;
      baseModel: string;
      hyperparameters?: Record<string, unknown>;
    }): Promise<void> => {
      if (!agent) return;
      const fd = new FormData();
      fd.append('file', input.file, input.file.name);
      fd.append('base_model', input.baseModel);
      if (input.hyperparameters) {
        fd.append('hyperparameters', JSON.stringify(input.hyperparameters));
      }
      try {
        const res = await fetch(`/api/agents/${agent.id}/finetune`, {
          method: 'POST',
          body: fd,
        });
        if (!res.ok) {
          const body = (await res.json().catch(() => ({}))) as {
            error?: string;
            message?: string;
          };
          throw new Error(
            body.error ?? body.message ?? `kickoff failed (${res.status})`,
          );
        }
        await reload();
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'kickoff failed');
      }
    },
    [agent, reload],
  );

  const cancelJob = useCallback(
    async (jobId: string): Promise<void> => {
      if (!agent) return;
      try {
        const res = await fetch(
          `/api/agents/${agent.id}/finetune/${encodeURIComponent(jobId)}`,
          { method: 'DELETE' },
        );
        if (!res.ok && res.status !== 404) {
          throw new Error(`cancel failed (${res.status})`);
        }
        await reload();
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'cancel failed');
      }
    },
    [agent, reload],
  );

  return { jobs, isLoading, error, createJob, cancelJob, reload };
}
