"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import {
  ArrowUpRight,
  CalendarClock,
  Database,
  FileSearch,
  Globe,
  Play,
  RefreshCw,
  ScanSearch,
  ShieldCheck,
  TimerReset,
} from "lucide-react";
import {
  VerevonButton,
  VerevonInput,
  VerevonSegmented,
  VerevonSegmentedButton,
  VerevonSelect,
  VerevonTextarea,
} from "@/components/ui/verevon-ui";
import { apiGet, apiSend } from "@/lib/api/client-envelope";
import { cn } from "@/lib/utils";

type IngestionView = "runs" | "schedules" | "sources" | "evidence" | "profiles";

type RunItem = {
  id: string;
  kind: string;
  status: string;
  createdAt: string;
  startedAt?: string | null;
  completedAt?: string | null;
  target: string;
  progress: {
    completed?: number;
    total?: number | null;
    pages?: number;
    urlCount?: number;
    query?: string | null;
  };
  stats: Record<string, unknown>;
};

type ScheduleItem = {
  id: string;
  name: string;
  kind: string;
  status: string;
  cron?: string | null;
  scheduleAt?: string | null;
  createdAt: string;
  lastRunAt?: string | null;
  nextRunAt?: string | null;
  target: string;
  config: Record<string, unknown>;
};

type EvidenceTimeline = {
  runId: string;
  timeline: Array<{
    run_id: string;
    kind: string;
    stage: string;
    status: string;
    seq: number;
    completed: number;
    total?: number | null;
    discovered: number;
    queued: number;
    retries: number;
    blocks: number;
    timestamp: string;
    payload?: Record<string, unknown>;
  }>;
  warnings: Array<{ id: string; stage: string; summary: string }>;
};

type ManualEvidence =
  | {
      kind: "scrape";
      targetUrl: string;
      extractedAt: string;
      fingerprint: string;
      statusCode: number;
      metadata: Record<string, unknown>;
      driver: Record<string, unknown>;
      formats: Record<string, unknown>;
      sourceTrace: Record<string, unknown> | null;
    }
  | {
      kind: "extract";
      targetUrl: string;
      extractedAt: string;
      result: unknown;
      results: unknown[];
    };

type SourcePayload = {
  graph?: { available?: boolean; nodeCount?: number; edgeCount?: number };
  integrations: Array<{
    id: string;
    title: string;
    provider: string;
    status: string;
    detail: string;
    counts: Record<string, number>;
    capabilities: string[];
    samples: Array<{ kind: string; label: string }>;
    readOnly: boolean;
  }>;
  quarrySources: Array<{
    id: string;
    name: string;
    url: string;
    kind: string;
    status: string;
    createdAt: string;
    updatedAt: string;
    config: Record<string, unknown>;
  }>;
};

type ProfilePayload = {
  profiles: Array<{
    id: string;
    restorable: boolean;
    cookies: number;
    storage: number;
    locale?: string | null;
    timezone?: string | null;
  }>;
};

type RunCreateResult = {
  run: {
    id: string;
    kind: string;
    status: string;
    createdAt: string;
    target: string;
  };
  evidence?: ManualEvidence;
};

const views: Array<{ id: IngestionView; label: string; icon: React.ReactNode }> = [
  { id: "runs", label: "Runs", icon: <ScanSearch className="size-4" /> },
  { id: "schedules", label: "Schedules", icon: <CalendarClock className="size-4" /> },
  { id: "sources", label: "Sources", icon: <Globe className="size-4" /> },
  { id: "evidence", label: "Evidence", icon: <FileSearch className="size-4" /> },
  { id: "profiles", label: "Profiles", icon: <ShieldCheck className="size-4" /> },
];

