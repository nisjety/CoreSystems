'use client';

import { useState, useEffect, useMemo, type ReactElement } from 'react';
import { Network, RotateCw, AlertCircle, X, Loader2 } from 'lucide-react';
import { GraphCanvas } from './GraphCanvas';
import type {
  GraphClaim,
  GraphEntity,
  GraphRelationship,
  GraphViewerSnapshot,
} from '@/types/data-plane/graph_v1';

interface GraphPaneClientProps {
  snapshot: GraphViewerSnapshot;
}

interface EntityDetail {
  entity: GraphEntity | null;
  relationships: GraphRelationship[];
  claims: GraphClaim[];
  warning?: string;
}

const TYPE_BADGE_STYLES: Record<string, string> = {
  person: 'bg-violet-50 text-violet-700 ring-violet-200',
  organization: 'bg-red-50 text-red-700 ring-red-200',
  product: 'bg-sky-50 text-sky-700 ring-sky-200',
  location: 'bg-emerald-50 text-emerald-700 ring-emerald-200',
  concept: 'bg-amber-50 text-amber-700 ring-amber-200',
};

function typeBadge(type: string): string {
  return TYPE_BADGE_STYLES[type.toLowerCase()] ?? 'bg-zinc-100 text-zinc-700 ring-zinc-200';
}

