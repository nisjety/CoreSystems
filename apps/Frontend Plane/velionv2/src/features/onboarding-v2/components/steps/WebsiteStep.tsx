"use client";

/**
 * Step 2 — website seed (live crawl). The user pastes a URL; on submit the
 * step opens the crawl-preview SSE (BFF composition over quarry-edge), renders
 * each snippet as a falling card, persists server-normalized branding signals
 * to wizard state (the frame shows the brand pill), and leaves the user in
 * control of when to continue.
 *
 * Degrades open: the stream always terminates with evidence, warning, or a
 * terminal state so the user can continue without being trapped.
 */

import Link from "next/link";
import { useEffect, useMemo, useRef, useState } from "react";

import {
  streamCrawlPreview,
  type CrawlProgress,
  type CrawlSnippet,
  type CrawlSnippetKind,
} from "../../lib/onboarding-api";
import { formatOnboardingText, type OnboardingLocale, useOnboardingCopy } from "../../lib/onboarding-i18n";
import { updateProfile } from "../../lib/onboarding-service";
import type { BrandingSignals, CrawlEvidence, OnboardingMachine, WebsitePayload } from "../../lib/onboarding-machine";
import {
  allOnboardingWebsites,
  appendCrawlSnippet,
  createEmptyCrawlEvidence,
  removeWebsiteFromState,
  updateCrawlEvidence,
} from "../../lib/onboarding-evidence";
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

type LiveCrawlProgress = Omit<CrawlProgress, "status"> & { status: CrawlProgress["status"] | "idle" };

