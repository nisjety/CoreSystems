'use client';

import { useState, useEffect, useRef, type ReactElement } from 'react';
import { X, Save, Trash2, FileText, Users, Loader2, AlertCircle, CheckCircle2 } from 'lucide-react';

interface DocumentDetail {
  id: string;
  title: string;
  source: string;
  type: string;
  status: string;
  content?: string;
  /** Agents bound to this doc — populated by the GET /api/knowledge/documents/[id] response. */
  agents?: ReadonlyArray<{ id: string; name: string }>;
  createdAt: string;
  updatedAt: string;
}

interface DocumentDrawerProps {
  documentId: string | null;
  onClose: () => void;
  onDeleted?: () => void;
}

/**
 * Wave 11 §5 — ElevenLabs-style document drawer.
 *
 *   Content tab : extracted-text preview + inline edit + deprecate.
 *   Agents tab  : agents bound to this doc with quick-unbind action.
 *
 * Persisted via PATCH /api/knowledge/documents/[id] → documents-service.
 * Saves trigger re-embedding asynchronously via the embedding-worker.
 */
export function DocumentDrawer({
  documentId,
  onClose,
  onDeleted,
}: DocumentDrawerProps): ReactElement | null {
  const [tab, setTab] = useState<'content' | 'agents'>('content');
  const [doc, setDoc] = useState<DocumentDetail | null>(null);
  const [editedContent, setEditedContent] = useState<string>('');
  const [editedTitle, setEditedTitle] = useState<string>('');
  const [saving, setSaving] = useState<boolean>(false);
  const [saveStatus, setSaveStatus] = useState<'idle' | 'saved' | 'error'>('idle');
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const saveTimerRef = useRef<number | null>(null);

  useEffect(() => {
    if (!documentId) {
      setDoc(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    void fetch(`/api/knowledge/documents/${documentId}`)
      .then(async (response) => {
        if (!response.ok) {
          const payload = (await response.json().catch(() => null)) as { error?: string } | null;
          throw new Error(payload?.error ?? `HTTP ${response.status}`);
        }
        return (await response.json()) as DocumentDetail;
      })
      .then((data) => {
        if (cancelled) return;
        setDoc(data);
        setEditedTitle(data.title);
        setEditedContent(data.content ?? '');
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : 'Load failed');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [documentId]);

  // Debounced auto-save: 1.5s after last keystroke commits the patch.
  useEffect(() => {
    if (!doc) return;
    if (editedTitle === doc.title && editedContent === (doc.content ?? '')) return;

    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
    }
    saveTimerRef.current = window.setTimeout(() => {
      void save();
    }, 1500);

    return () => {
      if (saveTimerRef.current !== null) {
        window.clearTimeout(saveTimerRef.current);
        saveTimerRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editedTitle, editedContent]);

  const save = async (): Promise<void> => {
    if (!doc) return;
    setSaving(true);
    setSaveStatus('idle');
    try {
      const response = await fetch(`/api/knowledge/documents/${doc.id}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ title: editedTitle, content: editedContent }),
      });
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }
      const updated = (await response.json()) as DocumentDetail;
      setDoc(updated);
      setSaveStatus('saved');
      window.setTimeout(() => setSaveStatus('idle'), 2000);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Save failed');
      setSaveStatus('error');
    } finally {
      setSaving(false);
    }
  };

  const handleDeprecate = async (): Promise<void> => {
    if (!doc) return;
    const response = await fetch(`/api/knowledge/documents/${doc.id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ status: 'deprecated' }),
    });
    if (response.ok) {
      setDoc((prev) => (prev ? { ...prev, status: 'deprecated' } : prev));
    }
  };

  const handleDelete = async (): Promise<void> => {
    if (!doc) return;
    if (!window.confirm('Delete this document? This cannot be undone.')) return;
    const response = await fetch(`/api/knowledge/documents/${doc.id}`, {
      method: 'DELETE',
    });
    if (response.ok) {
      onDeleted?.();
      onClose();
    }
  };

  if (!documentId) return null;

  return (
    <div
      className="fixed inset-0 z-40 flex justify-end bg-black/40"
      role="dialog"
      aria-modal="true"
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="flex h-full w-full max-w-[640px] flex-col bg-white shadow-2xl">
        <header className="flex shrink-0 items-start justify-between gap-3 border-b border-[#E5E7EB] px-5 py-4">
          <div className="min-w-0 flex-1">
            <input
              type="text"
              value={editedTitle}
              onChange={(e) => setEditedTitle(e.target.value)}
              placeholder="Untitled"
              className="block w-full bg-transparent text-[16px] font-semibold text-[#111827] outline-none focus:bg-[#F9FAFB] focus:px-2 focus:py-1 focus:-mx-2 focus:rounded"
            />
            <div className="mt-0.5 flex items-center gap-2 text-[11px] text-[#6B7280]">
              <span>{doc?.source}</span>
              {doc?.status ? (
                <>
                  <span>·</span>
                  <span className="capitalize">{doc.status}</span>
                </>
              ) : null}
            </div>
          </div>
          <div className="flex items-center gap-1">
            {saveStatus === 'saved' ? (
              <span className="inline-flex items-center gap-1 text-[11px] text-emerald-600">
                <CheckCircle2 className="size-3" />
                Saved
              </span>
            ) : saving ? (
              <span className="inline-flex items-center gap-1 text-[11px] text-[#6B7280]">
                <Loader2 className="size-3 animate-spin" />
                Saving…
              </span>
            ) : null}
            <button
              type="button"
              onClick={onClose}
              aria-label="Close"
              className="rounded-md p-1.5 text-[#6B7280] hover:bg-[#F3F4F6] hover:text-[#111827]"
            >
              <X className="size-4" />
            </button>
          </div>
        </header>

        <nav className="flex shrink-0 gap-1 border-b border-[#E5E7EB] px-5">
          <button
            type="button"
            onClick={() => setTab('content')}
            className={`flex items-center gap-1.5 border-b-2 px-2 py-2 text-[12px] font-medium ${
              tab === 'content'
                ? 'border-[#111827] text-[#111827]'
                : 'border-transparent text-[#6B7280] hover:text-[#111827]'
            }`}
          >
            <FileText className="size-3.5" />
            Content
          </button>
          <button
            type="button"
            onClick={() => setTab('agents')}
            className={`flex items-center gap-1.5 border-b-2 px-2 py-2 text-[12px] font-medium ${
              tab === 'agents'
                ? 'border-[#111827] text-[#111827]'
                : 'border-transparent text-[#6B7280] hover:text-[#111827]'
            }`}
          >
            <Users className="size-3.5" />
            Agents{' '}
            {doc?.agents ? (
              <span className="rounded-full bg-[#F3F4F6] px-1.5 py-0 text-[10px] text-[#6B7280]">
                {doc.agents.length}
              </span>
            ) : null}
          </button>
        </nav>

        <div className="flex-1 overflow-y-auto px-5 py-4">
          {loading ? (
            <div className="flex h-40 items-center justify-center text-[#9CA3AF]">
              <Loader2 className="size-5 animate-spin" />
            </div>
          ) : error ? (
            <div
              role="alert"
              className="flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700"
            >
              <AlertCircle className="size-3.5" />
              {error}
            </div>
          ) : tab === 'content' ? (
            <div>
              <h3 className="text-[11px] font-semibold uppercase tracking-wide text-[#6B7280]">
                Extracted content
              </h3>
              <p className="mt-1 text-[11px] text-[#9CA3AF]">
                The following text will be passed to the LLM for retrieval. Edit it
                to correct mistakes or tighten the wording — embeddings refresh
                automatically.
              </p>
              <textarea
                value={editedContent}
                onChange={(e) => setEditedContent(e.target.value)}
                rows={20}
                placeholder="No extracted content available."
                className="mt-3 block w-full rounded-md border border-[#E5E7EB] bg-white px-3 py-2 font-mono text-[12px] leading-5 text-[#111827] outline-none focus:border-[#111111]"
              />
            </div>
          ) : (
            <div>
              <h3 className="text-[11px] font-semibold uppercase tracking-wide text-[#6B7280]">
                Used by
              </h3>
              {doc?.agents && doc.agents.length > 0 ? (
                <ul className="mt-3 space-y-1">
                  {doc.agents.map((agent) => (
                    <li
                      key={agent.id}
                      className="flex items-center justify-between gap-2 rounded-md border border-[#E5E7EB] bg-white px-3 py-2"
                    >
                      <span className="text-[12px] font-medium text-[#111827]">
                        {agent.name}
                      </span>
                      <a
                        href={`/agents/${agent.id}/playground`}
                        className="text-[11px] text-[#6B7280] hover:text-[#111827]"
                      >
                        Open →
                      </a>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="mt-3 text-[12px] text-[#6B7280]">
                  No agents are bound to this document yet. Bind one from an
                  agent&apos;s Knowledge tab.
                </p>
              )}
            </div>
          )}
        </div>

        <footer className="flex shrink-0 items-center justify-between gap-2 border-t border-[#E5E7EB] px-5 py-3">
          <button
            type="button"
            onClick={handleDeprecate}
            disabled={!doc || doc.status === 'deprecated'}
            className="text-[12px] text-[#6B7280] hover:text-[#111827] disabled:opacity-50"
          >
            Deprecate
          </button>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleDelete}
              className="inline-flex items-center gap-1 rounded-full border border-red-200 bg-white px-3 py-1.5 text-[12px] font-medium text-red-700 hover:bg-red-50"
            >
              <Trash2 className="size-3" />
              Delete
            </button>
            <button
              type="button"
              onClick={save}
              disabled={saving}
              className="inline-flex items-center gap-1 rounded-full bg-[#111111] px-3 py-1.5 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:opacity-50"
            >
              <Save className="size-3" />
              Save
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}
