'use client';

import { useState, type ReactElement, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { X, Type, Loader2, AlertCircle } from 'lucide-react';

interface PasteTextModalProps {
  open: boolean;
  onClose: () => void;
}

/**
 * Wave 11 §2.2 — paste a text snippet directly. POSTs to
 * /api/knowledge/documents with `type=text`; documents-service handles
 * chunking and embedding asynchronously, no separate flow needed.
 */
export function PasteTextModal({ open, onClose }: PasteTextModalProps): ReactElement | null {
  const router = useRouter();
  const [title, setTitle] = useState<string>('');
  const [body, setBody] = useState<string>('');
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const handleSubmit = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    setError(null);
    if (!title.trim() || !body.trim()) {
      setError('Both title and body are required.');
      return;
    }

    setSubmitting(true);
    try {
      const response = await fetch('/api/knowledge/documents', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          type: 'text',
          title: title.trim(),
          content: body.trim(),
        }),
      });
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }
      setTitle('');
      setBody('');
      router.refresh();
      onClose();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Save failed');
    } finally {
      setSubmitting(false);
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
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-[560px] rounded-2xl bg-white p-6 shadow-2xl"
      >
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-center gap-2">
            <span className="inline-flex size-8 items-center justify-center rounded-md bg-[#F3F4F6] text-[#111827]">
              <Type className="size-4" strokeWidth={1.8} />
            </span>
            <div>
              <h2 className="text-[16px] font-semibold tracking-[-0.01em] text-[#111827]">
                Add text snippet
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6B7280]">
                Paste anything an agent should remember.
              </p>
            </div>
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

        <div className="mt-5 space-y-3">
          <label className="block">
            <span className="block text-[12px] font-medium text-[#374151]">
              Title <span className="text-red-500">*</span>
            </span>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Refund policy summary"
              maxLength={200}
              required
              className="mt-1 block w-full rounded-md border border-[#E5E7EB] bg-white px-3 py-2 text-[13px] text-[#111827] outline-none focus:border-[#111111]"
            />
          </label>

          <label className="block">
            <span className="block text-[12px] font-medium text-[#374151]">
              Content <span className="text-red-500">*</span>
            </span>
            <textarea
              value={body}
              onChange={(e) => setBody(e.target.value)}
              placeholder="Paste the text here…"
              rows={10}
              maxLength={32_000}
              required
              className="mt-1 block w-full rounded-md border border-[#E5E7EB] bg-white px-3 py-2 font-mono text-[12px] leading-5 text-[#111827] outline-none focus:border-[#111111]"
            />
            <span className="mt-1 block text-right text-[10px] text-[#9CA3AF]">
              {body.length.toLocaleString()} / 32,000 characters
            </span>
          </label>
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
            type="submit"
            disabled={submitting || !title.trim() || !body.trim()}
            className="inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:cursor-not-allowed disabled:bg-[#9CA3AF]"
          >
            {submitting ? <Loader2 className="size-3.5 animate-spin" /> : null}
            Save snippet
          </button>
        </div>
      </form>
    </div>
  );
}
