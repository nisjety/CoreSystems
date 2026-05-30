'use client';

import { useState, useRef, type ReactElement, type ChangeEvent, type DragEvent } from 'react';
import { useRouter } from 'next/navigation';
import { X, UploadCloud, FileText, Loader2, AlertCircle } from 'lucide-react';

const ACCEPTED_TYPES = '.pdf,.docx,.doc,.md,.txt,.csv,.html,.htm';
const MAX_BYTES = 100 * 1024 * 1024; // 100 MB per file (Intercom Fin spec)
const MAX_FILES = 10;

interface UploadFilesModalProps {
  open: boolean;
  onClose: () => void;
}

interface FileItem {
  id: string;
  file: File;
  status: 'pending' | 'uploading' | 'done' | 'error';
  error?: string;
}

/**
 * Wave 11 §2.2 — files upload modal. Intercom Fin spec sidecar pattern
 * (Mobbin `cc67dd1f…dc7c`): supported types, size limit, count limit
 * spelled out in the modal so the operator never hits a surprise.
 *
 * Posts each file via multipart to /api/knowledge/documents which
 * forwards to documents-service `/v1/documents` with `type=file`.
 */
export function UploadFilesModal({ open, onClose }: UploadFilesModalProps): ReactElement | null {
  const router = useRouter();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [items, setItems] = useState<ReadonlyArray<FileItem>>([]);
  const [dragOver, setDragOver] = useState<boolean>(false);
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const addFiles = (files: FileList | null): void => {
    if (!files) return;
    setError(null);
    const incoming: FileItem[] = [];
    for (const file of Array.from(files)) {
      if (items.length + incoming.length >= MAX_FILES) {
        setError(`Max ${MAX_FILES} files per upload.`);
        break;
      }
      if (file.size > MAX_BYTES) {
        setError(`"${file.name}" exceeds 100 MB.`);
        continue;
      }
      incoming.push({
        id: `${file.name}-${file.lastModified}-${file.size}`,
        file,
        status: 'pending',
      });
    }
    setItems((prev) => [...prev, ...incoming]);
  };

  const removeItem = (id: string): void => {
    setItems((prev) => prev.filter((item) => item.id !== id));
  };

  const handleDrop = (event: DragEvent<HTMLDivElement>): void => {
    event.preventDefault();
    setDragOver(false);
    addFiles(event.dataTransfer.files);
  };

  const handleInputChange = (event: ChangeEvent<HTMLInputElement>): void => {
    addFiles(event.target.files);
    event.target.value = '';
  };

  const upload = async (): Promise<void> => {
    if (items.length === 0) return;
    setSubmitting(true);
    setError(null);

    let anySucceeded = false;
    for (const item of items) {
      if (item.status === 'done') continue;
      setItems((prev) =>
        prev.map((p) => (p.id === item.id ? { ...p, status: 'uploading' } : p)),
      );
      try {
        const form = new FormData();
        form.append('file', item.file);
        const response = await fetch('/api/knowledge/documents', {
          method: 'POST',
          body: form,
        });
        if (!response.ok) {
          const payload = (await response.json().catch(() => null)) as { error?: string } | null;
          throw new Error(payload?.error ?? `HTTP ${response.status}`);
        }
        anySucceeded = true;
        setItems((prev) =>
          prev.map((p) => (p.id === item.id ? { ...p, status: 'done' } : p)),
        );
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : 'Upload failed';
        setItems((prev) =>
          prev.map((p) =>
            p.id === item.id ? { ...p, status: 'error', error: message } : p,
          ),
        );
      }
    }

    setSubmitting(false);
    if (anySucceeded) {
      router.refresh();
      // Auto-close after a short delay so the operator sees their uploads turn green.
      window.setTimeout(() => {
        onClose();
      }, 800);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      onClick={(event) => {
        if (event.target === event.currentTarget && !submitting) onClose();
      }}
    >
      <div className="w-full max-w-[640px] rounded-2xl bg-white p-6 shadow-2xl">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-[16px] font-semibold tracking-[-0.01em] text-[#111827]">
              Upload files
            </h2>
            <p className="mt-0.5 text-[12px] text-[#6B7280]">
              We&apos;ll extract text from each file and index it for retrieval.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={submitting}
            aria-label="Close"
            className="rounded-md p-1 text-[#6B7280] hover:bg-[#F3F4F6] hover:text-[#111827] disabled:opacity-50"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="mt-5 grid gap-4 md:grid-cols-[1fr_220px]">
          <div>
            <div
              onDragOver={(e) => {
                e.preventDefault();
                setDragOver(true);
              }}
              onDragLeave={() => setDragOver(false)}
              onDrop={handleDrop}
              onClick={() => fileInputRef.current?.click()}
              className={`flex cursor-pointer flex-col items-center justify-center rounded-xl border-2 border-dashed px-6 py-8 text-center transition ${
                dragOver
                  ? 'border-[#111111] bg-[#F9FAFB]'
                  : 'border-[#E5E7EB] bg-white hover:border-[#9CA3AF]'
              }`}
            >
              <UploadCloud className="size-6 text-[#6B7280]" />
              <p className="mt-2 text-[13px] font-medium text-[#111827]">
                Click or drag files here
              </p>
              <p className="mt-1 text-[11px] text-[#6B7280]">
                Accepts PDF, DOCX, MD, TXT, CSV, HTML
              </p>
              <input
                ref={fileInputRef}
                type="file"
                multiple
                accept={ACCEPTED_TYPES}
                onChange={handleInputChange}
                className="hidden"
              />
            </div>

            {items.length > 0 ? (
              <ul className="mt-3 space-y-1.5">
                {items.map((item) => (
                  <li
                    key={item.id}
                    className="flex items-center gap-2 rounded-md border border-[#E5E7EB] bg-white px-2.5 py-1.5"
                  >
                    <FileText className="size-3.5 shrink-0 text-[#6B7280]" />
                    <span className="min-w-0 flex-1 truncate text-[11px] text-[#111827]">
                      {item.file.name}
                    </span>
                    <span className="shrink-0 font-mono text-[10px] text-[#9CA3AF]">
                      {(item.file.size / 1024).toFixed(0)} KB
                    </span>
                    {item.status === 'uploading' ? (
                      <Loader2 className="size-3.5 animate-spin text-[#6B7280]" />
                    ) : item.status === 'done' ? (
                      <span className="text-[10px] font-medium text-emerald-600">✓</span>
                    ) : item.status === 'error' ? (
                      <span
                        className="text-[10px] font-medium text-red-600"
                        title={item.error}
                      >
                        ✗
                      </span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => removeItem(item.id)}
                        className="text-[#9CA3AF] hover:text-[#111827]"
                        aria-label="Remove"
                      >
                        <X className="size-3" />
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            ) : null}
          </div>

          <aside className="rounded-xl bg-[#F9FAFB] p-3 text-[11px] leading-5 text-[#374151]">
            <h3 className="text-[11px] font-semibold uppercase tracking-wide text-[#6B7280]">
              File limits
            </h3>
            <ul className="mt-2 space-y-1">
              <li>· Up to <strong>{MAX_FILES}</strong> files per upload.</li>
              <li>· Up to <strong>100 MB</strong> per file.</li>
              <li>· PDF, DOCX, MD, TXT, CSV, HTML supported.</li>
              <li>· Scanned PDFs are extracted with OCR (may take longer).</li>
            </ul>
          </aside>
        </div>

        {error ? (
          <div
            role="alert"
            className="mt-3 flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700"
          >
            <AlertCircle className="size-3.5" />
            {error}
          </div>
        ) : null}

        <div className="mt-5 flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={submitting}
            className="rounded-full border border-[#E5E7EB] bg-white px-4 py-2 text-[12px] font-medium text-[#374151] hover:border-[#9CA3AF] disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={upload}
            disabled={submitting || items.length === 0}
            className="inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:cursor-not-allowed disabled:bg-[#9CA3AF]"
          >
            {submitting ? <Loader2 className="size-3.5 animate-spin" /> : <UploadCloud className="size-3.5" />}
            Upload {items.length > 0 ? `(${items.length})` : ''}
          </button>
        </div>
      </div>
    </div>
  );
}