export function WebsiteStep({ machine }: { machine: OnboardingMachine }) {
  const { locale, copy } = useOnboardingCopy();
  const initial = machine.state.website;
  const [url, setUrl] = useState(initial?.url ?? "");
  const [brief, setBrief] = useState(initial?.agentBrief ?? "");
  const [submitted, setSubmitted] = useState(false);
  const [activeUrl, setActiveUrl] = useState(initial?.url ?? "");
  const [activeBrief, setActiveBrief] = useState(initial?.agentBrief ?? "");
  const [activeBranding, setActiveBranding] = useState<BrandingSignals | undefined>(initial?.branding);
  const [cards, setCards] = useState<FallingCard[]>([]);
  const [doneCount, setDoneCount] = useState<number | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [crawlEvidence, setCrawlEvidence] = useState<CrawlEvidence | undefined>(initial?.crawlEvidence);
  const [progress, setProgress] = useState<LiveCrawlProgress>({
    status: "idle",
    pages: 0,
    elements: 0,
    target: 3,
  });

  const cardsRef = useRef<FallingCard[]>([]);
  const brandingRef = useRef<BrandingSignals | undefined>(initial?.branding);
  const crawlEvidenceRef = useRef<CrawlEvidence | undefined>(initial?.crawlEvidence);
  const suppressStoredHydrationRef = useRef(false);
  useEffect(() => {
    cardsRef.current = cards;
  }, [cards]);
  useEffect(() => {
    brandingRef.current = activeBranding;
  }, [activeBranding]);

  useEffect(() => {
    if (submitted || suppressStoredHydrationRef.current || !initial?.url) return;

    const evidence = initial.crawlEvidence;
    setUrl(initial.url);
    setBrief(initial.agentBrief ?? "");
    setActiveUrl(initial.url);
    setActiveBrief(initial.agentBrief ?? "");
    setActiveBranding(initial.branding);
    brandingRef.current = initial.branding;
    crawlEvidenceRef.current = evidence;
    setCrawlEvidence(evidence);
    setCards((evidence?.snippets ?? []).map((snippet, index) => toFallingCard(snippet, index)));
    setDoneCount(isTerminalEvidence(evidence?.status) ? evidence?.pages ?? null : null);
    setProgress({
      status: evidence?.status ?? "idle",
      pages: evidence?.pages ?? 0,
      elements: evidence?.elements ?? 0,
      target: Math.max(3, evidence?.pages ?? evidence?.snippets.length ?? 0),
      latestUrl: evidence?.latestUrl,
      latestTitle: evidence?.latestTitle,
    });
  }, [initial?.agentBrief, initial?.branding, initial?.crawlEvidence, initial?.url, submitted]);

  useEffect(() => {
    if (!submitted || !url.trim()) return undefined;

    const controller = new AbortController();
    const websiteUrl = activeUrl || url.trim();
    const websiteBrief = activeBrief;
    const persistWebsite = (next: Partial<WebsitePayload>) => {
      const branding = next.branding ?? brandingRef.current;
      machine.addWebsite({
        url: websiteUrl,
        agentBrief: websiteBrief,
        ...(branding ? { branding } : {}),
        ...next,
      });
    };
    const updateEvidence = (updater: (current: CrawlEvidence | undefined) => CrawlEvidence) => {
      const next = updater(crawlEvidenceRef.current);
      crawlEvidenceRef.current = next;
      setCrawlEvidence(next);
      persistWebsite({ crawlEvidence: next });
    };

    void streamCrawlPreview(
      { url: websiteUrl, brief: websiteBrief || undefined, maxPages: 3 },
      {
        onStarted: (started) => {
          setProgress((prev) => ({
            ...prev,
            status: "starting",
            jobId: started.jobId ?? prev.jobId,
            target: started.target ?? prev.target,
          }));
          if (started.jobId) {
            persistWebsite({
              url: started.url ?? websiteUrl,
              agentBrief: websiteBrief,
              crawlJobId: started.jobId,
            });
          }
          updateEvidence((current) =>
            updateCrawlEvidence(current, {
              status: "starting",
              pages: 0,
              elements: 0,
              seedStatus: "pending",
            }),
          );
        },
        onSnippet: (snippet) => {
          setCards((prev) => {
            if (prev.some((card) => card.kind === snippet.kind && card.url === snippet.url)) {
              return prev;
            }
            return [...prev, toFallingCard(snippet, prev.length)].slice(-24);
          });
          updateEvidence((current) => appendCrawlSnippet(current, snippet));
        },
        onProgress: (nextProgress) => {
          setProgress((prev) => ({
            ...prev,
            ...nextProgress,
            pages: Math.max(prev.pages, nextProgress.pages),
            elements: Math.max(prev.elements, nextProgress.elements),
            target: nextProgress.target ?? prev.target,
            jobId: nextProgress.jobId ?? prev.jobId,
          }));
          if (nextProgress.jobId) {
            persistWebsite({
              crawlJobId: nextProgress.jobId,
            });
          }
          updateEvidence((current) =>
            updateCrawlEvidence(current, {
              status: nextProgress.status,
              pages: nextProgress.pages,
              elements: nextProgress.elements,
              latestUrl: nextProgress.latestUrl,
              latestTitle: nextProgress.latestTitle,
            }),
          );
        },
        onBranding: (payload) => {
          const branding = payload as BrandingSignals | null;
          if (!branding) return;
          const next = mergeBrandingSignals(brandingRef.current, branding);
          brandingRef.current = next;
          setActiveBranding(next);
          persistWebsite({ branding: next });
        },
        onWarning: (w) => {
          const friendly = friendlyWarning(w.code, w.message, copy.website.warnings);
          if (friendly) setWarning(friendly);
          if (friendly) {
            updateEvidence((current) => updateCrawlEvidence(current, { warnings: [friendly] }));
          }
        },
        onDone: (done) => {
          setDoneCount(
            typeof done.pages === "number"
              ? done.pages
              : typeof done.count === "number"
                ? done.count
                : null,
          );
          setProgress((prev) => ({
            ...prev,
            status:
              done.status === "failed" || done.status === "cancelled"
                ? (done.status as LiveCrawlProgress["status"])
                : "completed",
            pages: Math.max(prev.pages, typeof done.pages === "number" ? done.pages : prev.pages),
            elements: Math.max(
              prev.elements,
              typeof done.elements === "number" ? done.elements : prev.elements,
            ),
          }));
          updateEvidence((current) =>
            updateCrawlEvidence(current, {
              status:
                done.status === "failed" || done.status === "cancelled"
                  ? done.status
                  : "completed",
              pages: typeof done.pages === "number" ? done.pages : current?.pages,
              elements: typeof done.elements === "number" ? done.elements : current?.elements,
              seedStatus: "pending",
            }),
          );
        },
      },
      controller.signal,
    )
      .catch((err) => {
        if ((err as { name?: string }).name === "AbortError") return;
        setWarning(copy.website.warnings.unavailable);
        updateEvidence((current) => updateCrawlEvidence(current, { status: "failed", warnings: [copy.website.warnings.unavailable] }));
      })
      .finally(() => undefined);

    return () => {
      controller.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [submitted]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!url.trim() || submitted) return;
    suppressStoredHydrationRef.current = false;
    const websiteUrl = url.trim();
    const agentBrief = brief.trim();
    setActiveUrl(websiteUrl);
    setActiveBrief(agentBrief);
    setActiveBranding(undefined);
    brandingRef.current = undefined;
    const initialEvidence = createEmptyCrawlEvidence();
    crawlEvidenceRef.current = initialEvidence;
    setCrawlEvidence(initialEvidence);
    setCards([]);
    setDoneCount(null);
    setWarning(null);
    setProgress({ status: "starting", pages: 0, elements: 0, target: 3 });
    machine.addWebsite({ url: websiteUrl, agentBrief, crawlEvidence: initialEvidence });
    void updateProfile({
      name: machine.state.organization?.name,
      website: websiteUrl,
      brief: agentBrief || undefined,
    });
    setSubmitted(true);
  };

  const fillPct = useMemo(() => {
    const target = progress.target ?? doneCount ?? 3;
    const current = Math.max(progress.pages, cards.length);
    return Math.min(100, Math.round((current / Math.max(1, target)) * 100));
  }, [cards.length, doneCount, progress.pages, progress.target]);
  const websites = allOnboardingWebsites(machine.state);
  const running = submitted && (progress.status === "starting" || progress.status === "running");
  const hasCrawlSignal =
    Boolean(crawlEvidence && crawlEvidence.status !== "idle") ||
    Boolean(crawlEvidence?.snippets.length) ||
    Boolean(crawlEvidence?.warnings.length) ||
    websites.some((site) => Boolean(site.crawlEvidence && site.crawlEvidence.status !== "idle"));
  const showingStoredCrawl = !submitted && hasCrawlSignal;
  const showingPostCrawlActions = submitted || showingStoredCrawl;
  const canContinue =
    hasCrawlSignal ||
    ["completed", "failed", "cancelled"].includes(progress.status) ||
    Boolean(crawlEvidence && crawlEvidence.snippets.length > 0);

  const addAnotherWebsite = () => {
    suppressStoredHydrationRef.current = true;
    setSubmitted(false);
    setUrl("");
    setBrief("");
    setActiveUrl("");
    setActiveBrief("");
    setActiveBranding(undefined);
    setCards([]);
    setDoneCount(null);
    setWarning(null);
    crawlEvidenceRef.current = undefined;
    setCrawlEvidence(undefined);
    setProgress({ status: "idle", pages: 0, elements: 0, target: 3 });
  };

  const removeWebsite = (targetUrl: string) => {
    const nextState = removeWebsiteFromState(machine.state, targetUrl);
    machine.removeWebsite(targetUrl);
    if (!nextState.website) addAnotherWebsite();
  };

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
                disabled={submitted || showingStoredCrawl}
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
              disabled={submitted || showingStoredCrawl}
              value={brief}
              onChange={(e) => setBrief(e.target.value)}
              placeholder={copy.website.briefPlaceholder}
              className="mt-2 w-full resize-none rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 font-inter text-[13px] leading-5 text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
            />
          </label>

          {!showingPostCrawlActions ? (
            <div className="flex items-center gap-4">
              <PrimaryButton type="submit" disabled={!url.trim()}>
                {copy.website.continue}
              </PrimaryButton>
              <SkipLink onClick={() => machine.goTo("organization")}>{copy.website.skip}</SkipLink>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              {submitted && (
                <p className="font-inter text-[12px] text-[#6B6660]">
                  {formatOnboardingText(copy.website.fetching, { url })}
                </p>
              )}
              {warning && <p className="font-inter text-[11px] text-[#B07C2E]">{warning}</p>}
              <div className="flex flex-wrap items-center gap-3 pt-2">
                <PrimaryButton onClick={() => machine.goTo("organization")} disabled={!canContinue}>
                  {copy.website.continue}
                </PrimaryButton>
                <Link
                  href="/ingestions"
                  className="font-inter text-[11px] uppercase tracking-[0.18em] text-[#6B6660] transition-colors hover:text-[#111111]"
                >
                  Open Ingestions
                </Link>
                <button
                  type="button"
                  onClick={addAnotherWebsite}
                  disabled={running}
                  className="font-inter text-[11px] uppercase tracking-[0.18em] text-[#A09890] transition-colors hover:text-[#111111] disabled:cursor-not-allowed disabled:opacity-45"
                >
                  {websiteActionCopy(locale).addWebsite}
                </button>
                <SkipLink onClick={() => machine.goTo("organization")}>{copy.website.skip}</SkipLink>
              </div>
            </div>
          )}
        </form>
      </LeftPane>

      <RightPane>
        <SnippetDropFolder
          cards={cards}
          fillPct={fillPct}
          progress={progress}
          evidence={crawlEvidence}
          websites={websites}
          locale={locale}
          onRemove={removeWebsite}
        />
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

function isTerminalEvidence(status: CrawlEvidence["status"] | undefined): boolean {
  return status === "completed" || status === "failed" || status === "cancelled";
}

function mergeBrandingSignals(
  current: BrandingSignals | undefined,
  incoming: BrandingSignals,
): BrandingSignals {
  const palette = Array.from(new Set([...(current?.palette ?? []), ...(incoming.palette ?? [])]));
  return {
    ...current,
    ...incoming,
    themeColor: incoming.themeColor ?? current?.themeColor,
    palette: palette.length > 0 ? palette : undefined,
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
    case "auth_unavailable":
    case "unauthorized":
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

function SnippetDropFolder({
  cards,
  fillPct,
  progress,
  evidence,
  websites,
  locale,
  onRemove,
}: {
  cards: FallingCard[];
  fillPct: number;
  progress: LiveCrawlProgress;
  evidence: CrawlEvidence | undefined;
  websites: WebsitePayload[];
  locale: OnboardingLocale;
  onRemove: (url: string) => void;
}) {
  return (
    <div className="relative flex size-full items-end justify-center px-10 pb-10">
      <div className="pointer-events-none absolute inset-x-0 top-0 h-[58%] overflow-hidden">
        {cards.map((card) => (
          <SnippetCard key={card.id} card={card} />
        ))}
      </div>
      <FolderCard
        count={cards.length}
        fillPct={fillPct}
        progress={progress}
        recentCards={cards.slice(-4).reverse()}
      />
      <WebsiteEvidencePanel
        evidence={evidence}
        websites={websites}
        locale={locale}
        onRemove={onRemove}
      />
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

function WebsiteEvidencePanel({
  evidence,
  websites,
  locale,
  onRemove,
}: {
  evidence: CrawlEvidence | undefined;
  websites: WebsitePayload[];
  locale: OnboardingLocale;
  onRemove: (url: string) => void;
}) {
  const copy = websiteActionCopy(locale);
  const contentTypes = evidence?.contentTypes.length ? evidence.contentTypes : inferredContentTypes(websites);
  return (
    <aside className="absolute right-5 top-5 w-[min(19rem,calc(100%-2.5rem))] rounded-xl border border-[#E5DFD3] bg-white/90 p-4 shadow-[0_18px_36px_rgba(31,27,23,0.12)] backdrop-blur">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="font-inter text-[9px] font-semibold uppercase tracking-[0.16em] text-[#A09890]">{copy.readOnly}</p>
          <h3 className="mt-1 font-inter text-[14px] font-semibold leading-5 text-[#1F1B17]">{copy.title}</h3>
        </div>
        <span className="rounded-full bg-[#F5F1EC] px-2 py-1 font-mono text-[9px] uppercase tracking-[0.12em] text-[#6B6660]">
          {copy.status[evidence?.status ?? "idle"]}
        </span>
      </div>
      <div className="mt-3 grid grid-cols-3 gap-2">
        <Metric label={copy.pages} value={String(evidence?.pages ?? 0)} />
        <Metric label={copy.elements} value={String(evidence?.elements ?? 0)} />
        <Metric label={copy.sources} value={String(websites.length)} />
      </div>
      {contentTypes.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-1.5">
          {contentTypes.slice(0, 5).map((type) => (
            <span key={type} className="rounded-full bg-[#F0EFED] px-2 py-1 font-inter text-[10px] text-[#4E4A45]">
              {typeLabel(type)}
            </span>
          ))}
        </div>
      )}
      {websites.length > 0 && (
        <div className="mt-3 border-t border-[#F1ECDF] pt-3">
          <p className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">{copy.websites}</p>
          <div className="mt-2 flex max-h-32 flex-col gap-1.5 overflow-y-auto">
            {websites.map((site) => (
              <div key={site.url} className="flex min-w-0 items-center justify-between gap-2 rounded-md bg-[#F7F4ED] px-2.5 py-2">
                <span className="min-w-0">
                  <span className="block truncate font-inter text-[11px] font-medium text-[#1F1B17]">{websiteHost(site.url)}</span>
                  <span className="block truncate font-inter text-[9px] uppercase tracking-[0.12em] text-[#A09890]">
                    {copy.sourceStatus[site.crawlEvidence?.seedStatus ?? "pending"]}
                  </span>
                </span>
                <button
                  type="button"
                  onClick={() => onRemove(site.url)}
                  className="shrink-0 font-inter text-[9px] font-semibold uppercase tracking-[0.12em] text-[#A09890] transition-colors hover:text-[#B42318]"
                >
                  {copy.remove}
                </button>
              </div>
            ))}
          </div>
        </div>
      )}
      <p className="mt-3 border-t border-[#F1ECDF] pt-3 font-inter text-[10.5px] leading-4 text-[#6B6660]">
        {copy.knowledgeHint}
      </p>
    </aside>
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

function FolderCard({
  count,
  fillPct,
  progress,
  recentCards,
}: {
  count: number;
  fillPct: number;
  progress: LiveCrawlProgress;
  recentCards: FallingCard[];
}) {
  const { copy, formatNumber } = useOnboardingCopy();
  const liveCopy = copy.website.live;
  const statusLabel = liveCopy.status[progress.status];
  const itemCount = Math.max(progress.pages, count);
  return (
    <div className="relative w-[20rem] rounded-2xl border border-[#E5DFD3] bg-white shadow-[0_8px_24px_rgba(31,27,23,0.10)]">
      <div className="absolute -top-3 left-6 h-3 w-20 rounded-t-md border border-b-0 border-[#E5DFD3] bg-white" />
      <div className="px-5 pb-4 pt-6">
        <div className="flex items-start justify-between gap-3">
          <p className="font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890]">
            {copy.website.folder.title}
          </p>
          <span className="inline-flex h-6 shrink-0 items-center gap-1.5 rounded-full bg-[#F5F1EC] px-2 font-inter text-[10px] font-semibold uppercase tracking-[0.12em] text-[#6B6660]">
            <span
              className={`size-1.5 rounded-full ${
                progress.status === "running" ? "bg-[#10B981]" : "bg-[#A09890]"
              }`}
            />
            {statusLabel}
          </span>
        </div>
        <p
          className="mt-2 text-[20px] font-normal leading-[1.05] text-[#1F1B17]"
          style={{ fontFamily: "var(--font-geist-sans), var(--font-inter), Arial, sans-serif" }}
        >
          {formatOnboardingText(copy.website.folder.gathered, { count: itemCount })}
        </p>
        <p className="mt-1 font-inter text-[11px] text-[#6B6660]">{copy.website.folder.description}</p>
        <div className="mt-4 grid grid-cols-2 gap-2">
          <Metric label={liveCopy.items} value={formatNumber(itemCount)} />
          <Metric label={liveCopy.elements} value={formatNumber(progress.elements)} />
        </div>
        <div className="mt-4">
          <p className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">
            {liveCopy.recent}
          </p>
          <ul className="mt-2 min-h-[7.25rem] overflow-hidden border-y border-[#F1ECDF]">
            {recentCards.length > 0 ? (
              recentCards.map((card) => (
                <li
                  key={`${card.kind}:${card.url}`}
                  className="flex h-9 items-center justify-between gap-3 border-t border-[#F1ECDF] first:border-t-0"
                >
                  <span className="min-w-0">
                    <span className="block truncate font-inter text-[11px] text-[#1F1B17]">
                      {card.title}
                    </span>
                    <span className="block truncate font-inter text-[9px] uppercase tracking-[0.12em] text-[#A09890]">
                      {card.source === "live" ? liveCopy.live : liveCopy.seed}
                    </span>
                  </span>
                  <span className="shrink-0 font-inter text-[10px] tabular-nums text-[#6B6660]">
                    {formatNumber(card.elementCount ?? 0)}
                  </span>
                </li>
              ))
            ) : (
              <li className="flex h-[7.25rem] items-center font-inter text-[11px] text-[#A09890]">
                {liveCopy.empty}
              </li>
            )}
          </ul>
        </div>
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

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-[#F1ECDF] px-3 py-2">
      <p className="font-inter text-[9px] uppercase tracking-[0.14em] text-[#A09890]">{label}</p>
      <p className="mt-1 font-inter text-[16px] tabular-nums text-[#1F1B17]">{value}</p>
    </div>
  );
}

function websiteActionCopy(locale: OnboardingLocale) {
  if (locale === "nb") {
    return {
      addWebsite: "Legg til nettside",
      remove: "Fjern",
      readOnly: "Kun innsyn",
      title: "Dette fant Velion",
      pages: "Sider",
      elements: "Elementer",
      sources: "Kilder",
      websites: "Nettsider",
      knowledgeHint: "Du kan redigere, fjerne og auditere kildene fra Knowledge etter onboarding.",
      status: {
        idle: "klar",
        starting: "starter",
        running: "crawler",
        completed: "ferdig",
        failed: "feilet",
        cancelled: "stoppet",
      },
      sourceStatus: {
        pending: "venter på datagraf",
        ready: "klar i datagraf",
        failed: "ikke lagt til",
        removed: "fjernet",
      },
    } as const;
  }
  return {
    addWebsite: "Add website",
    remove: "Remove",
    readOnly: "Read only",
    title: "What Velion found",
    pages: "Pages",
    elements: "Elements",
    sources: "Sources",
    websites: "Websites",
    knowledgeHint: "You can edit, remove and audit sources from Knowledge after onboarding.",
    status: {
      idle: "ready",
      starting: "starting",
      running: "crawling",
      completed: "done",
      failed: "failed",
      cancelled: "stopped",
    },
    sourceStatus: {
      pending: "pending graph",
      ready: "ready in graph",
      failed: "not added",
      removed: "removed",
    },
  } as const;
}

function inferredContentTypes(websites: WebsitePayload[]): string[] {
  const types = new Set<string>();
  for (const site of websites) {
    for (const snippet of site.crawlEvidence?.snippets ?? []) {
      if (snippet.contentType) types.add(snippet.contentType);
      else types.add(snippet.kind);
    }
  }
  return Array.from(types).slice(0, 5);
}

function typeLabel(type: string): string {
  const lower = type.toLowerCase();
  if (lower.includes("html") || lower === "text") return "page";
  if (lower.includes("image")) return "image";
  if (lower.includes("pdf")) return "pdf";
  if (lower.includes("json")) return "data";
  if (lower === "link") return "link";
  return lower.split(/[;/]/)[0].replace(/^application\//, "");
}

function websiteHost(value: string): string {
  try {
    return new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(value) ? value : `https://${value}`).host.replace(/^www\./i, "");
  } catch {
    return value.replace(/^https?:\/\//i, "").split(/[/?#]/)[0] || value;
  }
}