export function VerevonIngestionsPage() {
  const [activeView, setActiveView] = useState<IngestionView>("runs");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [runs, setRuns] = useState<RunItem[]>([]);
  const [schedules, setSchedules] = useState<ScheduleItem[]>([]);
  const [sources, setSources] = useState<SourcePayload | null>(null);
  const [profiles, setProfiles] = useState<ProfilePayload | null>(null);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [runEvidence, setRunEvidence] = useState<EvidenceTimeline | null>(null);
  const [manualEvidence, setManualEvidence] = useState<ManualEvidence | null>(null);
  const [runForm, setRunForm] = useState({
    kind: "scrape",
    url: "",
    urls: "",
    prompt: "",
  });
  const [scheduleForm, setScheduleForm] = useState({
    name: "",
    kind: "crawl",
    targetUrl: "",
    cron: "0 7 * * *",
  });

  const selectedRun = useMemo(
    () => runs.find((run) => run.id === selectedRunId) ?? null,
    [runs, selectedRunId],
  );

  async function loadWorkspace() {
    setLoading(true);
    setError(null);
    try {
      const [runData, scheduleData, sourceData, profileData] = await Promise.all([
        apiGet<RunItem[]>("/api/ingestions/runs"),
        apiGet<ScheduleItem[]>("/api/ingestions/schedules"),
        apiGet<SourcePayload>("/api/ingestions/sources"),
        apiGet<ProfilePayload>("/api/ingestions/profiles"),
      ]);
      setRuns(runData);
      setSchedules(scheduleData);
      setSources(sourceData);
      setProfiles(profileData);
      if (!selectedRunId && runData[0]) setSelectedRunId(runData[0].id);
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Could not load ingestion workspace.");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadWorkspace();
  }, []);

  useEffect(() => {
    if (!selectedRunId) return;
    apiGet<EvidenceTimeline>(`/api/ingestions/evidence?runId=${encodeURIComponent(selectedRunId)}`)
      .then(setRunEvidence)
      .catch(() => setRunEvidence(null));
  }, [selectedRunId]);

  async function submitRun(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    try {
      const payload =
        runForm.kind === "batch"
          ? {
              kind: "batch",
              urls: runForm.urls
                .split(/\n|,/)
                .map((value) => value.trim())
                .filter(Boolean),
            }
          : {
              kind: runForm.kind,
              url: runForm.url.trim(),
              prompt: runForm.prompt.trim() || undefined,
            };
      const created = await apiSend<RunCreateResult>("/api/ingestions/runs", payload);
      if (created.evidence) {
        setManualEvidence(created.evidence);
        setActiveView("evidence");
      } else {
        setSelectedRunId(created.run.id);
        setActiveView("runs");
      }
      await loadWorkspace();
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Run could not be started.");
    }
  }

  async function createSchedule(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    try {
      await apiSend<ScheduleItem>("/api/ingestions/schedules", {
        name: scheduleForm.name,
        kind: scheduleForm.kind,
        targetUrl: scheduleForm.targetUrl,
        cron: scheduleForm.cron,
      });
      setScheduleForm((current) => ({ ...current, name: "", targetUrl: "" }));
      await loadWorkspace();
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Schedule could not be created.");
    }
  }

  async function runScheduleAction(action: string, scheduleId: string) {
    setError(null);
    try {
      await apiSend("/api/ingestions/actions", {
        action,
        scheduleId,
      });
      await loadWorkspace();
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Schedule action failed.");
    }
  }

  return (
    <div className="verevon-page-surface h-full min-h-0 overflow-y-auto">
      <div className="mx-auto flex min-h-full w-full max-w-[1560px] flex-col gap-5 p-4 sm:p-5 lg:p-7">
        <header className="flex flex-col gap-4 border-b border-[#DDDCD6] pb-5 dark:border-[#292B31] lg:flex-row lg:items-end lg:justify-between">
          <div className="min-w-0">
            <h1 className="verevon-page-title sm:text-[34px]">Ingestions</h1>
            <p className="verevon-page-body mt-3 max-w-3xl">
              Run crawls and extracts, inspect evidence, manage recurring schedules, and hand trusted sources back into Knowledge.
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <VerevonSegmented>
              {views.map((view) => (
                <VerevonSegmentedButton
                  key={view.id}
                  aria-pressed={activeView === view.id}
                  onClick={() => setActiveView(view.id)}
                >
                  {view.icon}
                  {view.label}
                </VerevonSegmentedButton>
              ))}
            </VerevonSegmented>
            <VerevonButton radius="sm" className="px-3" onClick={() => void loadWorkspace()}>
              <RefreshCw className={cn("size-4", loading && "animate-spin")} />
              Refresh
            </VerevonButton>
          </div>
        </header>

        {error ? (
          <section className="verevon-panel border border-[#E7C98B] bg-[#FFF6E5] p-4 text-[13px] text-[#6E5220] dark:border-[#5A4520] dark:bg-[#2A2214] dark:text-[#E6C27A]">
            {error}
          </section>
        ) : null}

        {activeView === "runs" ? (
          <section className="grid gap-4 xl:grid-cols-[360px_minmax(0,1fr)]">
            <RunComposer
              form={runForm}
              setForm={setRunForm}
              onSubmit={submitRun}
            />
            <RunsPanel
              runs={runs}
              selectedRunId={selectedRunId}
              onSelectRun={(runId) => {
                setSelectedRunId(runId);
                setActiveView("evidence");
              }}
            />
          </section>
        ) : null}

        {activeView === "schedules" ? (
          <section className="grid gap-4 xl:grid-cols-[360px_minmax(0,1fr)]">
            <ScheduleComposer form={scheduleForm} setForm={setScheduleForm} onSubmit={createSchedule} />
            <SchedulesPanel schedules={schedules} onAction={runScheduleAction} />
          </section>
        ) : null}

        {activeView === "sources" ? (
          <SourcesPanel sources={sources} />
        ) : null}

        {activeView === "evidence" ? (
          <EvidencePanel
            selectedRun={selectedRun}
            selectedRunId={selectedRunId}
            runEvidence={runEvidence}
            manualEvidence={manualEvidence}
            runs={runs}
            onSelectRun={setSelectedRunId}
          />
        ) : null}

        {activeView === "profiles" ? (
          <ProfilesPanel profiles={profiles} />
        ) : null}
      </div>
    </div>
  );
}

