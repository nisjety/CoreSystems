"use client";

/**
 * Step 3 — website seed (live crawl). The user pastes a URL; on submit the
 * step opens the crawl-preview SSE (BFF composition over quarry-edge), renders
 * each snippet as a falling card, persists server-normalized branding signals
 * to wizard state (the frame shows the brand pill), and advances to Connect.
 *
 * Degrades open: a 12s safety timer + a min-snippet grace advance guarantee the
 * user is never stuck on a flaky crawl.
 */

import { useEffect, useMemo, useRef, useState } from "react";

import { cn } from "@/lib/utils";
import {
  streamCrawlPreview,
  type CrawlSnippet,
  type CrawlSnippetKind,
} from "../../lib/onboarding-api";
import { formatOnboardingText, useOnboardingCopy } from "../../lib/onboarding-i18n";
import type { BrandingSignals, OnboardingMachine } from "../../lib/onboarding-machine";
import {
  LeftPane,
  PrimaryButton,
  RightPane,
  SkipLink,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from "../onboarding-shared";

interface FallingCard extends CrawlSnippet {
  xOffsetPct: number;
  delayMs: number;
  durationMs: number;
}

const SAFE_ADVANCE_AFTER_MS = 12_000;
const MIN_SNIPPETS_TO_ADVANCE = 4;

export function WebsiteStep({ machine }: { machine: OnboardingMachine }) {
  const { copy } = useOnboardingCopy();
  const initial = machine.state.website;
  const [url, setUrl] = useState(initial?.url ?? "");
  const [brief, setBrief] = useState(initial?.agentBrief ?? "");
  const [submitted, setSubmitted] = useState(false);
  const [cards, setCards] = useState<FallingCard[]>([]);
  const [doneCount, setDoneCount] = useState<number | null>(null);
  const [warning, setWarning] = useState<string | null>(null);

  const cardsRef = useRef<FallingCard[]>([]);
  cardsRef.current = cards;

  useEffect(() => {
    if (!submitted || !url.trim()) return undefined;

    const controller = new AbortController();
    let advanced = false;
    const advanceOnce = () => {
      if (advanced) return;
      advanced = true;
      window.setTimeout(() => machine.goTo("connect"), 700);
    };

    const safetyTimer = window.setTimeout(advanceOnce, SAFE_ADVANCE_AFTER_MS);

    void streamCrawlPreview(
      { url: url.trim(), brief: brief.trim() || undefined, maxPages: 8 },
      {
        onSnippet: (snippet) =>
          setCards((prev) => [...prev, toFallingCard(snippet, prev.length)]),
        onBranding: (payload) => {
          const branding = payload as BrandingSignals | null;
          if (!branding) return;
          const current = machine.state.website;
          machine.setWebsite({
            url: current?.url ?? url.trim(),
            agentBrief: current?.agentBrief ?? brief.trim(),
            crawlJobId: current?.crawlJobId,
            branding: { ...current?.branding, ...branding },
          });
        },
        onWarning: (w) => {
          const friendly = friendlyWarning(w.code, w.message, copy.website.warnings);
          if (friendly) setWarning(friendly);
        },
        onDone: (done) => {
          setDoneCount(typeof done.count === "number" ? done.count : null);
          advanceOnce();
        },
      },
      controller.signal,
    )
      .catch((err) => {
        if ((err as { name?: string }).name === "AbortError") return;
        setWarning(copy.website.warnings.unavailable);
      })
      .finally(() => {
        if (cardsRef.current.length >= MIN_SNIPPETS_TO_ADVANCE) advanceOnce();
        else window.setTimeout(advanceOnce, 2_500);
      });

    return () => {
      window.clearTimeout(safetyTimer);
      controller.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [submitted]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!url.trim() || submitted) return;
    machine.setWebsite({ url: url.trim(), agentBrief: brief.trim() });
    setSubmitted(true);
  };

  const fillPct = useMemo(() => {
    const target = doneCount ?? 8;
    return Math.min(100, Math.round((cards.length / Math.max(1, target)) * 100));
  }, [cards.length, doneCount]);

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.website.eyebrow}</StepEyebrow>
        <StepTitle>{copy.website.title}</StepTitle>
        <StepDescription>{copy.website.description}</StepDescription>

        <form onSubmit={submit} className="flex flex-col gap-5">
          <label className="block">
            <span className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              {copy.website.urlLabel}
            </span>
            <div className="mt-2 flex items-stretch overflow-hidden rounded-md border border-[#D6D2CB] bg-white focus-within:border-[#1F1B17]">
              <span className="flex items-center bg-[#F7F4ED] px-3 font-inter text-[12px] text-[#6B6660]">
                https://
              </span>
              <input
                autoFocus
                required
                type="text"
                inputMode="url"
                disabled={submitted}
                value={url.replace(/^https?:\/\//, "")}
                onChange={(e) =>
                  setUrl(e.target.value.replace(/^https?:\/\//, "").replace(/\s+/g, ""))
                }
                placeholder="aquatiq.com"
                className="flex-1 bg-white px-3 py-2.5 font-inter text-[14px] text-[#1F1B17] placeholder:text-[#A09890] focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
              />
            </div>
          </label>

          <label className="block">
            <span className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              {copy.website.briefLabel}
            </span>
            <textarea
              rows={2}
              disabled={submitted}
              value={brief}
              onChange={(e) => setBrief(e.target.value)}
              placeholder={copy.website.briefPlaceholder}
              className="mt-2 w-full resize-none rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 font-inter text-[13px] leading-5 text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
            />
          </label>

          {!submitted ? (
            <div className="flex items-center gap-4">
              <PrimaryButton type="submit" disabled={!url.trim()}>
                {copy.website.continue}
              </PrimaryButton>
              <SkipLink onClick={() => machine.goTo("connect")}>{copy.website.skip}</SkipLink>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              <p className="font-inter text-[12px] text-[#6B6660]">
                {formatOnboardingText(copy.website.fetching, { url })}
              </p>
              {warning && <p className="font-inter text-[11px] text-[#B07C2E]">{warning}</p>}
            </div>
          )}
        </form>
      </LeftPane>

      <RightPane>
        <SnippetDropFolder cards={cards} fillPct={fillPct} />
      </RightPane>
    </>
  );
}

function toFallingCard(snippet: CrawlSnippet, index: number): FallingCard {
  const seed = hashString(snippet.id);
  return {
    ...snippet,
    xOffsetPct: ((seed % 56) - 28) / 1,
    delayMs: (index % 3) * 220 + Math.floor((seed >> 8) % 250),
    durationMs: 1_600 + Math.floor((seed >> 16) % 1_000),
  };
}

function hashString(input: string): number {
  let hash = 0;
  for (let i = 0; i < input.length; i += 1) {
    hash = (hash * 31 + input.charCodeAt(i)) | 0;
  }
  return Math.abs(hash);
}

function friendlyWarning(
  code: string | undefined,
  fallback: string | undefined,
  warnings: Readonly<Record<string, string>>,
): string | null {
  switch (code) {
    case "invalid_url":
      return warnings.invalidUrl;
    case "bad_scheme":
      return warnings.badScheme;
    case "no_hostname":
      return warnings.noHostname;
    case "dns_failed":
      return warnings.dnsFailed;
    case "private_address":
      return warnings.privateAddress;
    case "control_unreachable":
      return warnings.unavailable;
    case "crawl_failed":
      return warnings.crawlFailed;
    case "no_events":
      return warnings.noEvents;
    case "knowledge_store_unavailable":
      return warnings.knowledgeStoreUnavailable;
    default:
      return fallback || null;
  }
}

function SnippetDropFolder({ cards, fillPct }: { cards: FallingCard[]; fillPct: number }) {
  return (
    <div className="relative flex size-full items-end justify-center px-10 pb-10">
      <div className="pointer-events-none absolute inset-x-0 top-0 h-[58%] overflow-hidden">
        {cards.map((card) => (
          <SnippetCard key={card.id} card={card} />
        ))}
      </div>
      <FolderCard count={cards.length} fillPct={fillPct} />
      <style>{`
        @keyframes velion-snippet-drop {
          0% { transform: translate(var(--vx, -50%), -20%) rotate(var(--vrot, 0deg)); opacity: 0; }
          15% { opacity: 1; }
          100% { transform: translate(var(--vx, -50%), 70%) rotate(var(--vrot, 0deg)); opacity: 1; }
        }
        @media (prefers-reduced-motion: reduce) {
          .velion-snippet { animation: none !important; opacity: 1 !important; transform: translate(-50%, 40%) !important; }
        }
      `}</style>
    </div>
  );
}

function SnippetCard({ card }: { card: FallingCard }) {
  const { copy } = useOnboardingCopy();
  const style: React.CSSProperties = {
    left: `calc(50% + ${card.xOffsetPct}%)`,
    animationDelay: `${card.delayMs}ms`,
    animationDuration: `${card.durationMs}ms`,
    animationName: "velion-snippet-drop",
    animationTimingFunction: "ease-out",
    animationFillMode: "forwards",
    ["--vx" as string]: "-50%",
    ["--vrot" as string]: `${((card.id.length * 7) % 7) - 3}deg`,
  };
  return (
    <div style={style} className="velion-snippet absolute top-0 will-change-transform">
      {renderCardBody(card, copy.website.kindLabels)}
    </div>
  );
}

function renderCardBody(
  card: FallingCard,
  kindLabel: Readonly<Record<CrawlSnippetKind, string>>,
): React.ReactElement {
  switch (card.kind) {
    case "image":
      return (
        <div className="w-[10.5rem] overflow-hidden rounded-md border border-[#E5DFD3] bg-white shadow-[0_8px_18px_rgba(31,27,23,0.10)]">
          <div className="relative h-20 w-full bg-[#F1ECDF]">
            {card.thumbUrl && (
              // eslint-disable-next-line @next/next/no-img-element
              <img
                src={card.thumbUrl}
                alt=""
                referrerPolicy="no-referrer"
                onError={(e) => {
                  (e.currentTarget as HTMLImageElement).style.display = "none";
                }}
                className="size-full object-cover"
              />
            )}
            <span className="absolute left-2 top-2 rounded-full bg-white/90 px-2 py-0.5 font-inter text-[9px] uppercase tracking-[0.14em] text-[#1F1B17]">
              {kindLabel.image}
            </span>
          </div>
          <div className="px-2.5 py-2">
            <p className="truncate font-inter text-[11px] text-[#1F1B17]">{card.title}</p>
          </div>
        </div>
      );
    case "file":
      return (
        <div className="flex w-[12rem] items-center gap-2.5 rounded-md border border-[#E5DFD3] bg-white px-3 py-2.5 shadow-[0_8px_18px_rgba(31,27,23,0.10)]">
          <div className="flex h-9 w-7 items-center justify-center rounded-sm bg-[#1F1B17] font-inter text-[9px] font-semibold uppercase tracking-[0.08em] text-white">
            {fileExt(card.contentType)}
          </div>
          <div className="min-w-0 flex-1">
            <p className="truncate font-inter text-[10px] uppercase tracking-[0.12em] text-[#A09890]">
              {kindLabel.file}
            </p>
            <p className="truncate font-inter text-[11px] text-[#1F1B17]">{card.title}</p>
          </div>
        </div>
      );
    case "link":
      return (
        <div className="flex w-[11rem] items-center gap-2 rounded-full border border-[#E5DFD3] bg-white px-3 py-1.5 shadow-[0_6px_14px_rgba(31,27,23,0.08)]">
          <span className="size-1.5 rounded-full bg-[#34D399]" />
          <span className="truncate font-inter text-[11px] text-[#1F1B17]">{card.title}</span>
        </div>
      );
    default:
      return (
        <div className="w-[12.5rem] rounded-md border border-[#E5DFD3] bg-white px-3 py-2.5 shadow-[0_8px_18px_rgba(31,27,23,0.10)]">
          <p className="font-inter text-[10px] uppercase tracking-[0.12em] text-[#A09890]">{card.title}</p>
          {card.excerpt && (
            <p className="mt-1 line-clamp-2 font-inter text-[11px] leading-4 text-[#1F1B17]">
              {card.excerpt}
            </p>
          )}
        </div>
      );
  }
}

function fileExt(contentType: string | undefined): string {
  const ct = (contentType ?? "").toLowerCase();
  if (ct === "application/pdf") return "PDF";
  if (ct.includes("word")) return "DOC";
  if (ct.includes("excel") || ct.includes("spreadsheet")) return "XLS";
  if (ct === "text/csv") return "CSV";
  return "FILE";
}

function FolderCard({ count, fillPct }: { count: number; fillPct: number }) {
  const { copy } = useOnboardingCopy();
  return (
    <div className="relative w-[18rem] rounded-2xl border border-[#E5DFD3] bg-white shadow-[0_8px_24px_rgba(31,27,23,0.10)]">
      <div className="absolute -top-3 left-6 h-3 w-20 rounded-t-md border border-b-0 border-[#E5DFD3] bg-white" />
      <div className="px-5 pb-4 pt-6">
        <p className="font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890]">
          {copy.website.folder.title}
        </p>
        <p
          className="mt-2 text-[20px] font-normal leading-[1.05] text-[#1F1B17]"
          style={{ fontFamily: "var(--font-geist-sans), var(--font-inter), Arial, sans-serif" }}
        >
          {formatOnboardingText(copy.website.folder.gathered, { count })}
        </p>
        <p className="mt-1 font-inter text-[11px] text-[#6B6660]">{copy.website.folder.description}</p>
      </div>
      <div className="border-t border-[#F1ECDF] px-5 py-3">
        <div className="flex items-center justify-between">
          <span className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">
            {copy.website.folder.progress}
          </span>
          <span className="font-inter text-[11px] tabular-nums text-[#1F1B17]">{fillPct}%</span>
        </div>
        <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-[#F1ECDF]">
          <div
            className="h-full rounded-full bg-[#1F1B17] transition-[width] duration-500"
            style={{ width: `${fillPct}%` }}
          />
        </div>
      </div>
    </div>
  );
}
