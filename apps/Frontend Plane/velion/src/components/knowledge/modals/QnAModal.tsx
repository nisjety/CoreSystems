'use client';

import { useState, type ReactElement, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { X, MessageCircleQuestion, Loader2, AlertCircle } from 'lucide-react';

interface QnAModalProps {
  open: boolean;
  onClose: () => void;
  /** Existing pair to edit (id implies update; absent implies create). */
  initial?: { id?: string; question?: string; answer?: string; status?: 'draft' | 'published' };
}

/**
 * Wave 11 §2.2 — operator-authored Q&A pair. POSTs to /api/knowledge/qa
 * which persists to Convex `knowledgeQnA` (see qa-store.ts). Q&A is a
 * first-class entity — retrieval picks it up via `wiki-store-go` once
 * D5 ships, until then the playground citation surface highlights Q&A
 * hits explicitly.
 */
export function QnAModal({ open, onClose, initial }: QnAModalProps): ReactElement | null {
  const router = useRouter();
  const [question, setQuestion] = useState<string>(initial?.question ?? '');
  const [answer, setAnswer] = useState<string>(initial?.answer ?? '');
  const [status, setStatus] = useState<'draft' | 'published'>(
    initial?.status === 'published' ? 'published' : 'draft',
  );
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const handleSubmit = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    setError(null);
    if (!question.trim() || !answer.trim()) {
      setError('Both question and answer are required.');
      return;
    }

    setSubmitting(true);
    try {
      const url = initial?.id ? `/api/knowledge/qa/${initial.id}` : '/api/knowledge/qa';
      const method = initial?.id ? 'PATCH' : 'POST';
      const response = await fetch(url, {
        method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          question: question.trim(),
          answer: answer.trim(),
          status,
        }),
      });
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }
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
              <MessageCircleQuestion className="size-4" strokeWidth={1.8} />
            </span>
            <div>
              <h2 className="text-[16px] font-semibold tracking-[-0.01em] text-[#111827]">
                {initial?.id ? 'Edit Q&A' : 'New Q&A pair'}
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6B7280]">
                Agents will quote this answer verbatim when matched.
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
              Question <span className="text-red-500">*</span>
            </span>
            <textarea
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              placeholder="How do I cancel my subscription?"
              rows={2}
              maxLength={500}
              required
              className="mt-1 block w-full rounded-md border border-[#E5E7EB] bg-white px-3 py-2 text-[13px] text-[#111827] outline-none focus:border-[#111111]"
            />
          </label>

          <label className="block">
            <span className="block text-[12px] font-medium text-[#374151]">
              Answer <span className="text-red-500">*</span>
            </span>
            <textarea
              value={answer}
              onChange={(e) => setAnswer(e.target.value)}
              placeholder="Go to Settings → Billing → Cancel subscription."
              rows={6}
              maxLength={8000}
              required
              className="mt-1 block w-full rounded-md border border-[#E5E7EB] bg-white px-3 py-2 text-[13px] text-[#111827] outline-none focus:border-[#111111]"
            />
          </label>

          <fieldset>
            <legend className="block text-[12px] font-medium text-[#374151]">Status</legend>
            <div className="mt-1 flex gap-2">
              {(['draft', 'published'] as const).map((value) => (
                <button
                  key={value}
                  type="button"
                  onClick={() => setStatus(value)}
                  className={`rounded-full border px-3 py-1 text-[12px] capitalize transition ${
                    status === value
                      ? 'border-[#111111] bg-[#111111] text-white'
                      : 'border-[#E5E7EB] bg-white text-[#374151] hover:border-[#9CA3AF]'
                  }`}
                >
                  {value}
                </button>
              ))}
            </div>
            <p className="mt-1 text-[11px] text-[#6B7280]">
              Drafts aren&apos;t served to agents until published.
            </p>
          </fieldset>
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
            disabled={submitting || !question.trim() || !answer.trim()}
            className="inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:cursor-not-allowed disabled:bg-[#9CA3AF]"
          >
            {submitting ? <Loader2 className="size-3.5 animate-spin" /> : null}
            {initial?.id ? 'Save changes' : 'Create Q&A'}
          </button>
        </div>
      </form>
    </div>
  );
}