function RunComposer({
  form,
  setForm,
  onSubmit,
}: {
  form: { kind: string; url: string; urls: string; prompt: string };
  setForm: React.Dispatch<React.SetStateAction<{ kind: string; url: string; urls: string; prompt: string }>>;
  onSubmit: (event: React.FormEvent<HTMLFormElement>) => Promise<void>;
}) {
  const isBatch = form.kind === "batch";

  return (
    <form onSubmit={(event) => void onSubmit(event)} className="verevon-panel flex flex-col gap-4 p-5">
      <div>
        <h2 className="text-[20px] font-semibold text-[#111111] dark:text-white">Start a run</h2>
        <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
          Manual parity for scrape, crawl, extract, and batch execution.
        </p>
      </div>
      <label className="flex flex-col gap-2 text-[12px] font-medium text-[#555B52] dark:text-[#B7BEC9]">
        Run type
        <VerevonSelect value={form.kind} onChange={(event) => setForm((current) => ({ ...current, kind: event.target.value }))}>
          <option value="scrape">Scrape</option>
          <option value="crawl">Crawl</option>
          <option value="extract">Extract</option>
          <option value="batch">Batch</option>
        </VerevonSelect>
      </label>
      {isBatch ? (
        <label className="flex flex-col gap-2 text-[12px] font-medium text-[#555B52] dark:text-[#B7BEC9]">
          URLs
          <VerevonTextarea
            rows={6}
            value={form.urls}
            onChange={(event) => setForm((current) => ({ ...current, urls: event.target.value }))}
            placeholder="https://example.com/pricing&#10;https://example.com/docs"
          />
        </label>
      ) : (
        <label className="flex flex-col gap-2 text-[12px] font-medium text-[#555B52] dark:text-[#B7BEC9]">
          Target URL
          <VerevonInput
            value={form.url}
            onChange={(event) => setForm((current) => ({ ...current, url: event.target.value }))}
            placeholder="https://example.com"
          />
        </label>
      )}
      {form.kind === "extract" ? (
        <label className="flex flex-col gap-2 text-[12px] font-medium text-[#555B52] dark:text-[#B7BEC9]">
          Extraction prompt
          <VerevonTextarea
            rows={4}
            value={form.prompt}
            onChange={(event) => setForm((current) => ({ ...current, prompt: event.target.value }))}
            placeholder="Extract key support topics, contact channels, and pricing signals."
          />
        </label>
      ) : null}
      <VerevonButton variant="primary" radius="sm" className="w-full justify-center">
        <Play className="size-4" />
        Start run
      </VerevonButton>
    </form>
  );
}

