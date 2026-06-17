'use client';

import { Cloud, CloudOff, Database, FileText, Layout, Loader2, RefreshCw, Users } from 'lucide-react';
import {
  usePlanner,
  type PlannerSyncStatus,
  type EditorMode,
  type PlannerPresenceScope,
} from './providers/PlannerProvider';

// ── Sync status badge ──────────────────────────────────────────────────────

function SyncBadge({ status }: { status: PlannerSyncStatus }) {
  if (status === 'initialising') {
    return (
      <span className="flex items-center gap-1 text-xs text-muted-foreground">
        <Loader2 className="h-3 w-3 animate-spin" />
        Initialising…
      </span>
    );
  }
  if (status === 'syncing') {
    return (
      <span className="flex items-center gap-1 text-xs text-blue-500">
        <Loader2 className="h-3 w-3 animate-spin" />
        Syncing…
      </span>
    );
  }
  if (status === 'persisted') {
    return (
      <span className="flex items-center gap-1 text-xs text-emerald-700">
        <Database className="h-3 w-3" />
        Saved
      </span>
    );
  }
  if (status === 'synced') {
    return (
      <span className="flex items-center gap-1 text-xs text-green-600">
        <Cloud className="h-3 w-3" />
        Synced
      </span>
    );
  }
  if (status === 'error') {
    return (
      <span className="flex items-center gap-1 text-xs text-amber-600">
        <CloudOff className="h-3 w-3" />
        Local only
      </span>
    );
  }
  // fallback
  return (
    <span className="flex items-center gap-1 text-xs text-muted-foreground">
      <CloudOff className="h-3 w-3" />
      Unavailable
    </span>
  );
}

// ── Mode toggle button ─────────────────────────────────────────────────────

function ModeToggle({ mode, onChange }: { mode: EditorMode; onChange: (m: EditorMode) => void }) {
  return (
    <div className="flex items-center rounded-md border border-border bg-muted/40 p-0.5">
      <button
        onClick={() => onChange('page')}
        title="Document mode"
        className={`flex items-center gap-1.5 rounded px-2 py-1 text-xs transition-colors ${
          mode === 'page'
            ? 'bg-background text-foreground shadow-sm'
            : 'text-muted-foreground hover:text-foreground'
        }`}
      >
        <FileText className="h-3.5 w-3.5" />
        Doc
      </button>
      <button
        onClick={() => onChange('edgeless')}
        title="Canvas / whiteboard mode"
        className={`flex items-center gap-1.5 rounded px-2 py-1 text-xs transition-colors ${
          mode === 'edgeless'
            ? 'bg-background text-foreground shadow-sm'
            : 'text-muted-foreground hover:text-foreground'
        }`}
      >
        <Layout className="h-3.5 w-3.5" />
        Canvas
      </button>
    </div>
  );
}

function PresenceBadge({
  liveEditors,
  presenceScope,
}: {
  liveEditors: number;
  presenceScope: PlannerPresenceScope;
}) {
  return (
    <span className="flex items-center gap-1 text-xs text-stone-500">
      <Users className="h-3 w-3" />
      {presenceScope === 'realtime'
        ? liveEditors > 1
          ? `${liveEditors} live editors`
          : '1 live editor'
        : 'Local only'}
    </span>
  );
}

function formatSavedAt(timestamp: number | null) {
  if (!timestamp) {
    return null;
  }

  return new Intl.DateTimeFormat(undefined, {
    hour: 'numeric',
    minute: '2-digit',
  }).format(timestamp);
}

// ── Toolbar ────────────────────────────────────────────────────────────────

export function PlannerToolbar() {
  const {
    documentTitle,
    lastSavedAt,
    liveEditors,
    mode,
    presenceScope,
    syncStatus,
    syncError,
    setMode,
    reinitialise,
  } = usePlanner();
  const savedAt = formatSavedAt(lastSavedAt);

  return (
    <div className="flex h-10 shrink-0 items-center gap-3 border-b border-border bg-background/80 px-4 backdrop-blur-sm">
      <div className="min-w-0">
        <span className="block truncate text-sm font-medium text-foreground">{documentTitle}</span>
      </div>

      <div className="mx-1 h-4 w-px bg-border" />

      <ModeToggle mode={mode} onChange={setMode} />

      <div className="ml-auto flex items-center gap-2">
        <PresenceBadge liveEditors={liveEditors} presenceScope={presenceScope} />

        <SyncBadge status={syncStatus} />

        {savedAt ? <span className="text-xs text-muted-foreground">Saved {savedAt}</span> : null}

        {syncStatus === 'error' && (
          <button
            onClick={reinitialise}
            title={syncError ?? 'Reload planner workspace'}
            className="flex items-center gap-1 rounded px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <RefreshCw className="h-3 w-3" />
            Retry
          </button>
        )}
      </div>
    </div>
  );
}
