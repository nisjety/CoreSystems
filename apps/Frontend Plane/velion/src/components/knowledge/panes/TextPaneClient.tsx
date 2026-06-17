'use client';

import { useState, type ReactElement } from 'react';
import { useRouter } from 'next/navigation';
import { Plus, Type } from 'lucide-react';
import { DocumentList, type DocumentRow } from '@/components/knowledge/DocumentList';
import { PasteTextModal } from '@/components/knowledge/modals/PasteTextModal';
import type { KnowledgeDocumentSummary } from '@/lib/integrations/types';

interface TextPaneClientProps {
  documents: KnowledgeDocumentSummary[];
}

function toRow(doc: KnowledgeDocumentSummary): DocumentRow {
  const fresh = Date.now() - Date.parse(doc.createdAt) < 7 * 24 * 60 * 60 * 1000;
  return {
    id: doc.id,
    title: doc.title,
    status: (doc.status as DocumentRow['status']) ?? 'active',
    updatedAt: doc.updatedAt,
    isFresh: fresh,
  };
}

export function TextPaneClient({ documents }: TextPaneClientProps): ReactElement {
  const router = useRouter();
  const [open, setOpen] = useState<boolean>(false);

  const handleBulkDelete = async (ids: ReadonlyArray<string>): Promise<void> => {
    await Promise.all(
      ids.map((id) =>
        fetch(`/api/knowledge/documents/${id}`, { method: 'DELETE' }).catch(() => null),
      ),
    );
    router.refresh();
  };

  return (
    <div className="px-6 py-6">
      <header className="mb-5 flex items-center justify-between gap-4">
        <div>
          <h1 className="flex items-center gap-2 text-[20px] font-semibold tracking-[-0.02em] text-[#111827]">
            <Type className="size-4 text-[#6B7280]" />
            Text snippets
          </h1>
          <p className="mt-0.5 text-[12px] text-[#6B7280]">
            Short pieces of text agents should remember verbatim — brand rules,
            shortcuts, glossaries.
          </p>
        </div>
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="inline-flex items-center gap-1.5 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B]"
        >
          <Plus className="size-3.5" />
          Add snippet
        </button>
      </header>

      <DocumentList
        documents={documents.map(toRow)}
        searchPlaceholder="Search snippets…"
        emptyStateTitle="No text snippets yet"
        emptyStateBody="Paste a snippet to get started — agents will remember it verbatim."
        onOpen={(id) => router.push(`/knowledge/documents/${id}`)}
        onBulkDelete={(ids) => handleBulkDelete(ids as ReadonlyArray<string>)}
      />

      <PasteTextModal open={open} onClose={() => setOpen(false)} />
    </div>
  );
}
