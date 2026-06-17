'use client';

import { useState, type ReactElement } from 'react';
import { useRouter } from 'next/navigation';
import { Plus, FileText } from 'lucide-react';
import { DocumentList, type DocumentRow } from '@/components/knowledge/DocumentList';
import { UploadFilesModal } from '@/components/knowledge/modals/UploadFilesModal';
import type { KnowledgeDocumentSummary } from '@/lib/integrations/types';

interface FilesPaneClientProps {
  documents: KnowledgeDocumentSummary[];
}

function isFresh(iso: string): boolean {
  return Date.now() - Date.parse(iso) < 7 * 24 * 60 * 60 * 1000;
}

function toRow(doc: KnowledgeDocumentSummary): DocumentRow {
  return {
    id: doc.id,
    title: doc.title,
    subtitle: doc.source,
    status: (doc.status as DocumentRow['status']) ?? 'active',
    updatedAt: doc.updatedAt,
    isFresh: isFresh(doc.createdAt),
  };
}

export function FilesPaneClient({ documents }: FilesPaneClientProps): ReactElement {
  const router = useRouter();
  const [uploadOpen, setUploadOpen] = useState<boolean>(false);

  const rows = documents.map(toRow);

  const handleBulkDelete = async (ids: ReadonlyArray<string>): Promise<void> => {
    await Promise.all(
      ids.map((id) =>
        fetch(`/api/knowledge/documents/${id}`, { method: 'DELETE' }).catch(() => null),
      ),
    );
    router.refresh();
  };

  const handleBulkReindex = async (ids: ReadonlyArray<string>): Promise<void> => {
    await Promise.all(
      ids.map((id) =>
        fetch(`/api/knowledge/documents/${id}/reindex`, { method: 'POST' }).catch(() => null),
      ),
    );
    router.refresh();
  };

  return (
    <div className="px-6 py-6">
      <header className="mb-5 flex items-center justify-between gap-4">
        <div>
          <h1 className="flex items-center gap-2 text-[20px] font-semibold tracking-[-0.02em] text-[#111827]">
            <FileText className="size-4 text-[#6B7280]" />
            Files
          </h1>
          <p className="mt-0.5 text-[12px] text-[#6B7280]">
            Upload PDFs, Word docs, Markdown, plain text, or CSV. We extract and
            index the contents automatically.
          </p>
        </div>
        <button
          type="button"
          onClick={() => setUploadOpen(true)}
          className="inline-flex items-center gap-1.5 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B]"
        >
          <Plus className="size-3.5" />
          Upload files
        </button>
      </header>

      <DocumentList
        documents={rows}
        searchPlaceholder="Search files…"
        emptyStateTitle="No files yet"
        emptyStateBody="Upload a PDF, Word doc, or other supported file to get started."
        onOpen={(id) => router.push(`/knowledge/documents/${id}`)}
        onBulkDelete={(ids) => handleBulkDelete(ids as ReadonlyArray<string>)}
        onBulkReindex={(ids) => handleBulkReindex(ids as ReadonlyArray<string>)}
      />

      <UploadFilesModal open={uploadOpen} onClose={() => setUploadOpen(false)} />
    </div>
  );
}