export function GraphPaneClient({ snapshot }: GraphPaneClientProps): ReactElement {
  const [visibleTypes, setVisibleTypes] = useState<ReadonlySet<string>>(new Set());
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<EntityDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState<boolean>(false);
  const [detailError, setDetailError] = useState<string | null>(null);

  const orphanCount = useMemo(() => {
    const linked = new Set<string>();
    for (const rel of snapshot.relationships) {
      linked.add(rel.entity_a_id);
      linked.add(rel.entity_b_id);
    }
    return snapshot.entities.filter((e) => !linked.has(e.entity_id)).length;
  }, [snapshot]);

  // Fetch detail when an entity is selected.
  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    setDetailLoading(true);
    setDetailError(null);
    void fetch(`/api/knowledge/graph/entities/${encodeURIComponent(selectedId)}`)
      .then(async (response) => {
        if (!response.ok) {
          const payload = (await response.json().catch(() => null)) as { error?: string } | null;
          throw new Error(payload?.error ?? `HTTP ${response.status}`);
        }
        return (await response.json()) as EntityDetail;
      })
      .then((data) => {
        if (cancelled) return;
        setDetail(data);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setDetailError(err instanceof Error ? err.message : 'Load failed');
      })
      .finally(() => {
        if (!cancelled) setDetailLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  const toggleType = (type: string): void => {
    setVisibleTypes((prev) => {
      const next = new Set(prev);
      if (next.has(type)) next.delete(type);
      else next.add(type);
      return next;
    });
  };

  const clearFilters = (): void => setVisibleTypes(new Set());

  return (
    <div className="flex h-full min-h-0 flex-col px-6 py-6">
      <header className="mb-3 flex items-start justify-between gap-4">
        <div>
          <h1 className="flex items-center gap-2 text-[20px] font-semibold tracking-[-0.02em] text-[#111827]">
            <Network className="size-4 text-[#6B7280]" />
            Knowledge graph
          </h1>
          <p className="mt-0.5 max-w-[64ch] text-[12px] text-[#6B7280]">
            Entities and relationships extracted from your documents. Click a
            node to see what the agents know about it.
          </p>
        </div>
        <button
          type="button"
          onClick={() => window.location.reload()}
          aria-label="Refresh"
          title="Refresh"
          className="inline-flex items-center gap-1 rounded-full border border-[#E5E7EB] bg-white px-3 py-1.5 text-[12px] font-medium text-[#374151] hover:border-[#111111] hover:text-[#111827]"
        >
          <RotateCw className="size-3" />
          Refresh
        </button>
      </header>

      {snapshot.warning ? (
        <div
          role="alert"
          className="mb-3 flex items-center gap-2 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-[12px] text-amber-800"
        >
          <AlertCircle className="size-3.5" />
          {snapshot.warning}
        </div>
      ) : null}

      <div className="mb-3 flex flex-wrap items-center gap-1.5">
        <span className="text-[11px] font-medium uppercase tracking-wide text-[#6B7280]">
          Types:
        </span>
        {snapshot.available_types.length === 0 ? (
          <span className="text-[11px] text-[#9CA3AF]">none yet</span>
        ) : (
          snapshot.available_types.map((type) => {
            const active = visibleTypes.has(type);
            return (
              <button
                key={type}
                type="button"
                onClick={() => toggleType(type)}
                className={`rounded-full px-2.5 py-0.5 text-[11px] font-medium capitalize ring-1 ring-inset transition ${
                  active
                    ? `${typeBadge(type)} ring-2`
                    : 'bg-white text-[#374151] ring-[#E5E7EB] hover:border-[#9CA3AF]'
                }`}
              >
                {type}
              </button>
            );
          })
        )}
        {visibleTypes.size > 0 ? (
          <button
            type="button"
            onClick={clearFilters}
            className="ml-2 text-[11px] text-[#6B7280] hover:text-[#111827]"
          >
            Clear
          </button>
        ) : null}
        <div className="ml-auto flex items-center gap-3 text-[11px] text-[#6B7280]">
          <span>
            <span className="font-mono text-[#111827]">{snapshot.entities.length}</span> entities
          </span>
          <span>
            <span className="font-mono text-[#111827]">{snapshot.relationships.length}</span>{' '}
            relationships
          </span>
          {orphanCount > 0 ? (
            <span title="Entities with no edges — signal of poor extraction or ingest gaps">
              <span className="font-mono text-amber-700">{orphanCount}</span> orphans
            </span>
          ) : null}
        </div>
      </div>

      <div className="relative min-h-[480px] flex-1 overflow-hidden rounded-xl border border-[#E5E7EB] bg-white">
        <GraphCanvas
          snapshot={snapshot}
          selectedId={selectedId}
          onSelect={setSelectedId}
          visibleTypes={visibleTypes}
        />

        {selectedId ? (
          <div className="absolute right-3 top-3 z-10 w-[320px] max-w-[40vw] rounded-xl border border-[#E5E7EB] bg-white shadow-xl">
            <header className="flex items-start justify-between gap-2 border-b border-[#F3F4F6] px-4 py-3">
              <div className="min-w-0 flex-1">
                {detailLoading ? (
                  <div className="flex items-center gap-2 text-[12px] text-[#6B7280]">
                    <Loader2 className="size-3 animate-spin" />
                    Loading…
                  </div>
                ) : detail?.entity ? (
                  <>
                    <div className="flex items-center gap-2">
                      <h3 className="truncate text-[13px] font-semibold text-[#111827]">
                        {detail.entity.text}
                      </h3>
                      <span
                        className={`shrink-0 rounded-full px-1.5 py-0 text-[9px] font-medium uppercase tracking-wide ring-1 ring-inset ${typeBadge(detail.entity.type)}`}
                      >
                        {detail.entity.type}
                      </span>
                    </div>
                    <div className="mt-0.5 text-[11px] text-[#9CA3AF]">
                      Confidence {(detail.entity.confidence ?? 1).toFixed(2)} ·{' '}
                      {detail.entity.provenance}
                    </div>
                  </>
                ) : (
                  <p className="text-[12px] text-[#6B7280]">Entity not found</p>
                )}
              </div>
              <button
                type="button"
                onClick={() => setSelectedId(null)}
                aria-label="Close"
                className="rounded-md p-1 text-[#6B7280] hover:bg-[#F3F4F6] hover:text-[#111827]"
              >
                <X className="size-3.5" />
              </button>
            </header>

            <div className="max-h-[420px] overflow-y-auto px-4 py-3 text-[12px]">
              {detailError ? (
                <div className="rounded-md border border-red-200 bg-red-50 px-2 py-1.5 text-[11px] text-red-700">
                  {detailError}
                </div>
              ) : null}

              {detail?.warning ? (
                <div className="mb-2 rounded-md border border-amber-200 bg-amber-50 px-2 py-1.5 text-[11px] text-amber-800">
                  {detail.warning}
                </div>
              ) : null}

              {detail?.relationships && detail.relationships.length > 0 ? (
                <section className="mb-3">
                  <h4 className="text-[10px] font-semibold uppercase tracking-wide text-[#6B7280]">
                    Relationships ({detail.relationships.length})
                  </h4>
                  <ul className="mt-1 space-y-1">
                    {detail.relationships.slice(0, 20).map((rel) => {
                      const other =
                        rel.entity_a_id === selectedId ? rel.entity_b_id : rel.entity_a_id;
                      return (
                        <li
                          key={rel.rel_id}
                          className="flex items-center gap-2 rounded-md bg-[#F9FAFB] px-2 py-1"
                        >
                          <span className="rounded bg-white px-1 py-0 text-[9px] font-medium text-[#6B7280] ring-1 ring-inset ring-[#E5E7EB]">
                            {rel.relation_type}
                          </span>
                          <button
                            type="button"
                            onClick={() => setSelectedId(other)}
                            className="min-w-0 flex-1 truncate text-left text-[11px] text-[#374151] hover:text-[#111827]"
                          >
                            → {other}
                          </button>
                          <span className="font-mono text-[9px] text-[#9CA3AF]">
                            {rel.confidence.toFixed(2)}
                          </span>
                        </li>
                      );
                    })}
                  </ul>
                </section>
              ) : null}

              {detail?.claims && detail.claims.length > 0 ? (
                <section>
                  <h4 className="text-[10px] font-semibold uppercase tracking-wide text-[#6B7280]">
                    Claims ({detail.claims.length})
                  </h4>
                  <ul className="mt-1 space-y-1.5">
                    {detail.claims.slice(0, 10).map((claim) => (
                      <li
                        key={claim.claim_id}
                        className="rounded-md border border-[#E5E7EB] bg-white px-2 py-1.5"
                      >
                        <p className="text-[11px] leading-5 text-[#374151]">{claim.text}</p>
                        <div className="mt-0.5 flex items-center gap-2 text-[9px] text-[#9CA3AF]">
                          <span className="capitalize">{claim.status}</span>
                          <span>·</span>
                          <span className="font-mono">conf {claim.confidence.toFixed(2)}</span>
                          {claim.contradicted_by_claim_ids.length > 0 ? (
                            <>
                              <span>·</span>
                              <span className="text-amber-700">
                                {claim.contradicted_by_claim_ids.length} contradiction(s)
                              </span>
                            </>
                          ) : null}
                        </div>
                      </li>
                    ))}
                  </ul>
                </section>
              ) : null}

              {detail &&
              detail.entity &&
              (detail.relationships?.length ?? 0) === 0 &&
              (detail.claims?.length ?? 0) === 0 ? (
                <p className="text-[11px] text-[#6B7280]">
                  No relationships or claims yet for this entity.
                </p>
              ) : null}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
