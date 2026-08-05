"use client";

import Link from "next/link";
import type { Route } from "next";
import { useRouter } from "next/navigation";
import { useEffect, useReducer, useRef, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { Search } from "lucide-react";
import {
  VerevonButton,
  VerevonInput,
  VerevonModal,
  VerevonModalClose,
  VerevonModalTitle,
  VerevonTextarea,
} from "@/components/ui/verevon-ui";
import {
  searchNavbar,
  submitNavbarSupportRequest,
  type SearchResult,
} from "@/features/shell-v2/lib/navbar-data";

export function GlobalSearchDialog({ onClose }: { onClose: () => void }) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [searchState, dispatchSearch] = useReducer(searchDialogReducer, initialSearchDialogState);
  const { error, loading, query, results } = searchState;

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.length < 2) {
      return;
    }

    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      dispatchSearch({ type: "started" });
      searchNavbar(trimmed, controller.signal)
        .then((payload) => {
          dispatchSearch({ type: "succeeded", results: payload.results });
        })
        .catch((searchError: unknown) => {
          if (!controller.signal.aborted) {
            dispatchSearch({
              type: "failed",
              message: searchError instanceof Error ? searchError.message : "Search failed.",
            });
          }
        });
    }, 220);

    return () => {
      controller.abort();
      window.clearTimeout(timer);
    };
  }, [query]);

  return (
    <VerevonModal label="Knowledge search" size="search">
        <div className="verevon-modal-header">
          <Search className="size-4 text-[#989286]" />
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => {
              dispatchSearch({ type: "queryChanged", query: event.target.value });
            }}
            placeholder="Search the knowledge base"
            aria-label="Search the knowledge base"
            className="min-w-0 flex-1 bg-transparent text-[15px] text-[#111111] placeholder:text-[#9A9387] focus:outline-none dark:text-white"
          />
          <VerevonModalClose onClick={onClose} aria-label="Close global search">
            Esc
          </VerevonModalClose>
        </div>
        <div className="max-h-[420px] overflow-y-auto p-2">
          {loading ? <EmptyPanel text="Searching…" /> : null}
          {error ? <EmptyPanel text={error} /> : null}
          {!loading && !error && query.length >= 2 && results.length === 0 ? <EmptyPanel text="No matching knowledge records." /> : null}
          {results.map((result) => (
            <Link key={result.id} href={result.href as Route} onClick={onClose} className="block rounded-[14px] p-3 transition-colors hover:bg-[#F7F7F8] dark:hover:bg-[#191A1F]">
              <div className="flex items-center justify-between gap-4">
                <p className="text-sm font-semibold text-[#111111] dark:text-white">{result.label}</p>
                <span className="rounded-full bg-[#F2F2F2] px-2 py-0.5 text-[10px] text-[#777] dark:bg-[#23252A] dark:text-[#AEB4C0]">{result.source}</span>
              </div>
              <p className="mt-1 line-clamp-2 text-xs leading-5 text-[#777] dark:text-[#AEB4C0]">{result.excerpt}</p>
            </Link>
          ))}
        </div>
    </VerevonModal>
  );
}

type SearchDialogState = {
  error: string | null;
  loading: boolean;
  query: string;
  results: SearchResult[];
};

type SearchDialogAction =
  | { type: "queryChanged"; query: string }
  | { type: "started" }
  | { type: "succeeded"; results: SearchResult[] }
  | { type: "failed"; message: string };

const initialSearchDialogState: SearchDialogState = {
  error: null,
  loading: false,
  query: "",
  results: [],
};

function searchDialogReducer(state: SearchDialogState, action: SearchDialogAction): SearchDialogState {
  switch (action.type) {
    case "queryChanged": {
      if (action.query.trim().length < 2) {
        return {
          ...state,
          error: null,
          loading: false,
          query: action.query,
          results: [],
        };
      }

      return {
        ...state,
        query: action.query,
      };
    }
    case "started":
      return {
        ...state,
        loading: true,
      };
    case "succeeded":
      return {
        ...state,
        error: null,
        loading: false,
        results: action.results,
      };
    case "failed":
      return {
        ...state,
        error: action.message,
        loading: false,
        results: [],
      };
  }
}

export function AssistantModal({ pathname, onClose }: { pathname: string; onClose: () => void }) {
  const [prompt, setPrompt] = useState("");
  const { push } = useRouter();

  return (
    <DialogPanel title="AI Assistant" onClose={onClose}>
      <p className="verevon-type-body text-[#666] dark:text-[#AEB4C0]">Help with {pathname.split("/").filter(Boolean).pop() ?? "this page"}.</p>
      <VerevonTextarea
        value={prompt}
        onChange={(event) => setPrompt(event.target.value)}
        rows={4}
        placeholder="Ask Verevon…"
        aria-label="Ask Verevon assistant"
        className="mt-4"
      />
      <VerevonButton
        onClick={() => {
          push(`/chat?prompt=${encodeURIComponent(prompt)}`);
          onClose();
        }}
        disabled={!prompt.trim()}
        variant="primary"
        className="mt-3 px-4 disabled:opacity-50"
      >
        Open in chat
      </VerevonButton>
    </DialogPanel>
  );
}

export function SupportModal({ pathname, onClose }: { pathname: string; onClose: () => void }) {
  const [subject, setSubject] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const submit = () => {
    setSaving(true);
    setError(null);
    submitNavbarSupportRequest({ subject, message, context: pathname })
      .then(() => {
        toast.success("Support request saved to user-core.");
        onClose();
      })
      .catch((submitError: unknown) => {
        setError(submitError instanceof Error ? submitError.message : "Support request could not be saved.");
      })
      .then(() => setSaving(false));
  };

  return (
    <DialogPanel title="Help center" onClose={onClose}>
      <VerevonInput
        value={subject}
        onChange={(event) => setSubject(event.target.value)}
        placeholder="Subject"
        aria-label="Support request subject"
      />
      <VerevonTextarea
        value={message}
        onChange={(event) => setMessage(event.target.value)}
        rows={5}
        placeholder="What do you need help with?"
        aria-label="Support request message"
        className="mt-3"
      />
      {error ? <p className="mt-2 text-xs text-[#B42318]">{error}</p> : null}
      <VerevonButton
        onClick={submit}
        disabled={saving || !subject.trim() || !message.trim()}
        variant="primary"
        className="mt-3 px-4 disabled:opacity-50"
      >
        {saving ? "Saving…" : "Save request"}
      </VerevonButton>
    </DialogPanel>
  );
}

function DialogPanel({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <VerevonModal compact label={title} className="p-4">
        <div className="mb-4 flex items-center justify-between">
          <VerevonModalTitle>{title}</VerevonModalTitle>
          <VerevonModalClose onClick={onClose} aria-label={`Close ${title}`}>
            Esc
          </VerevonModalClose>
        </div>
        {children}
    </VerevonModal>
  );
}

function EmptyPanel({ text }: { text: string }) {
  return <div className="px-4 py-8 text-center text-sm text-[#999] dark:text-[#AEB4C0]">{text}</div>;
}
