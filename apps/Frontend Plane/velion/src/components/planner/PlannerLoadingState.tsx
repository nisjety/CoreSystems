'use client';

import { Loader2, CloudOff, RefreshCw } from 'lucide-react';
import { usePlanner } from './providers/PlannerProvider';

/**
 * Shown while the workspace is initialising or has hit an error.
 * Renders nothing once the collection is ready (editor takes over).
 */
export function PlannerLoadingState() {
  const { syncStatus, syncError, collection, reinitialise } = usePlanner();

  // Collection ready — nothing to show here
  if (collection && syncStatus !== 'initialising') return null;

  if (syncStatus === 'initialising') {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-muted-foreground">
        <Loader2 className="h-8 w-8 animate-spin" />
        <p className="text-sm">Loading workspace…</p>
      </div>
    );
  }

  if (syncStatus === 'error') {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-4">
        <CloudOff className="h-10 w-10 text-amber-500" />
        <div className="text-center">
          <p className="font-medium text-foreground">Could not load workspace</p>
          {syncError && (
            <p className="mt-1 max-w-xs text-sm text-muted-foreground">{syncError}</p>
          )}
        </div>
        <button
          onClick={reinitialise}
          className="flex items-center gap-2 rounded-md border border-border bg-background px-4 py-2 text-sm transition-colors hover:bg-muted"
        >
          <RefreshCw className="h-4 w-4" />
          Try again
        </button>
      </div>
    );
  }

  return null;
}