function RunsPanel({
  runs,
  selectedRunId,
  onSelectRun,
}: {
  runs: RunItem[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
}) {
  return (
    <section className="verevon-panel p-5">
      <div className="flex items-end justify-between gap-3">
        <div>
          <h2 className="text-[20px] font-semibold text-[#111111] dark:text-white">Recent runs</h2>
          <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
            Durable crawl, extract, batch, search, and agent jobs from Quarry.
          </p>
        </div>
        <Link href="/knowledge" className="inline-flex items-center gap-2 text-[12px] font-medium text-[#4C5A87] hover:text-[#2C3B67] dark:text-[#B8C6FF]">
          Open Knowledge
          <ArrowUpRight className="size-4" />
        </Link>
      </div>
      <div className="mt-4 overflow-hidden rounded-[14px] border border-[#E5E4DD] dark:border-[#2A2D33]">
        <table className="min-w-full divide-y divide-[#E5E4DD] text-left text-[13px] dark:divide-[#2A2D33]">
          <thead className="bg-[#FBFAF6] text-[#6D7169] dark:bg-[#181A1F] dark:text-[#9EA5B1]">
            <tr>
              <th className="px-4 py-3 font-medium">Kind</th>
              <th className="px-4 py-3 font-medium">Target</th>
              <th className="px-4 py-3 font-medium">Status</th>
              <th className="px-4 py-3 font-medium">Created</th>
              <th className="px-4 py-3 font-medium">Progress</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[#EDEBE5] dark:divide-[#23262C]">
            {runs.map((run) => (
              <tr
                key={run.id}
                className={cn(
                  "cursor-pointer bg-white transition-colors hover:bg-[#FAFAF7] dark:bg-[#121419] dark:hover:bg-[#171A20]",
                  selectedRunId === run.id && "bg-[#F6F7FB] dark:bg-[#1A1E28]",
                )}
                onClick={() => onSelectRun(run.id)}
              >
                <td className="px-4 py-3 capitalize text-[#111111] dark:text-white">{run.kind}</td>
                <td className="max-w-[320px] truncate px-4 py-3 text-[#434A42] dark:text-[#CBD2DD]">{run.target}</td>
                <td className="px-4 py-3"><StatusBadge status={run.status} /></td>
                <td className="px-4 py-3 text-[#6D7169] dark:text-[#9EA5B1]">{relativeTime(run.createdAt)}</td>
                <td className="px-4 py-3 text-[#434A42] dark:text-[#CBD2DD]">
                  {run.progress.completed ?? 0}
                  {run.progress.total ? ` / ${run.progress.total}` : ""}
                </td>
              </tr>
            ))}
            {runs.length === 0 ? (
              <tr>
                <td colSpan={5} className="px-4 py-8 text-center text-[#6D7169] dark:text-[#9EA5B1]">
                  No durable runs yet.
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function ScheduleComposer({
  form,
  setForm,
  onSubmit,
}: {
  form: { name: string; kind: string; targetUrl: string; cron: string };
  setForm: React.Dispatch<React.SetStateAction<{ name: string; kind: string; targetUrl: string; cron: string }>>;
  onSubmit: (event: React.FormEvent<HTMLFormElement>) => Promise<void>;
}) {
  return (
    <form onSubmit={(event) => void onSubmit(event)} className="verevon-panel flex flex-col gap-4 p-5">
      <div>
        <h2 className="text-[20px] font-semibold text-[#111111] dark:text-white">Create schedule</h2>
        <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
          Recurring ingestion for sources that should stay fresh without manual runs.
        </p>
      </div>
      <label className="flex flex-col gap-2 text-[12px] font-medium text-[#555B52] dark:text-[#B7BEC9]">
        Name
        <VerevonInput value={form.name} onChange={(event) => setForm((current) => ({ ...current, name: event.target.value }))} placeholder="Docs crawl" />
      </label>
      <label className="flex flex-col gap-2 text-[12px] font-medium text-[#555B52] dark:text-[#B7BEC9]">
        Kind
        <VerevonSelect value={form.kind} onChange={(event) => setForm((current) => ({ ...current, kind: event.target.value }))}>
          <option value="crawl">Crawl</option>
          <option value="extract">Extract</option>
          <option value="search">Search</option>
          <option value="batch">Batch</option>
          <option value="agent">Agent</option>
        </VerevonSelect>
      </label>
      <label className="flex flex-col gap-2 text-[12px] font-medium text-[#555B52] dark:text-[#B7BEC9]">
        Target URL
        <VerevonInput value={form.targetUrl} onChange={(event) => setForm((current) => ({ ...current, targetUrl: event.target.value }))} placeholder="https://example.com/docs" />
      </label>
      <label className="flex flex-col gap-2 text-[12px] font-medium text-[#555B52] dark:text-[#B7BEC9]">
        Cron
        <VerevonInput value={form.cron} onChange={(event) => setForm((current) => ({ ...current, cron: event.target.value }))} placeholder="0 7 * * *" />
      </label>
      <VerevonButton variant="primary" radius="sm" className="w-full justify-center">
        <CalendarClock className="size-4" />
        Save schedule
      </VerevonButton>
    </form>
  );
}

function SchedulesPanel({
  schedules,
  onAction,
}: {
  schedules: ScheduleItem[];
  onAction: (action: string, scheduleId: string) => Promise<void>;
}) {
  return (
    <section className="verevon-panel p-5">
      <div>
        <h2 className="text-[20px] font-semibold text-[#111111] dark:text-white">Schedule lifecycle</h2>
        <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
          Pause, resume, trigger, and retire recurring jobs from one surface.
        </p>
      </div>
      <div className="mt-4 grid gap-3">
        {schedules.map((schedule) => (
          <article key={schedule.id} className="rounded-[14px] border border-[#E5E4DD] bg-white p-4 dark:border-[#2A2D33] dark:bg-[#121419]">
            <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <h3 className="truncate text-[15px] font-semibold text-[#111111] dark:text-white">{schedule.name}</h3>
                  <StatusBadge status={schedule.status} />
                </div>
                <p className="mt-1 text-[13px] text-[#505751] dark:text-[#BBC2CD]">{schedule.target}</p>
                <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[12px] text-[#6D7169] dark:text-[#9EA5B1]">
                  <span className="capitalize">{schedule.kind}</span>
                  <span>{schedule.cron || schedule.scheduleAt || "Manual cadence"}</span>
                  <span>Next: {schedule.nextRunAt ? relativeTime(schedule.nextRunAt) : "Not scheduled"}</span>
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                {schedule.status === "paused" ? (
                  <VerevonButton size="sm" radius="sm" onClick={() => void onAction("unpause_schedule", schedule.id)}>
                    <Play className="size-4" />
                    Resume
                  </VerevonButton>
                ) : (
                  <VerevonButton size="sm" radius="sm" onClick={() => void onAction("pause_schedule", schedule.id)}>
                    <TimerReset className="size-4" />
                    Pause
                  </VerevonButton>
                )}
                <VerevonButton size="sm" radius="sm" onClick={() => void onAction("trigger_schedule", schedule.id)}>
                  <RefreshCw className="size-4" />
                  Trigger
                </VerevonButton>
                <VerevonButton size="sm" radius="sm" onClick={() => void onAction("delete_schedule", schedule.id)}>
                  Retire
                </VerevonButton>
              </div>
            </div>
          </article>
        ))}
        {schedules.length === 0 ? (
          <div className="rounded-[14px] border border-dashed border-[#D7D4CC] p-8 text-center text-[13px] text-[#6D7169] dark:border-[#2A2D33] dark:text-[#9EA5B1]">
            No recurring schedules yet.
          </div>
        ) : null}
      </div>
    </section>
  );
}

function SourcesPanel({ sources }: { sources: SourcePayload | null }) {
  return (
    <section className="grid gap-4 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)]">
      <div className="verevon-panel p-5">
        <div className="flex items-end justify-between gap-3">
          <div>
            <h2 className="text-[20px] font-semibold text-[#111111] dark:text-white">Connected sources</h2>
            <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
              Integration-backed knowledge and website ingestion targets visible to Verevon.
            </p>
          </div>
          <Link href="/knowledge" className="inline-flex items-center gap-2 text-[12px] font-medium text-[#4C5A87] hover:text-[#2C3B67] dark:text-[#B8C6FF]">
            Open Knowledge
            <ArrowUpRight className="size-4" />
          </Link>
        </div>
        <div className="mt-4 grid gap-3">
          {(sources?.integrations ?? []).map((source) => (
            <article key={source.id} className="rounded-[14px] border border-[#E5E4DD] bg-white p-4 dark:border-[#2A2D33] dark:bg-[#121419]">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <h3 className="text-[15px] font-semibold text-[#111111] dark:text-white">{source.title}</h3>
                  <p className="mt-1 text-[12px] text-[#6D7169] dark:text-[#9EA5B1]">{source.provider} • {source.detail}</p>
                </div>
                <StatusBadge status={source.status} />
              </div>
              {source.capabilities.length ? (
                <div className="mt-3 flex flex-wrap gap-2">
                  {source.capabilities.slice(0, 4).map((capability) => (
                    <span key={capability} className="rounded-full bg-[#F4F2EB] px-2.5 py-1 text-[11px] text-[#555B52] dark:bg-[#1A1D23] dark:text-[#C6CDD8]">
                      {capability}
                    </span>
                  ))}
                </div>
              ) : null}
            </article>
          ))}
        </div>
      </div>
      <div className="verevon-panel p-5">
        <h2 className="text-[20px] font-semibold text-[#111111] dark:text-white">Tracked web sources</h2>
        <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
          Durable source resources registered in Quarry for recurring refresh and review.
        </p>
        <div className="mt-4 grid gap-3">
          {(sources?.quarrySources ?? []).map((source) => (
            <article key={source.id} className="rounded-[14px] border border-[#E5E4DD] bg-white p-4 dark:border-[#2A2D33] dark:bg-[#121419]">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <h3 className="truncate text-[15px] font-semibold text-[#111111] dark:text-white">{source.name}</h3>
                  <p className="mt-1 truncate text-[12px] text-[#6D7169] dark:text-[#9EA5B1]">{source.url}</p>
                </div>
                <StatusBadge status={source.status} />
              </div>
              <div className="mt-3 flex items-center justify-between text-[12px] text-[#6D7169] dark:text-[#9EA5B1]">
                <span className="capitalize">{source.kind}</span>
                <span>{relativeTime(source.updatedAt)}</span>
              </div>
            </article>
          ))}
          {(sources?.quarrySources ?? []).length === 0 ? (
            <div className="rounded-[14px] border border-dashed border-[#D7D4CC] p-8 text-center text-[13px] text-[#6D7169] dark:border-[#2A2D33] dark:text-[#9EA5B1]">
              Quarry has no durable source records yet.
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}

function EvidencePanel({
  selectedRun,
  selectedRunId,
  runEvidence,
  manualEvidence,
  runs,
  onSelectRun,
}: {
  selectedRun: RunItem | null;
  selectedRunId: string | null;
  runEvidence: EvidenceTimeline | null;
  manualEvidence: ManualEvidence | null;
  runs: RunItem[];
  onSelectRun: (runId: string) => void;
}) {
  return (
    <section className="grid gap-4 xl:grid-cols-[340px_minmax(0,1fr)]">
      <aside className="verevon-panel p-5">
        <h2 className="text-[20px] font-semibold text-[#111111] dark:text-white">Evidence focus</h2>
        <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
          Inspect provenance and warnings before trusting or operationalizing a result.
        </p>
        <div className="mt-4 grid gap-2">
          {runs.slice(0, 12).map((run) => (
            <button
              key={run.id}
              type="button"
              onClick={() => onSelectRun(run.id)}
              className={cn(
                "rounded-[12px] border px-3 py-3 text-left transition",
                selectedRunId === run.id
                  ? "border-[#485681] bg-[#F5F7FD] dark:border-[#8897C9] dark:bg-[#172030]"
                  : "border-[#E5E4DD] bg-white hover:bg-[#FAFAF7] dark:border-[#2A2D33] dark:bg-[#121419] dark:hover:bg-[#171A20]",
              )}
            >
              <div className="flex items-center justify-between gap-3">
                <span className="text-[13px] font-semibold text-[#111111] dark:text-white">{run.kind}</span>
                <StatusBadge status={run.status} />
              </div>
              <p className="mt-1 truncate text-[12px] text-[#6D7169] dark:text-[#9EA5B1]">{run.target}</p>
            </button>
          ))}
        </div>
      </aside>
      <div className="verevon-panel p-5">
        <div className="flex items-end justify-between gap-3">
          <div>
            <h2 className="text-[20px] font-semibold text-[#111111] dark:text-white">Operational evidence</h2>
            <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
              {selectedRun
                ? `Timeline and warnings for ${selectedRun.target}.`
                : "Most recent manual scrape or extract output."}
            </p>
          </div>
          <Link href="/knowledge" className="inline-flex items-center gap-2 text-[12px] font-medium text-[#4C5A87] hover:text-[#2C3B67] dark:text-[#B8C6FF]">
            Send to Knowledge
            <ArrowUpRight className="size-4" />
          </Link>
        </div>

        {manualEvidence ? (
          <div className="mt-4 rounded-[14px] border border-[#E5E4DD] bg-white p-4 dark:border-[#2A2D33] dark:bg-[#121419]">
            <div className="flex items-center gap-2">
              <Database className="size-4 text-[#6D7169] dark:text-[#9EA5B1]" />
              <h3 className="text-[15px] font-semibold text-[#111111] dark:text-white">Latest manual {manualEvidence.kind}</h3>
            </div>
            <pre className="mt-3 overflow-x-auto rounded-[12px] bg-[#F7F6F1] p-4 text-[12px] text-[#3D403A] dark:bg-[#171A20] dark:text-[#CBD2DD]">
              {JSON.stringify(manualEvidence, null, 2)}
            </pre>
          </div>
        ) : null}

        {runEvidence ? (
          <div className="mt-4 grid gap-3">
            {runEvidence.warnings.length ? (
              <div className="rounded-[14px] border border-[#E7C98B] bg-[#FFF6E5] p-4 dark:border-[#5A4520] dark:bg-[#2A2214]">
                <h3 className="text-[14px] font-semibold text-[#6E5220] dark:text-[#E6C27A]">Warnings</h3>
                <ul className="mt-2 grid gap-2 text-[12px] text-[#6E5220] dark:text-[#E6C27A]">
                  {runEvidence.warnings.map((warning) => (
                    <li key={warning.id}>{warning.summary}</li>
                  ))}
                </ul>
              </div>
            ) : null}
            <div className="rounded-[14px] border border-[#E5E4DD] bg-white p-4 dark:border-[#2A2D33] dark:bg-[#121419]">
              <h3 className="text-[15px] font-semibold text-[#111111] dark:text-white">Run timeline</h3>
              <div className="mt-3 grid gap-3">
                {runEvidence.timeline.map((event) => (
                  <div key={`${event.run_id}:${event.seq}`} className="rounded-[12px] border border-[#ECE9E0] bg-[#FBFAF6] p-3 dark:border-[#23262C] dark:bg-[#171A20]">
                    <div className="flex items-center justify-between gap-3">
                      <div className="flex items-center gap-2">
                        <StatusBadge status={event.status} />
                        <span className="text-[13px] font-semibold capitalize text-[#111111] dark:text-white">{event.stage}</span>
                      </div>
                      <span className="text-[12px] text-[#6D7169] dark:text-[#9EA5B1]">{relativeTime(event.timestamp)}</span>
                    </div>
                    <p className="mt-2 text-[12px] text-[#555B52] dark:text-[#C6CDD8]">
                      Completed {event.completed}
                      {event.total ? ` / ${event.total}` : ""}, queued {event.queued}, discovered {event.discovered}, blocks {event.blocks}
                    </p>
                    {event.payload ? (
                      <pre className="mt-3 overflow-x-auto rounded-[10px] bg-white p-3 text-[11px] text-[#3D403A] dark:bg-[#111318] dark:text-[#CBD2DD]">
                        {JSON.stringify(event.payload, null, 2)}
                      </pre>
                    ) : null}
                  </div>
                ))}
              </div>
            </div>
          </div>
        ) : selectedRunId ? (
          <div className="mt-4 rounded-[14px] border border-dashed border-[#D7D4CC] p-8 text-center text-[13px] text-[#6D7169] dark:border-[#2A2D33] dark:text-[#9EA5B1]">
            No durable evidence timeline is available for this run yet.
          </div>
        ) : null}
      </div>
    </section>
  );
}

function ProfilesPanel({ profiles }: { profiles: ProfilePayload | null }) {
  return (
    <section className="verevon-panel p-5">
      <h2 className="text-[20px] font-semibold text-[#111111] dark:text-white">Profiles</h2>
      <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
        Browser/session profiles for protected sources and stateful refresh flows.
      </p>
      <div className="mt-4 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {(profiles?.profiles ?? []).map((profile) => (
          <article key={profile.id} className="rounded-[14px] border border-[#E5E4DD] bg-white p-4 dark:border-[#2A2D33] dark:bg-[#121419]">
            <div className="flex items-center justify-between gap-3">
              <h3 className="truncate text-[14px] font-semibold text-[#111111] dark:text-white">{profile.id}</h3>
              <StatusBadge status={profile.restorable ? "ready" : "review"} />
            </div>
            <div className="mt-3 grid gap-1 text-[12px] text-[#555B52] dark:text-[#C6CDD8]">
              <span>Cookies: {profile.cookies}</span>
              <span>Storage entries: {profile.storage}</span>
              <span>{profile.locale || "No locale"} • {profile.timezone || "No timezone"}</span>
            </div>
          </article>
        ))}
        {(profiles?.profiles ?? []).length === 0 ? (
          <div className="rounded-[14px] border border-dashed border-[#D7D4CC] p-8 text-center text-[13px] text-[#6D7169] dark:border-[#2A2D33] dark:text-[#9EA5B1]">
            No saved profiles yet.
          </div>
        ) : null}
      </div>
    </section>
  );
}

function StatusBadge({ status }: { status: string }) {
  const tone =
    status === "completed" || status === "active" || status === "connected" || status === "ready"
      ? "bg-[#EEF8F1] text-[#1E7A45] dark:bg-[#122119] dark:text-[#8BE0A7]"
      : status === "running" || status === "queued" || status === "syncing"
        ? "bg-[#F2EFFE] text-[#6A55B8] dark:bg-[#1C1930] dark:text-[#B8A8FF]"
        : status === "paused"
          ? "bg-[#F2F2F2] text-[#555] dark:bg-[#23252B] dark:text-[#B7BEC9]"
          : "bg-[#FFF7E8] text-[#9A661A] dark:bg-[#241B10] dark:text-[#EAB762]";

  return <span className={cn("rounded-full px-2.5 py-1 text-[11px] font-medium capitalize", tone)}>{status.replace(/_/g, " ")}</span>;
}

function relativeTime(value?: string | null) {
  if (!value) return "Unknown";
  const time = Date.parse(value);
  if (!Number.isFinite(time)) return value;
  const deltaMs = time - Date.now();
  const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ["day", 86_400_000],
    ["hour", 3_600_000],
    ["minute", 60_000],
  ];
  for (const [unit, amount] of units) {
    if (Math.abs(deltaMs) >= amount || unit === "minute") {
      return rtf.format(Math.round(deltaMs / amount), unit);
    }
  }
  return value;
}
