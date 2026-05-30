'use client';

import { useState, type ReactElement } from 'react';
import { useRouter } from 'next/navigation';
import { Plus, MessageCircleQuestion, Pencil, Trash2 } from 'lucide-react';
import { QnAModal } from '@/components/knowledge/modals/QnAModal';
import type { KnowledgeQnAEntry } from '@/components/knowledge/types';

interface QnAPaneClientProps {
  entries: KnowledgeQnAEntry[];
}

const STATUS_STYLES: Record<KnowledgeQnAEntry['status'], string> = {
  draft: 'bg-amber-50 text-amber-700 ring-amber-200',
  published: 'bg-emerald-50 text-emerald-700 ring-emerald-200',
  deprecated: 'bg-zinc-100 text-zinc-600 ring-zinc-200',
};

export function QnAPaneClient({ entries }: QnAPaneClientProps): ReactElement {
  const router = useRouter();
  const [open, setOpen] = useState<boolean>(false);
  const [editing, setEditing] = useState<KnowledgeQnAEntry | null>(null);

  const handleDelete = async (id: string): Promise<void> => {
    if (!window.confirm('Delete this Q&A pair?')) return;
    const response = await fetch(`/api/knowledge/qa/${id}`, { method: 'DELETE' });
    if (response.ok) router.refresh();
  };

  const handleEdit = (entry: KnowledgeQnAEntry): void => {
    setEditing(entry);
    setOpen(true);
  };

  const handleClose = (): void => {
    setOpen(false);
    setEditing(null);
  };

  return (
    <div className="px-6 py-6">
      <header className="mb-5 flex items-center justify-between gap-4">
        <div>
          <h1 className="flex items-center gap-2 text-[20px] font-semibold tracking-[-0.02em] text-[#111827]">
            <MessageCircleQuestion className="size-4 text-[#6B7280]" />
            Q&A pairs
          </h1>
          <p className="mt-0.5 text-[12px] text-[#6B7280]">
            Authoritative answers your agents must use verbatim when matched.
          </p>
        </div>
        <button
          type="button"
          onClick={() => {
            setEditing(null);
            setOpen(true);
          }}
          className="inline-flex items-center gap-1.5 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B]"
        >
          <Plus className="size-3.5" />
          New Q&A
        </button>
      </header>

      {entries.length === 0 ? (
        <div className="flex min-h-[200px] flex-col items-center justify-center rounded-xl border border-dashed border-[#E5E7EB] bg-[#FAFAFA] px-6 py-10 text-center">
          <p className="text-[14px] font-medium text-[#111827]">No Q&A pairs yet</p>
          <p className="mt-1 max-w-[40ch] text-[12px] leading-5 text-[#6B7280]">
            Create authoritative answers for the questions your customers ask most.
            Agents will quote them verbatim when matched.
          </p>
        </div>
      ) : (
        <ul className="space-y-2">
          {entries.map((entry) => (
            <li
              key={entry.id}
              className="rounded-xl border border-[#E5E7EB] bg-white p-4 transition hover:border-[#D1D5DB]"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="text-[13px] font-semibold text-[#111827]">
                      Q: {entry.question}
                    </h3>
                    <span
                      className={`inline-flex items-center rounded-full px-1.5 py-0 text-[9px] font-medium uppercase tracking-wide ring-1 ring-inset ${
                        STATUS_STYLES[entry.status]
                      }`}
                    >
                      {entry.status}
                    </span>
                    {entry.citationCount > 0 ? (
                      <span className="text-[10px] text-[#6B7280]">
                        Used in {entry.citationCount} answer{entry.citationCount === 1 ? '' : 's'}
                      </span>
                    ) : null}
                  </div>
                  <p className="mt-1.5 whitespace-pre-wrap text-[12px] leading-5 text-[#374151]">
                    <span className="font-medium text-[#6B7280]">A:</span> {entry.answer}
                  </p>
                </div>
                <div className="flex shrink-0 items-start gap-1">
                  <button
                    type="button"
                    onClick={() => handleEdit(entry)}
                    aria-label="Edit"
                    className="rounded-md p-1.5 text-[#6B7280] hover:bg-[#F3F4F6] hover:text-[#111827]"
                  >
                    <Pencil className="size-3.5" />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDelete(entry.id)}
                    aria-label="Delete"
                    className="rounded-md p-1.5 text-[#6B7280] hover:bg-red-50 hover:text-red-600"
                  >
                    <Trash2 className="size-3.5" />
                  </button>
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}

      <QnAModal
        open={open}
        onClose={handleClose}
        initial={editing ? { id: editing.id, question: editing.question, answer: editing.answer, status: editing.status === 'deprecated' ? 'draft' : editing.status } : undefined}
      />
    </div>
  );
}
