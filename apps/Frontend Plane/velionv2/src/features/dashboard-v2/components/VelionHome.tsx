"use client";

import Image from "next/image";
import Link from "next/link";
import type { Route } from "next";
import { useRouter } from "next/navigation";
import {
	useCallback,
	useDeferredValue,
	useEffect,
	useReducer,
	useRef,
	useState,
} from "react";
import {
	getComposerTools,
	getGatewayModelForComposerModel,
} from "@/features/chat-v2/hooks/use-chat-dashboard-composer";
import {
	launchChatSessionFromComposer,
	writeChatLaunchMotion,
} from "@/features/chat-v2/lib/chat-workspace";
import {
	ArrowRight,
	CirclePlus,
	ExternalLink,
	Loader2,
	Search,
	Sparkles,
} from "lucide-react";
import {
	dashboardCards,
	type DashboardCard,
} from "@/features/dashboard-v2/lib/dashboard-surface";
import {
  NewsDashboardCard,
  TrafficDashboardCard,
  WeatherDashboardCard,
} from "@/features/dashboard-v2/components/VelionInformationCards";
import { DashboardComposer } from "@/features/composer-v2/components/DashboardComposer";
import {
	formatComposerTurnTime,
	loadComposerSettings,
} from "@/features/composer-v2/lib/dashboard-composer-storage";
import {
	type ComposerFile,
	type ComposerSettings,
	type ComposerTurn,
	type DashboardComposerModel,
	type ResponseMode,
} from "@/features/dashboard-v2/lib/dashboard-composer-model";
import { TopLayerTooltip } from "@/features/shell-v2/components/TopLayerTooltip";
import {
	readCachedValue,
	writeCachedValue,
} from "@/features/search-v2/lib/search-query-cache";
import { useControlPlaneContext } from "@/features/shell-v2/lib/control-plane-provider";
import { formatPlanLabel } from "@/features/shell-v2/lib/shell-data";
import { apiGet, apiSend } from "@/lib/api/client-envelope";
import { cn } from "@/lib/utils";

type DashboardTab = "Chat" | "Søk" | "Kunnskap";

type SearchSuggestion = {
	text: string;
	source: string;
	collection: string;
	object: string;
};

type VelionHomeState = {
	activeTab: DashboardTab;
	browseWeb: boolean;
	cardPage: number;
	deepSearch: boolean;
	files: ComposerFile[];
	historyOpen: boolean;
	isLaunchingChat: boolean;
	message: string;
	modelOpen: boolean;
	responseMode: ResponseMode;
	selectedModel: DashboardComposerModel;
	settings: ComposerSettings;
	settingsOpen: boolean;
	suggestionsOpen: boolean;
	turns: ComposerTurn[];
	voiceMode: boolean;
};

type VelionHomeAction =
	| { type: "active-tab-changed"; tab: DashboardTab }
	| { type: "browse-web-changed"; active: boolean }
	| { type: "card-page-next"; pageCount: number }
	| { type: "deep-search-changed"; active: boolean }
	| { type: "files-changed"; files: ComposerFile[] }
	| { type: "history-open-changed"; open: boolean }
	| { type: "message-changed"; message: string }
	| { type: "message-submitted"; turn: ComposerTurn }
	| { type: "model-changed"; model: DashboardComposerModel }
	| { type: "model-open-changed"; open: boolean }
	| { type: "open-agent-builder" }
	| { type: "response-mode-changed"; mode: ResponseMode }
	| { type: "settings-changed"; settings: ComposerSettings }
	| { type: "settings-open-changed"; open: boolean }
	| { type: "suggestions-open-changed"; open: boolean }
	| { type: "voice-mode-changed"; active: boolean }
	| { type: "card-prompt-applied"; card: DashboardCard }
	| { type: "chat-launch-started" };

function createInitialVelionHomeState(): VelionHomeState {
	return {
		activeTab: "Chat",
		browseWeb: true,
		cardPage: 0,
		deepSearch: false,
		files: [],
		historyOpen: false,
		isLaunchingChat: false,
		message: "",
		modelOpen: false,
		responseMode: "auto",
		selectedModel: "GPT-4o Mini",
		settings: loadComposerSettings(),
		settingsOpen: false,
		suggestionsOpen: false,
		turns: [],
		voiceMode: false,
	};
}

function velionHomeReducer(
	state: VelionHomeState,
	action: VelionHomeAction,
): VelionHomeState {
	switch (action.type) {
		case "active-tab-changed":
			return { ...state, activeTab: action.tab };
		case "browse-web-changed":
			return { ...state, browseWeb: action.active };
		case "card-page-next":
			return {
				...state,
				cardPage: (state.cardPage + 1) % action.pageCount,
			};
		case "deep-search-changed":
			return { ...state, deepSearch: action.active };
		case "files-changed":
			return { ...state, files: action.files };
		case "history-open-changed":
			return { ...state, historyOpen: action.open };
		case "message-changed":
			return { ...state, message: action.message };
		case "message-submitted":
			return {
				...state,
				files: [],
				historyOpen: false,
				message: "",
				modelOpen: false,
				settingsOpen: false,
				suggestionsOpen: false,
				turns: [action.turn, ...state.turns].slice(0, 6),
			};
		case "model-changed":
			return { ...state, selectedModel: action.model };
		case "model-open-changed":
			return { ...state, modelOpen: action.open };
		case "open-agent-builder":
			return {
				...state,
				message:
					"Opprett en agent som håndterer kundesamtaler med kunnskapsbase, tone og eskaleringer.",
				suggestionsOpen: true,
			};
		case "response-mode-changed":
			return { ...state, responseMode: action.mode };
		case "settings-changed":
			return { ...state, settings: action.settings };
		case "settings-open-changed":
			return { ...state, settingsOpen: action.open };
		case "suggestions-open-changed":
			return { ...state, suggestionsOpen: action.open };
		case "voice-mode-changed":
			return { ...state, voiceMode: action.active };
		case "card-prompt-applied":
			return {
				...state,
				activeTab: "Chat",
				browseWeb:
					action.card.id === "weather" ||
					action.card.id === "traffic" ||
					action.card.id === "news"
						? true
						: state.browseWeb,
				deepSearch: false,
				message: action.card.prompt,
			};
		case "chat-launch-started":
			return { ...state, isLaunchingChat: true };
		default:
			return state;
	}
}

type WebSearchResult = {
	url: string;
	title?: string;
	snippet?: string;
};

type WebSearchCitation = {
	url: string;
	title?: string;
};

type ImageHit = {
	url: string;
	thumbnailUrl: string;
	imageUrl: string;
	title: string | null;
};

const DASHBOARD_SEARCH_IMAGE_CACHE_TTL_MS = 5 * 60 * 1000;
const DASHBOARD_SEARCH_SUGGESTION_CACHE_TTL_MS = 30 * 1000;
const dashboardSearchImageCache = new Map<
	string,
	{ expiresAt: number; value: ImageHit[] }
>();
const dashboardSearchSuggestionCache = new Map<
	string,
	{ expiresAt: number; value: SearchSuggestion[] }
>();

type WebSearchPayload = {
	mode?: "search" | "fetch";
	results?: WebSearchResult[];
	answer?: string;
	citations?: WebSearchCitation[];
	url?: string;
	title?: string;
	description?: string | null;
	excerpt?: string | null;
};

type FetchedPage = {
	url: string;
	title: string;
	description: string | null;
	excerpt: string | null;
};

type PreviewResult = {
	url: string;
	title: string;
	hostname: string;
	snippet: string | null;
};

type SearchPanelState = {
	query: string;
	submittedQuery: string;
	suggestions: SearchSuggestion[];
	suggestionsQuery: string;
	highlightedIndex: number;
	previewResults: PreviewResult[];
	previewQuery: string;
	previewLoading: boolean;
	webMode: "search" | "fetch" | null;
	webResults: WebSearchResult[];
	webAnswer: string;
	webCitations: WebSearchCitation[];
	webFetchedPage: FetchedPage | null;
	webLoading: boolean;
	webError: string | null;
};

type SearchPanelAction =
	| { type: "query-changed"; query: string }
	| { type: "reset" }
	| { type: "submitted"; query: string }
	| { type: "suggestions-cleared" }
	| {
			type: "suggestions-loaded";
			query: string;
			suggestions: SearchSuggestion[];
	  }
	| { type: "highlight-changed"; index: number }
	| { type: "preview-loading"; query: string }
	| { type: "preview-loaded"; query: string; results: PreviewResult[] }
	| { type: "preview-cleared" }
	| { type: "web-search-started"; keepExisting?: boolean }
	| { type: "web-cleared" }
	| {
			type: "web-results-loaded";
			results: WebSearchResult[];
			answer: string;
			citations: WebSearchCitation[];
	  }
	| { type: "web-fetch-loaded"; page: FetchedPage }
	| { type: "web-search-error"; message: string };

const initialSearchPanelState: SearchPanelState = {
	query: "",
	submittedQuery: "",
	suggestions: [],
	suggestionsQuery: "",
	highlightedIndex: -1,
	previewResults: [],
	previewQuery: "",
	previewLoading: false,
	webMode: null,
	webResults: [],
	webAnswer: "",
	webCitations: [],
	webFetchedPage: null,
	webLoading: false,
	webError: null,
};

function searchPanelReducer(
	state: SearchPanelState,
	action: SearchPanelAction,
): SearchPanelState {
	switch (action.type) {
		case "query-changed":
			return { ...state, query: action.query, highlightedIndex: -1 };
		case "reset":
			return initialSearchPanelState;
		case "submitted":
			return {
				...state,
				query: action.query,
				submittedQuery: action.query,
				suggestions: [],
				suggestionsQuery: "",
				highlightedIndex: -1,
				previewResults: [],
				previewQuery: "",
				previewLoading: false,
			};
		case "suggestions-cleared":
			return {
				...state,
				suggestions: [],
				suggestionsQuery: "",
				highlightedIndex: -1,
			};
		case "suggestions-loaded":
			return {
				...state,
				suggestions: action.suggestions,
				suggestionsQuery: action.query,
				highlightedIndex: -1,
			};
		case "highlight-changed":
			return { ...state, highlightedIndex: action.index };
		case "preview-loading":
			return {
				...state,
				previewLoading: true,
				previewQuery: action.query,
			};
		case "preview-loaded":
			return {
				...state,
				previewLoading: false,
				previewResults: action.results,
				previewQuery: action.query,
			};
		case "preview-cleared":
			return {
				...state,
				previewLoading: false,
				previewResults: [],
				previewQuery: "",
			};
		case "web-search-started":
			return {
				...state,
				webLoading: true,
				webError: null,
				webMode: action.keepExisting ? state.webMode : null,
				webResults: action.keepExisting ? state.webResults : [],
				webAnswer: action.keepExisting ? state.webAnswer : "",
				webCitations: action.keepExisting ? state.webCitations : [],
				webFetchedPage: action.keepExisting
					? state.webFetchedPage
					: null,
			};
		case "web-cleared":
			return {
				...state,
				webLoading: false,
				webError: null,
				webMode: null,
				webResults: [],
				webAnswer: "",
				webCitations: [],
				webFetchedPage: null,
			};
		case "web-results-loaded":
			return {
				...state,
				webLoading: false,
				webMode: "search",
				webResults: action.results,
				webAnswer: action.answer,
				webCitations: action.citations,
			};
		case "web-fetch-loaded":
			return {
				...state,
				webLoading: false,
				webMode: "fetch",
				webFetchedPage: action.page,
			};
		case "web-search-error":
			return { ...state, webLoading: false, webError: action.message };
		default:
			return state;
	}
}

function safeHostname(url: string): string {
	try {
		return new URL(url).hostname.replace(/^www\./, "");
	} catch {
		return url;
	}
}

function buildPreviewResults(payload: WebSearchPayload): PreviewResult[] {
	if (payload.mode === "fetch" || payload.url) {
		if (!payload.url) return [];
		return [
			{
				url: payload.url,
				title: payload.title ?? payload.url,
				hostname: safeHostname(payload.url),
				snippet: payload.description ?? payload.excerpt ?? null,
			},
		];
	}

	const results = Array.isArray(payload.results)
		? payload.results
				.filter(
					(result) =>
						typeof result.url === "string" &&
						result.url.trim().length > 0,
				)
				.slice(0, 6)
		: [];

	return results.map((result) => ({
		url: result.url,
		title: result.title ?? result.url,
		hostname: safeHostname(result.url),
		snippet: result.snippet ?? null,
	}));
}

const tabs: DashboardTab[] = ["Chat", "Søk", "Kunnskap"];
const aboveFoldDashboardCardIds = new Set(["weather", "traffic", "news"]);

function getNorwegianGreeting() {
	const hour = new Date().getHours();

	if (hour < 11) {
		return "God morgen";
	}

	if (hour < 17) {
		return "God ettermiddag";
	}

	return "God kveld";
}

function firstName(value?: string | null) {
	const trimmed = value?.trim();
	if (!trimmed) return "";
	if (trimmed.includes("@")) return trimmed.split("@")[0] || "";
	return trimmed.split(/\s+/)[0] || "";
}

export function VelionHome() {
	const composerRef = useRef<HTMLDivElement>(null);
	const router = useRouter();
	const controlPlane = useControlPlaneContext();
	const greeting = getNorwegianGreeting();
	const [state, dispatch] = useReducer(
		velionHomeReducer,
		undefined,
		createInitialVelionHomeState,
	);
	const [searchExpanded, setSearchExpanded] = useState(false);
	const [searchPreviewActive, setSearchPreviewActive] = useState(false);
	const [searchPanelSnapshot, setSearchPanelSnapshot] =
		useState<SearchPanelSnapshot | null>(null);
	const {
		activeTab,
		browseWeb,
		cardPage,
		deepSearch,
		files,
		historyOpen,
		isLaunchingChat,
		message,
		modelOpen,
		responseMode,
		selectedModel,
		settings,
		settingsOpen,
		suggestionsOpen,
		turns,
		voiceMode,
	} = state;

	const pageCount = Math.ceil(dashboardCards.length / 3);
	const visibleCards = dashboardCards.slice(cardPage * 3, cardPage * 3 + 3);
	const displayName = firstName(
		controlPlane.user?.name ?? controlPlane.user?.email,
	);
	const planLabel = formatPlanLabel(
		controlPlane.entitlements?.plan ?? controlPlane.organization?.plan,
	);
	const searchModeExpanded = activeTab === "Søk" && searchExpanded;
	const searchModePreviewing =
		activeTab === "Søk" && searchPreviewActive && !searchExpanded;
	const homeTitle =
		activeTab === "Søk"
			? "Søk på nett of i velion"
			: activeTab === "Kunnskap"
				? "Kunnskapsbase"
				: displayName
					? `${greeting}, ${displayName}`
					: greeting;

	const changeActiveTab = (tab: DashboardTab) => {
		dispatch({ type: "active-tab-changed", tab });
		if (tab !== "Søk") {
			setSearchExpanded(false);
			setSearchPanelSnapshot(null);
			setSearchPreviewActive(false);
		}
	};

	useEffect(() => {
		router.prefetch("/chat" as Route);
	}, [router]);

	const submitMessage = useCallback(() => {
		const body = message.trim();
		if (!body && files.length === 0) return;
		if (isLaunchingChat) return;

		const now = new Date();
		const submittedText = body || "Vedlegg sendt til Velion.";
		const nextTurn: ComposerTurn = {
			id: `turn-${now.getTime()}`,
			body: submittedText,
			model: selectedModel,
			responseMode,
			browseWeb,
			deepSearch,
			files: files.map((file) => file.name),
			createdAt: formatComposerTurnTime(now),
			createdAtIso: now.toISOString(),
		};

		const launchedSession = launchChatSessionFromComposer({
			text: submittedText,
			model: getGatewayModelForComposerModel(selectedModel),
			tools: getComposerTools({
				browseWeb,
				deepSearch,
				message: body,
				responseMode,
			}),
			attachments: files.map((file) => ({
				id: file.id,
				name: file.name,
				size: file.size,
				type: file.type || "application/octet-stream",
			})),
		});
		writeChatLaunchMotion(launchedSession.id, submittedText);

		dispatch({ type: "chat-launch-started" });

		const prefersReducedMotion = window.matchMedia(
			"(prefers-reduced-motion: reduce)",
		).matches;
		window.setTimeout(() => {
			dispatch({ type: "message-submitted", turn: nextTurn });
			router.push("/chat" as Route);
		}, prefersReducedMotion ? 0 : 300);
	}, [
		browseWeb,
		deepSearch,
		files,
		isLaunchingChat,
		message,
		responseMode,
		router,
		selectedModel,
	]);

	const applyCardPrompt = (card: DashboardCard) => {
		dispatch({ type: "card-prompt-applied", card });
		window.requestAnimationFrame(() => {
			composerRef.current?.scrollIntoView({
				behavior: "smooth",
				block: "center",
			});
		});
	};

	const openAgentBuilder = () => {
		dispatch({ type: "open-agent-builder" });
	};

	const composerContent = (
		<div
			className={cn(
				"relative w-full px-0 transition-all duration-500 ease-out",
				searchModeExpanded
					? "mx-auto h-full max-w-[1180px] lg:w-[82%]"
					: "mx-auto max-w-[820px] lg:w-[68%]",
			)}
		>
			<div
				ref={composerRef}
				className={cn(
					"velion-home-composer velion-fade-up velion-stagger-1 w-full transition-all duration-500 ease-out",
					searchModeExpanded
						? "velion-home-composer-expanded mx-auto h-full max-w-[1180px]"
						: "mx-auto max-w-[820px]",
					isLaunchingChat ? "velion-chat-launch-out" : "",
				)}
			>
				{activeTab === "Chat" ? (
					<DashboardComposer
						browseWeb={browseWeb}
						deepSearch={deepSearch}
						files={files}
						historyOpen={historyOpen}
						message={message}
						modelOpen={modelOpen}
						responseMode={responseMode}
						selectedModel={selectedModel}
						settings={settings}
						settingsOpen={settingsOpen}
						suggestionsOpen={suggestionsOpen}
						turns={turns}
						voiceMode={voiceMode}
						onBrowseWebChange={(active) =>
							dispatch({ type: "browse-web-changed", active })
						}
						onDeepSearchChange={(active) =>
							dispatch({ type: "deep-search-changed", active })
						}
						onFilesChange={(nextFiles) =>
							dispatch({
								type: "files-changed",
								files: nextFiles,
							})
						}
						onHistoryOpenChange={(open) =>
							dispatch({ type: "history-open-changed", open })
						}
						onMessageChange={(nextMessage) =>
							dispatch({
								type: "message-changed",
								message: nextMessage,
							})
						}
						onModelChange={(model) =>
							dispatch({ type: "model-changed", model })
						}
						onModelOpenChange={(open) =>
							dispatch({ type: "model-open-changed", open })
						}
						onOpenAgentBuilder={openAgentBuilder}
						onResponseModeChange={(mode) =>
							dispatch({ type: "response-mode-changed", mode })
						}
						onSettingsChange={(nextSettings) =>
							dispatch({
								type: "settings-changed",
								settings: nextSettings,
							})
						}
						onSettingsOpenChange={(open) =>
							dispatch({ type: "settings-open-changed", open })
						}
						onSubmit={submitMessage}
						onSuggestionsOpenChange={(open) =>
							dispatch({ type: "suggestions-open-changed", open })
						}
						onVoiceModeChange={(active) =>
							dispatch({ type: "voice-mode-changed", active })
						}
					/>
				) : activeTab === "Søk" ? (
					<SearchPanel
						expanded={searchModeExpanded}
						onExpandedChange={setSearchExpanded}
						initialSnapshot={searchPanelSnapshot}
						onPreviewActiveChange={setSearchPreviewActive}
						onSnapshotChange={setSearchPanelSnapshot}
					/>
				) : (
					<KnowledgePanel />
				)}
			</div>
		</div>
	);

	return (
		<div className="velion-dashboard-surface relative h-full overflow-hidden bg-transparent text-[#1A1A1A] transition-colors dark:text-[#F7F8F8]">
			<div
				className="pointer-events-none absolute inset-0 dashboard-home-grid"
				aria-hidden="true"
			/>

			<div
				className={cn(
					"velion-home-stage relative flex h-full min-h-0 flex-col overflow-hidden",
					isLaunchingChat ? "velion-home-launching" : "",
				)}
			>
				<DashboardTabs
					activeTab={activeTab}
					onTabChange={changeActiveTab}
				/>

				{searchModeExpanded ? (
					<section className="velion-home-composer-section min-h-0 flex-1 overflow-hidden px-4 pb-4 pt-14 transition-[padding] duration-500">
						{composerContent}
					</section>
				) : (
					<div
						className={cn(
							"velion-home-bands min-h-0 flex-1",
							searchModePreviewing
								? "velion-home-bands-previewing"
								: "",
						)}
					>
						<section
							className={cn(
								"velion-home-header velion-home-band velion-home-band-top velion-fade-up px-4",
								searchModePreviewing
									? "items-start pt-8 pb-3"
									: "",
							)}
						>
							<div className="mx-auto flex w-full max-w-5xl flex-col items-center">
								<div
									className={cn(
										"velion-home-plan w-full max-w-[720px]",
										searchModePreviewing
											? "opacity-55"
											: "",
									)}
								>
									<PlanBadge planLabel={planLabel} />
								</div>

								<h1
									suppressHydrationWarning
									className={cn(
										"velion-home-title w-full max-w-[720px] font-[450] leading-none tracking-tight text-[#1A1A1A] transition-all duration-300 dark:text-[#F7F8F8]",
										searchModePreviewing
											? "origin-top scale-[0.78] translate-y-[-12px] opacity-50"
											: "",
									)}
								>
									{homeTitle}
								</h1>
							</div>
						</section>

						<section className="velion-home-composer-section velion-home-band velion-home-band-middle px-4 transition-[padding] duration-500">
							{composerContent}
						</section>

						<section
							className={cn(
								"velion-home-cards velion-home-band velion-home-band-bottom mx-auto flex min-h-0 w-full max-w-5xl flex-col justify-start px-4 transition-opacity duration-200",
								searchModePreviewing
									? "pointer-events-none opacity-0"
									: "",
							)}
						>
							<div
								key={cardPage}
								className="velion-home-card-grid velion-card-page grid grid-cols-1 gap-4 lg:grid-cols-3"
							>
								{visibleCards.map((card) => (
									<DashboardImageCard
										key={card.id}
										card={card}
										onPrompt={applyCardPrompt}
									/>
								))}
							</div>

							<button
								type="button"
								onClick={() =>
									dispatch({
										type: "card-page-next",
										pageCount,
									})
								}
								className="velion-home-next mx-auto flex size-12 items-center justify-center transition-transform duration-300 hover:scale-105 active:scale-95"
								aria-label="Vis neste kortside"
								title="Vis neste kortside"
							>
								<span
									className="relative block h-5 w-8"
									aria-hidden="true"
								>
									<span className="absolute -left-1 top-1/2 h-[3px] w-[25px] -translate-y-1/2 rotate-45 rounded-full bg-[#171D18]" />
									<span className="absolute -right-1 top-2/5 h-[3px] w-[22px] -translate-y-1/2 -rotate-45 rounded-full bg-[#171D18]" />
								</span>
							</button>
						</section>
					</div>
				)}
			</div>
		</div>
	);
}

function DashboardTabs({
	activeTab,
	onTabChange,
}: {
	activeTab: DashboardTab;
	onTabChange: (tab: DashboardTab) => void;
}) {
	return (
		<div className="velion-home-tabs sticky top-0 z-10 px-4">
			<div className="mx-auto flex w-full max-w-5xl justify-center">
				<div className="inline-flex items-center gap-0.5 rounded-full bg-black/6 p-1 dark:bg-white/10">
					{tabs.map((tab) => (
						<button
							key={tab}
							type="button"
							onClick={() => onTabChange(tab)}
							title={`Vis ${tab.toLowerCase()}`}
							className={cn(
								"rounded-full px-5 py-1.5 text-[13px] font-medium transition-all duration-150",
								activeTab === tab
									? "bg-white text-[#1A1A1A] shadow-sm dark:bg-[#23252A] dark:text-white"
									: "text-[#6B6560] hover:text-[#1A1A1A] dark:text-[#AEB4C0] dark:hover:text-white",
							)}
							aria-pressed={activeTab === tab}
						>
							{tab}
						</button>
					))}
				</div>
			</div>
		</div>
	);
}

function PlanBadge({ planLabel }: { planLabel: string }) {
	return (
		<span className="inline-flex items-center gap-1.5 rounded-full border border-[#E5E0D8] bg-white px-3.5 py-1 text-[12.5px] font-medium text-[#6B6560] transition-colors dark:border-[#2A2C31] dark:bg-[#17181C] dark:text-[#AEB4C0]">
			{planLabel} Plan
			<span className="text-[#D4C9BF]">·</span>
			<button
				type="button"
				className="font-semibold text-[#E8853D] hover:underline"
				title="Oppgrader plan"
			>
				Upgrade
			</button>
		</span>
	);
}

const LIVE_SEARCH_DEBOUNCE_MS = 650;

type SearchPanelSnapshot = {
	searchState: SearchPanelState;
	followUpQuery: string;
	activeResultTab: SearchResultTab;
	images: ImageHit[];
	imagesError: string | null;
	imagesQuery: string;
	imagesStatus: ImagesStatus;
};

type SearchPanelProps = {
	expanded: boolean;
	onExpandedChange: (expanded: boolean) => void;
	initialSnapshot?: SearchPanelSnapshot | null;
	onPreviewActiveChange?: (active: boolean) => void;
	onSnapshotChange?: (snapshot: SearchPanelSnapshot) => void;
};

type SearchResultTab = "Info" | "Videos" | "Map" | "Images" | "Shopping";
type ImagesStatus = "idle" | "loading" | "loaded" | "error";

export function SearchPanel({
	expanded,
	onExpandedChange,
	initialSnapshot,
	onPreviewActiveChange,
	onSnapshotChange,
}: SearchPanelProps) {
	const [searchState, dispatchSearch] = useReducer(
		searchPanelReducer,
		initialSnapshot?.searchState ?? initialSearchPanelState,
	);
	const [followUpQuery, setFollowUpQuery] = useState(
		initialSnapshot?.followUpQuery ?? "",
	);
	const [activeResultTab, setActiveResultTab] = useState<SearchResultTab>(
		initialSnapshot?.activeResultTab ?? "Info",
	);
	const [images, setImages] = useState<ImageHit[]>(
		initialSnapshot?.images ?? [],
	);
	const [imagesError, setImagesError] = useState<string | null>(
		initialSnapshot?.imagesError ?? null,
	);
	const [imagesQuery, setImagesQuery] = useState(
		initialSnapshot?.imagesQuery ?? "",
	);
	const [imagesStatus, setImagesStatus] = useState<ImagesStatus>(
		initialSnapshot?.imagesStatus ?? "idle",
	);
	const searchAbortRef = useRef<AbortController | null>(null);
	const imagesAbortRef = useRef<AbortController | null>(null);
	const restoredPendingSearchRef = useRef(
		initialSnapshot?.searchState.webLoading &&
			initialSnapshot.searchState.submittedQuery
			? initialSnapshot.searchState.submittedQuery
			: null,
	);
	const {
		query,
		suggestions,
		suggestionsQuery,
		highlightedIndex,
		previewResults,
		previewLoading,
		webAnswer,
		webCitations,
		webError,
		webFetchedPage,
		webLoading,
		webMode,
		webResults,
	} = searchState;
	const activeQuery = query.trim();
	const deferredActiveQuery = useDeferredValue(activeQuery);
	const isTyping = activeQuery.length > 0;
	const visibleSuggestions =
		isTyping && suggestionsQuery === activeQuery ? suggestions : [];
	const showPreview = previewResults.length > 0 && isTyping && !expanded;
	const previewResultsVisible = previewResults.slice(0, 6);
	const previewPanelActive =
		!expanded &&
		activeQuery.length >= 3 &&
		(previewLoading || previewResultsVisible.length > 0);
	const expandedResults: WebSearchResult[] =
		webMode === "fetch" && webFetchedPage
			? [
					{
						url: webFetchedPage.url,
						title: webFetchedPage.title,
						snippet:
							webFetchedPage.description ??
							webFetchedPage.excerpt ??
							undefined,
					},
				]
			: webResults;
	const sourceItems = Array.from(
		[...webCitations, ...expandedResults].reduce((map, item) => {
			if (!item.url || map.has(item.url)) return map;
			map.set(item.url, {
				url: item.url,
				title: item.title ?? safeHostname(item.url),
				hostname: safeHostname(item.url),
			});
			return map;
		}, new Map<string, { url: string; title: string; hostname: string }>()),
	).map(([, value]) => value);
	const resultTabs: SearchResultTab[] = [
		"Info",
		"Videos",
		"Map",
		"Images",
		"Shopping",
	];

	// Unique IDs for ARIA
	const listboxId = "dashboard-search-listbox";
	const getOptionId = (index: number) => `dashboard-search-option-${index}`;

	useEffect(() => {
		return () => {
			searchAbortRef.current?.abort();
			imagesAbortRef.current?.abort();
		};
	}, []);

	useEffect(() => {
		onSnapshotChange?.({
			searchState,
			followUpQuery,
			activeResultTab,
			images,
			imagesError,
			imagesQuery,
			imagesStatus,
		});
	}, [
		activeResultTab,
		followUpQuery,
		images,
		imagesError,
		imagesQuery,
		imagesStatus,
		onSnapshotChange,
		searchState,
	]);

	useEffect(() => {
		onPreviewActiveChange?.(previewPanelActive);
	}, [onPreviewActiveChange, previewPanelActive]);

	// --- Suggestions fetch (120 ms debounce, existing) ---
		useEffect(() => {
			if (!isTyping || activeQuery.length < 2) {
				return;
			}
			const cachedSuggestions = readCachedValue(
				dashboardSearchSuggestionCache,
				activeQuery,
			);
			if (cachedSuggestions) {
				dispatchSearch({
					type: "suggestions-loaded",
					query: activeQuery,
					suggestions: cachedSuggestions,
				});
				return;
			}

			const controller = new AbortController();
			const timeout = window.setTimeout(() => {
				apiGet<{ suggestions?: SearchSuggestion[] }>(
				`/api/v1/search/suggestions?q=${encodeURIComponent(activeQuery)}&scope=queries&limit=6`,
				{ credentials: "include", signal: controller.signal },
			)
					.then((payload) => {
						if (!payload.suggestions) {
							dispatchSearch({ type: "suggestions-cleared" });
							return;
						}
						const nextSuggestions = payload.suggestions.filter(
							(s) =>
								typeof s.text === "string" &&
								s.text.trim().length > 0,
						);
						writeCachedValue(
							dashboardSearchSuggestionCache,
							activeQuery,
							nextSuggestions,
							DASHBOARD_SEARCH_SUGGESTION_CACHE_TTL_MS,
						);
						dispatchSearch({
							type: "suggestions-loaded",
							query: activeQuery,
							suggestions: nextSuggestions,
						});
					})
				.catch((error: unknown) => {
					if (
						error instanceof DOMException &&
						error.name === "AbortError"
					)
						return;
					dispatchSearch({ type: "suggestions-cleared" });
				});
		}, 120);

		return () => {
			controller.abort();
			window.clearTimeout(timeout);
		};
	}, [activeQuery, isTyping]);

	const runSearch = useCallback((
		q: string,
		options?: {
			includeAnswer?: boolean;
			keepExisting?: boolean;
			limit?: number;
		},
	) => {
		const trimmed = q.trim();
		if (trimmed.length < 3) {
			dispatchSearch({ type: "preview-cleared" });
			dispatchSearch({ type: "web-cleared" });
			return;
		}
		const limit = options?.limit ?? 10;
		const includeAnswer = options?.includeAnswer ?? false;
		const keepExisting = options?.keepExisting ?? false;

		searchAbortRef.current?.abort();
		const controller = new AbortController();
		searchAbortRef.current = controller;

		dispatchSearch({ type: "submitted", query: trimmed });
		if (keepExisting) {
			dispatchSearch({ type: "preview-cleared" });
		} else {
			dispatchSearch({ type: "preview-loading", query: trimmed });
		}
		dispatchSearch({ type: "web-search-started", keepExisting });
		setImages([]);
		setImagesError(null);
		setImagesQuery("");
		setImagesStatus("idle");

		apiSend<WebSearchPayload>(
			"/api/v1/search/web",
			{ query: trimmed, limit, includeAnswer },
			"POST",
			{ credentials: "include", signal: controller.signal },
		)
			.then((payload) => {
				if (payload.mode === "fetch" || payload.url) {
					if (!payload.url) {
						dispatchSearch({ type: "preview-cleared" });
						dispatchSearch({
							type: "web-search-error",
							message:
								"Velion fant ikke en gyldig side for søket.",
						});
						return;
					}

					const preview = buildPreviewResults(payload);
					dispatchSearch({
						type: "preview-loaded",
						query: trimmed,
						results: preview,
					});
					dispatchSearch({
						type: "web-fetch-loaded",
						page: {
							url: payload.url,
							title: payload.title ?? payload.url,
							description: payload.description ?? null,
							excerpt: payload.excerpt ?? null,
						},
					});
					return;
				}

				const results = Array.isArray(payload.results)
					? payload.results
							.filter(
								(result) =>
									typeof result.url === "string" &&
									result.url.trim().length > 0,
							)
							.slice(0, 8)
					: [];
				const preview = buildPreviewResults(payload);
				const citations = Array.isArray(payload.citations)
					? payload.citations.filter(
							(citation) =>
								typeof citation.url === "string" &&
								citation.url.trim().length > 0,
						)
					: [];

				if (preview.length > 0) {
					dispatchSearch({
						type: "preview-loaded",
						query: trimmed,
						results: preview,
					});
				} else {
					dispatchSearch({ type: "preview-cleared" });
				}
				dispatchSearch({
					type: "web-results-loaded",
					results,
					answer:
						payload.answer?.trim() ||
						(results.length
							? `Velion fant ${results.length} relevante treff for "${trimmed}".`
							: `Velion fant ingen sikre treff for "${trimmed}" akkurat nå.`),
					citations,
				});
			})
			.catch((error: unknown) => {
				if (
					error instanceof DOMException &&
					error.name === "AbortError"
				)
					return;
				dispatchSearch({ type: "preview-cleared" });
				dispatchSearch({
					type: "web-search-error",
					message:
						error instanceof Error
							? error.message
							: "Søket kunne ikke fullføres.",
				});
			});
	}, []);

	const runPreviewSearch = useCallback((q: string) => {
		const trimmed = q.trim();
		if (trimmed.length < 3) {
			dispatchSearch({ type: "preview-cleared" });
			return;
		}

		searchAbortRef.current?.abort();
		const controller = new AbortController();
		searchAbortRef.current = controller;

		dispatchSearch({ type: "preview-loading", query: trimmed });

		apiSend<WebSearchPayload>(
			"/api/v1/search/web",
			{ query: trimmed, limit: 6, includeAnswer: false },
			"POST",
			{ credentials: "include", signal: controller.signal },
		)
			.then((payload) => {
				const preview = buildPreviewResults(payload);
				if (preview.length > 0) {
					dispatchSearch({
						type: "preview-loaded",
						query: trimmed,
						results: preview,
					});
					return;
				}

				dispatchSearch({ type: "preview-cleared" });
			})
			.catch((error: unknown) => {
				if (
					error instanceof DOMException &&
					error.name === "AbortError"
				)
					return;
				dispatchSearch({ type: "preview-cleared" });
			});
	}, []);

	useEffect(() => {
		const restoredQuery = restoredPendingSearchRef.current;
		if (!restoredQuery) return;
		runSearch(restoredQuery, {
			includeAnswer: false,
			keepExisting: true,
			limit: 10,
		});
	}, [runSearch]);

	const persistExpandedSearchSnapshot = useCallback(
		(nextQuery: string) => {
			const trimmed = nextQuery.trim();
			if (trimmed.length < 3) return;
			const previewForQuery =
				searchState.previewQuery === trimmed
					? searchState.previewResults.slice(0, 6)
					: [];
			const existingResults = previewForQuery.map((result) => ({
				url: result.url,
				title: result.title,
				snippet: result.snippet ?? undefined,
			}));

			const nextSearchState: SearchPanelState = {
				...searchState,
				query: trimmed,
				submittedQuery: trimmed,
				suggestions: [],
				suggestionsQuery: "",
				highlightedIndex: -1,
				previewResults: previewForQuery,
				previewQuery: previewForQuery.length ? trimmed : "",
				previewLoading: false,
				webMode: existingResults.length ? "search" : null,
				webResults: existingResults,
				webAnswer: "",
				webCitations: [],
				webFetchedPage: null,
				webLoading: true,
				webError: null,
			};

			onSnapshotChange?.({
				searchState: nextSearchState,
				followUpQuery,
				activeResultTab,
				images: [],
				imagesError: null,
				imagesQuery: "",
				imagesStatus: "idle",
			});
		},
		[
			activeResultTab,
			followUpQuery,
			onSnapshotChange,
			searchState,
		],
	);

	const expandFromPreview = useCallback(
		(nextQuery: string) => {
			const trimmed = nextQuery.trim();
			if (trimmed.length < 3) return;
			persistExpandedSearchSnapshot(trimmed);
			dispatchSearch({ type: "suggestions-cleared" });
			onExpandedChange(true);
		},
		[onExpandedChange, persistExpandedSearchSnapshot],
	);

		const fetchImages = useCallback((q: string) => {
			const trimmed = q.trim();
			if (!trimmed) return;
			const cachedImages = readCachedValue(
				dashboardSearchImageCache,
				trimmed,
			);
			if (cachedImages) {
				setImages(cachedImages);
				setImagesError(null);
				setImagesQuery(trimmed);
				setImagesStatus("loaded");
				return;
			}

			imagesAbortRef.current?.abort();
			const controller = new AbortController();
		imagesAbortRef.current = controller;

		setImages([]);
		setImagesError(null);
		setImagesQuery(trimmed);
		setImagesStatus("loading");

		void (async () => {
			try {
				const response = await fetch("/api/v1/search/images", {
					method: "POST",
					credentials: "include",
					headers: { "Content-Type": "application/json" },
					body: JSON.stringify({ query: trimmed, limit: 18 }),
					signal: controller.signal,
				});

				const payload = (await response.json().catch(() => null)) as {
					data?: { images?: ImageHit[] };
					error?: { message?: string };
				} | null;

					if (!response.ok || !payload?.data) {
					setImagesError(
						payload?.error?.message ??
							"Image search could not be completed.",
					);
					setImagesStatus("error");
					return;
					}

					const nextImages = Array.isArray(payload.data.images)
						? payload.data.images
						: [];
					writeCachedValue(
						dashboardSearchImageCache,
						trimmed,
						nextImages,
						DASHBOARD_SEARCH_IMAGE_CACHE_TTL_MS,
					);
					setImages(
						nextImages,
					);
					setImagesStatus("loaded");
			} catch (error: unknown) {
				if (
					error instanceof DOMException &&
					error.name === "AbortError"
				)
					return;
				setImagesError("Image search could not be completed.");
				setImagesStatus("error");
			}
		})();
	}, []);

	// --- Live result fetch: waits until the user pauses, then aborts stale requests. ---
	useEffect(() => {
		if (!isTyping) {
			searchAbortRef.current?.abort();
			dispatchSearch({ type: "preview-cleared" });
			dispatchSearch({ type: "web-cleared" });
			onExpandedChange(false);
			restoredPendingSearchRef.current = null;
			return;
		}

		if (deferredActiveQuery.length < 3) {
			searchAbortRef.current?.abort();
			dispatchSearch({ type: "preview-cleared" });
			dispatchSearch({ type: "web-cleared" });
			return;
		}

		if (restoredPendingSearchRef.current === deferredActiveQuery) {
			restoredPendingSearchRef.current = null;
			return;
		}

		if (expanded) {
			return;
		}

		const timeout = window.setTimeout(() => {
			dispatchSearch({ type: "suggestions-cleared" });
			runPreviewSearch(deferredActiveQuery);
		}, LIVE_SEARCH_DEBOUNCE_MS);

		return () => {
			window.clearTimeout(timeout);
			searchAbortRef.current?.abort();
		};
	}, [
		deferredActiveQuery,
		expanded,
		isTyping,
		onExpandedChange,
		runPreviewSearch,
	]);

	useEffect(() => {
		if (!expanded || activeQuery.length < 3) return;
		if (imagesStatus === "idle" || imagesQuery !== activeQuery) {
			const timeout = window.setTimeout(() => {
				fetchImages(activeQuery);
			}, 0);
			return () => window.clearTimeout(timeout);
		}
	}, [activeQuery, expanded, fetchImages, imagesQuery, imagesStatus]);

	const submit = (event: React.FormEvent) => {
		event.preventDefault();
		const highlighted =
			highlightedIndex >= 0 ? visibleSuggestions[highlightedIndex] : null;
		const nextQuery = highlighted?.text ?? query;
		const trimmed = nextQuery.trim();
		if (trimmed.length >= 3 && !expanded) {
			expandFromPreview(trimmed);
			return;
		}
		runSearch(nextQuery, { includeAnswer: false, limit: 10 });
	};

	const submitFollowUp = (event: React.FormEvent) => {
		event.preventDefault();
		const nextQuery = followUpQuery.trim();
		if (!nextQuery) return;
		dispatchSearch({ type: "query-changed", query: nextQuery });
		setFollowUpQuery("");
		onExpandedChange(true);
		runSearch(nextQuery, { includeAnswer: false, limit: 10 });
	};

	const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
		if (!visibleSuggestions.length) return;

		if (event.key === "ArrowDown") {
			event.preventDefault();
			dispatchSearch({
				type: "highlight-changed",
				index: (highlightedIndex + 1) % visibleSuggestions.length,
			});
		} else if (event.key === "ArrowUp") {
			event.preventDefault();
			const next =
				highlightedIndex <= 0
					? visibleSuggestions.length - 1
					: highlightedIndex - 1;
			dispatchSearch({ type: "highlight-changed", index: next });
		} else if (event.key === "Escape") {
			event.preventDefault();
			dispatchSearch({ type: "suggestions-cleared" });
			dispatchSearch({ type: "preview-cleared" });
			if (expanded) {
				onExpandedChange(false);
			}
		} else if (event.key === "Enter" && highlightedIndex >= 0) {
			event.preventDefault();
			const suggestion = visibleSuggestions[highlightedIndex];
			if (suggestion) {
				dispatchSearch({ type: "suggestions-cleared" });
				if (!expanded) {
					expandFromPreview(suggestion.text);
					return;
				}
				runSearch(suggestion.text, {
					includeAnswer: false,
					limit: 10,
				});
			}
		}
	};

	const isDropdownOpen = visibleSuggestions.length > 0;

	if (expanded) {
		return (
			<div className="velion-panel-in velion-search-expanded-shell relative flex h-full min-h-[520px] w-full flex-col overflow-hidden bg-transparent transition-all duration-500 ease-out">
				<div className="shrink-0 px-5 pb-3 pt-4">
					<form onSubmit={submit}>
						<label
							className="sr-only"
							htmlFor="dashboard-expanded-search"
						>
							Endre søk
						</label>
						<div className="flex min-w-0 items-center gap-2">
							<Search
								className="size-4 shrink-0 text-[#9A9188]"
								aria-hidden="true"
							/>
							<input
								id="dashboard-expanded-search"
								value={query}
								onChange={(event) => {
									dispatchSearch({
										type: "query-changed",
										query: event.target.value,
									});
								}}
								className="min-w-0 flex-1 bg-transparent text-[19px] font-semibold tracking-[-0.02em] text-[#24262D] placeholder:text-[#AAA198] focus:outline-none dark:text-white dark:placeholder:text-[#737780]"
								placeholder="Skriv et nytt søk..."
								autoComplete="off"
							/>
							{webLoading ? (
								<Loader2
									className="size-4 shrink-0 animate-spin text-[#9A9188]"
									aria-hidden="true"
								/>
							) : null}
						</div>
					</form>

					<div className="mt-3 flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
						<div className="flex min-w-0 items-center gap-1 overflow-x-auto rounded-full bg-black/[0.04] p-1 dark:bg-white/[0.08]">
							{resultTabs.map((tab) => (
								<button
									key={tab}
									type="button"
									onClick={() => setActiveResultTab(tab)}
									className={cn(
										"h-8 rounded-full px-3 text-[12px] font-medium transition",
										activeResultTab === tab
											? "bg-white text-[#1A1A1A] shadow-sm dark:bg-[#22242A] dark:text-white"
											: "text-[#756F68] hover:text-[#1A1A1A] dark:text-[#AEB4C0] dark:hover:text-white",
									)}
									aria-pressed={activeResultTab === tab}
								>
									{tab}
								</button>
							))}
						</div>

						<div className="flex items-center gap-2">
							<button
								type="button"
								onClick={() => setActiveResultTab("Info")}
								className="rounded-full bg-white/58 px-3 py-1.5 text-[12px] font-medium text-[#6F6860] transition hover:bg-white dark:bg-white/[0.06] dark:text-[#AEB4C0] dark:hover:bg-white/[0.1]"
							>
								Filter
							</button>
							<button
								type="button"
								onClick={() => setActiveResultTab("Info")}
								className="rounded-full bg-white/58 px-3 py-1.5 text-[12px] font-medium text-[#6F6860] transition hover:bg-white dark:bg-white/[0.06] dark:text-[#AEB4C0] dark:hover:bg-white/[0.1]"
							>
								Sources{" "}
								{sourceItems.length ? sourceItems.length : ""}
							</button>
							<button
								type="button"
								onClick={() => onExpandedChange(false)}
								className="rounded-full bg-white/58 px-3 py-1.5 text-[12px] font-medium text-[#6F6860] transition hover:bg-white dark:bg-white/[0.06] dark:text-[#AEB4C0] dark:hover:bg-white/[0.1]"
							>
								Kompakt
							</button>
						</div>
					</div>
				</div>

				<div className="grid min-h-0 flex-1 grid-cols-1 overflow-hidden xl:grid-cols-[minmax(0,1fr)_340px]">
					<div className="min-h-0 overflow-y-auto p-4 lg:p-5">
						{webLoading && expandedResults.length === 0 ? (
							<div className="flex items-center gap-2 rounded-[18px] bg-white/58 px-4 py-3 text-[13px] font-medium text-[#6F6860] dark:bg-white/[0.04] dark:text-[#AEB4C0]">
								<Loader2
									className="size-4 animate-spin"
									aria-hidden="true"
								/>
								Thinking...
							</div>
						) : webError && expandedResults.length === 0 ? (
							<div className="rounded-[18px] bg-[#FFF7F5] p-4 text-[13px] font-medium text-[#A53E2E] dark:bg-[#281817] dark:text-[#FFB8AE]">
								{webError}
							</div>
						) : activeResultTab === "Images" ? (
							<ImageResultsPanel
								images={images}
								imagesError={imagesError}
								imagesStatus={imagesStatus}
								query={activeQuery}
							/>
						) : activeResultTab === "Videos" ? (
							<VideoResultsPanel
								images={images}
								query={activeQuery}
								results={expandedResults}
							/>
						) : activeResultTab === "Map" ? (
							<MapGuidePanel
								query={activeQuery}
								results={expandedResults}
							/>
						) : activeResultTab === "Shopping" ? (
							<ShoppingResultsPanel
								query={activeQuery}
								results={expandedResults}
							/>
						) : (
							<div className="space-y-3">
								{webLoading ? (
									<div className="flex items-center gap-2 rounded-[18px] bg-white/58 px-4 py-3 text-[13px] font-medium text-[#6F6860] dark:bg-white/[0.04] dark:text-[#AEB4C0]">
										<Loader2
											className="size-4 animate-spin"
											aria-hidden="true"
										/>
										Laster flere treff…
									</div>
								) : null}

								{webError ? (
									<div className="rounded-[18px] bg-[#FFF7F5] p-4 text-[13px] font-medium text-[#A53E2E] dark:bg-[#281817] dark:text-[#FFB8AE]">
										{webError}
									</div>
								) : null}

								{activeResultTab === "Info" && webAnswer ? (
									<section className="rounded-[22px] bg-white/58 p-4 dark:bg-white/[0.04]">
										<p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#9A9188] dark:text-[#737780]">
											Velion summary
										</p>
										<p className="mt-2 text-[14px] leading-relaxed text-[#2E3137] dark:text-[#E9EAED]">
											{webAnswer}
										</p>
									</section>
								) : null}

								{expandedResults.length > 0 ? (
									expandedResults.map((result, index) => (
										<SearchResultCard
											key={result.url}
											image={
												images[
													index %
														Math.max(
															images.length,
															1,
														)
												]
											}
											result={result}
											index={index}
										/>
									))
								) : (
									<div className="rounded-[18px] bg-white/58 p-4 text-[13px] text-[#756F68] dark:bg-white/[0.04] dark:text-[#AEB4C0]">
										Endre søket over, eller skriv en
										oppfølging under.
									</div>
								)}
							</div>
						)}
					</div>

					<aside className="hidden min-h-0 p-4 xl:block">
						<SearchInsightRail
							activeTab={activeResultTab}
							answer={webAnswer}
							images={images}
							query={activeQuery}
							sourceItems={sourceItems.slice(0, 5)}
						/>
					</aside>
				</div>

				<form onSubmit={submitFollowUp} className="shrink-0 p-3">
					<label
						className="sr-only"
						htmlFor="dashboard-search-follow-up"
					>
						Ask follow up
					</label>
					<div className="flex items-center gap-2 rounded-full bg-white/86 px-3 py-2 shadow-[0_10px_34px_rgba(20,21,24,0.06)] dark:bg-[#191B20]/92">
						<CirclePlus
							className="size-4 shrink-0 text-[#9A9188]"
							aria-hidden="true"
						/>
						<input
							id="dashboard-search-follow-up"
							value={followUpQuery}
							onChange={(event) =>
								setFollowUpQuery(event.target.value)
							}
							className="h-9 min-w-0 flex-1 bg-transparent text-[14px] font-medium text-[#24262D] placeholder:text-[#AAA198] focus:outline-none dark:text-white dark:placeholder:text-[#737780]"
							placeholder="Ask follow up..."
							autoComplete="off"
						/>
						<button
							type="submit"
							disabled={!followUpQuery.trim()}
							className="grid size-9 shrink-0 place-items-center rounded-full bg-[#111111] text-white transition hover:bg-[#2A2A2A] disabled:bg-[#E9E4DD] disabled:text-[#A99E93] dark:bg-white dark:text-[#111111] dark:disabled:bg-[#2A2C31] dark:disabled:text-[#737780]"
							title="Send follow-up"
							aria-label="Send follow-up"
						>
							<ArrowRight className="size-4" />
						</button>
					</div>
				</form>
			</div>
		);
	}

	return (
		<div className="velion-panel-in relative transition-all duration-500 ease-out">
			<form onSubmit={submit} className="relative z-[1] w-full">
				<label className="sr-only" htmlFor="dashboard-search">
					Søk i selskapets kunnskap
				</label>
				<div className="flex items-center gap-2">
					<TopLayerTooltip label="Add context" placement="top">
						<button
							type="button"
							aria-label="Add search context"
							className="velion-glass-input grid size-11 shrink-0 place-items-center rounded-full text-[#34363D] transition hover:shadow-[0_16px_36px_rgba(76,60,92,0.16)] dark:text-white"
						>
							<CirclePlus className="size-4" />
						</button>
					</TopLayerTooltip>
					<div className="velion-glass-input flex h-12 min-w-0 flex-1 items-center rounded-full px-4">
						<Search className="mr-2 size-4 shrink-0 text-[#9A9188]" />
						<input
							id="dashboard-search"
							role="combobox"
							aria-label="Søk i selskapets kunnskap"
							aria-expanded={isDropdownOpen}
							aria-controls={
								isDropdownOpen ? listboxId : undefined
							}
							aria-activedescendant={
								highlightedIndex >= 0
									? getOptionId(highlightedIndex)
									: undefined
							}
							aria-autocomplete="list"
							autoComplete="off"
							value={query}
							onChange={(event) => {
								dispatchSearch({
									type: "query-changed",
									query: event.target.value,
								});
							}}
							onKeyDown={handleKeyDown}
							onBlur={() => {
								// Slight delay so click on suggestions can fire first
								window.setTimeout(() => {
									dispatchSearch({
										type: "suggestions-cleared",
									});
									dispatchSearch({ type: "preview-cleared" });
								}, 150);
							}}
							className="h-full min-w-0 flex-1 bg-transparent text-[14px] font-medium text-[#24262D] placeholder:text-[#AAA198] focus:outline-none dark:text-white dark:placeholder:text-[#737780]"
							placeholder="Ask anything…"
						/>
						<button
							type="submit"
							aria-label="Søk"
							disabled={!query.trim()}
							className="grid size-9 shrink-0 place-items-center rounded-full bg-[#111111] text-white transition hover:bg-[#2A2A2A] disabled:bg-[#E9E4DD] disabled:text-[#A99E93] dark:bg-white dark:text-[#111111] dark:disabled:bg-[#2A2C31] dark:disabled:text-[#737780]"
							title="Søk"
						>
							<ArrowRight className="size-4" />
						</button>
					</div>
				</div>

				{/* Typeahead suggestions dropdown */}
				{isDropdownOpen ? (
					<div
						id={listboxId}
						role="listbox"
						aria-label="Søkeforslag"
						className="velion-glass velion-fade-up ml-[52px] mt-2 overflow-hidden rounded-[22px] p-2"
					>
						<p className="px-3 pb-1.5 pt-1 text-[12px] font-semibold text-[#504A43] dark:text-[#D4D6DC]">
							Forslag
						</p>
						{visibleSuggestions.map((suggestion, idx) => {
							const isHighlighted = idx === highlightedIndex;
							const hasLabel =
								suggestion.collection || suggestion.source;
							return (
								<button
									key={`${suggestion.collection}:${suggestion.object}`}
									id={getOptionId(idx)}
									role="option"
									aria-selected={isHighlighted}
									type="button"
									onMouseDown={(e) => {
										// Prevent blur from firing before click
										e.preventDefault();
										dispatchSearch({
											type: "suggestions-cleared",
										});
										if (!expanded) {
											expandFromPreview(
												suggestion.text,
											);
											return;
										}
										runSearch(suggestion.text, {
											includeAnswer: false,
											limit: 10,
										});
									}}
									onMouseEnter={() =>
										dispatchSearch({
											type: "highlight-changed",
											index: idx,
										})
									}
									className={cn(
										"flex w-full items-center gap-3 rounded-[12px] px-3 py-2.5 text-left text-[13px] font-medium text-[#2E3137] transition dark:text-white",
										isHighlighted
											? "bg-[#F4F1EB] dark:bg-white/10"
											: "hover:bg-[#F4F1EB] dark:hover:bg-white/10",
									)}
								>
									<Search
										className="size-4 shrink-0 text-[#7E776F]"
										aria-hidden="true"
									/>
									<span className="min-w-0 flex-1 truncate">
										{suggestion.text}
									</span>
									{hasLabel ? (
										<span className="shrink-0 text-[11px] font-normal text-[#AAA198] dark:text-[#5A5E66]">
											{suggestion.collection ||
												suggestion.source}
										</span>
									) : null}
								</button>
							);
						})}
					</div>
				) : null}

				{/* Inline preview cards */}
				{showPreview ? (
					<div className="velion-fade-up ml-[52px] mt-3">
						<div className="rounded-[24px] border border-black/[0.06] bg-white px-3 py-3 shadow-[0_16px_42px_rgba(20,21,24,0.08)] dark:border-white/[0.07] dark:bg-[#1A1D24] dark:shadow-[0_18px_42px_rgba(0,0,0,0.36)]">
							<div className="max-h-[274px] space-y-2 overflow-y-auto overscroll-contain pr-1">
								{previewResultsVisible.map((result) => (
									<a
										key={result.url}
										href={result.url}
										target="_blank"
										rel="noopener noreferrer"
										className="flex min-h-[86px] min-w-0 flex-col gap-0.5 rounded-[18px] border border-black/[0.05] bg-[#F5F4F1] px-4 py-3 transition hover:bg-[#EFEBE5] dark:border-white/[0.06] dark:bg-[#20242C] dark:hover:bg-[#252A33]"
									>
										<span className="flex min-w-0 items-center gap-1.5 text-[11px] font-medium text-[#8C837A] dark:text-[#7F8794]">
											<span className="truncate">
												{result.hostname}
											</span>
											<ExternalLink
												className="size-3 shrink-0"
												aria-hidden="true"
											/>
										</span>
										<span className="line-clamp-2 text-[14px] font-semibold leading-snug text-[#17191F] dark:text-white">
											{result.title}
										</span>
										{result.snippet ? (
											<span className="line-clamp-2 text-[12px] leading-relaxed text-[#666059] dark:text-[#A8AFBA]">
												{result.snippet}
											</span>
										) : null}
									</a>
								))}
							</div>
							<div className="mt-3 flex items-center justify-between gap-3 px-1">
								<p className="text-[12px] font-medium text-[#8A8177] dark:text-[#818896]">
									Viser {Math.min(previewResultsVisible.length, 3)} av{" "}
									{previewResultsVisible.length} forhåndstreff.
								</p>
								<button
									type="button"
									onClick={() => expandFromPreview(activeQuery)}
									className="rounded-full bg-[#111111] px-3 py-1.5 text-[12px] font-semibold text-white transition hover:bg-[#2A2A2A] dark:bg-white dark:text-[#111111] dark:hover:bg-[#E7E8EB]"
								>
									Se mer
								</button>
							</div>
						</div>
					</div>
				) : webError && !expanded ? (
					<div className="ml-[52px] mt-3 rounded-[16px] border border-[#F1C9C2] bg-[#FFF7F5] px-4 py-3 text-[12px] font-medium text-[#A53E2E] dark:border-[#6E332D] dark:bg-[#281817] dark:text-[#FFB8AE]">
						{webError}
					</div>
				) : !expanded &&
				  (previewLoading || webLoading) &&
				  activeQuery.length >= 3 ? (
					<div className="ml-[52px] mt-3 flex items-center gap-2 px-1 text-[12px] text-[#AAA198] dark:text-[#5A5E66]">
						<Loader2
							className="size-3.5 animate-spin"
							aria-hidden="true"
						/>
						Søker når du stopper å skrive…
					</div>
				) : null}
			</form>
		</div>
	);
}

function faviconUrlForResult(url: string) {
	const hostname = safeHostname(url);
	if (!hostname || hostname === url) return null;
	return `https://www.google.com/s2/favicons?domain=${encodeURIComponent(hostname)}&sz=64`;
}

function SearchResultCard({
	image,
	result,
	index,
}: {
	image?: ImageHit;
	result: WebSearchResult;
	index: number;
}) {
	const favicon = faviconUrlForResult(result.url);

	return (
		<a
			href={result.url}
			target="_blank"
			rel="noopener noreferrer"
			className="group block rounded-[24px] bg-white/58 p-4 transition hover:-translate-y-0.5 hover:bg-white/82 dark:bg-white/[0.04] dark:hover:bg-white/[0.07]"
		>
			<div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_168px]">
				<div className="flex min-w-0 items-start gap-3">
					<span className="mt-1 grid size-9 shrink-0 place-items-center rounded-full bg-black/[0.04] text-[12px] font-semibold text-[#514B44] dark:bg-white/[0.07] dark:text-[#D4D6DC]">
						{index + 1}
					</span>
					<span className="min-w-0 flex-1">
						<span className="flex min-w-0 items-center gap-1.5 text-[12px] font-medium text-[#9A9188] dark:text-[#737780]">
							<span className="truncate">
								{safeHostname(result.url)}
							</span>
							<ExternalLink
								className="size-3 shrink-0"
								aria-hidden="true"
							/>
						</span>
						<span className="mt-1 block line-clamp-2 text-[17px] font-semibold leading-snug text-[#1A1A1A] dark:text-white">
							{result.title ?? result.url}
						</span>
						<span className="mt-3 block text-[11px] font-semibold uppercase tracking-[0.16em] text-[#A79E96] dark:text-[#737780]">
							AI explanation
						</span>
						<span className="mt-1 block line-clamp-3 text-[13.5px] leading-relaxed text-[#6F6860] dark:text-[#AEB4C0]">
							{result.snippet
								? `Velion sees this as relevant to the query because the page context overlaps with the topic: ${result.snippet}`
								: `Velion matched this source to your search and can inspect it further before using it in an answer.`}
						</span>
						<span className="mt-3 flex flex-wrap gap-1.5">
							{[
								safeHostname(result.url),
								"web page",
								"source",
							].map((label) => (
								<span
									key={label}
									className="rounded-full bg-black/[0.04] px-2.5 py-1 text-[11px] font-medium text-[#756F68] dark:bg-white/[0.06] dark:text-[#AEB4C0]"
								>
									{label}
								</span>
							))}
						</span>
					</span>
				</div>

				<div className="min-w-0">
					<div className="relative aspect-[4/3] overflow-hidden rounded-[20px] bg-black/[0.04] dark:bg-white/[0.06]">
						{image?.thumbnailUrl ? (
							// eslint-disable-next-line @next/next/no-img-element
							<img
								src={image.thumbnailUrl}
								alt={image.title ?? ""}
								loading="lazy"
								decoding="async"
								referrerPolicy="no-referrer"
								className="h-full w-full object-cover transition group-hover:scale-[1.02]"
							/>
						) : favicon ? (
							<div className="grid h-full w-full place-items-center">
								{/* eslint-disable-next-line @next/next/no-img-element */}
								<img
									src={favicon}
									alt=""
									className="size-12 opacity-80"
									loading="lazy"
									decoding="async"
								/>
							</div>
						) : (
							<div className="grid h-full w-full place-items-center text-[28px] font-semibold text-[#9A9188]">
								{safeHostname(result.url)
									.slice(0, 1)
									.toUpperCase()}
							</div>
						)}
						{favicon ? (
							<span className="absolute right-2 top-2 grid size-8 place-items-center rounded-full bg-white/86 shadow-sm dark:bg-[#17181C]/86">
								{/* eslint-disable-next-line @next/next/no-img-element */}
								<img
									src={favicon}
									alt=""
									className="size-4"
									loading="lazy"
									decoding="async"
								/>
							</span>
						) : null}
					</div>
				</div>
			</div>
		</a>
	);
}

function ImageResultsPanel({
	images,
	imagesError,
	imagesStatus,
	query,
}: {
	images: ImageHit[];
	imagesError: string | null;
	imagesStatus: ImagesStatus;
	query: string;
}) {
	if (imagesStatus === "loading" || imagesStatus === "idle") {
		return (
			<div className="[column-fill:_balance] gap-3 [column-count:2] md:[column-count:3]">
				{[38, 30, 44, 34, 42, 28, 36, 40].map((height, index) => (
					<div
						key={index}
						style={{ height: height * 4 }}
						className="mb-3 rounded-[22px] bg-white/48 dark:bg-white/[0.05]"
					/>
				))}
			</div>
		);
	}

	if (imagesStatus === "error") {
		return (
			<div className="rounded-[20px] bg-[#FFF7F5] p-4 text-[13px] text-[#A53E2E] dark:bg-[#281817] dark:text-[#FFB8AE]">
				{imagesError ?? "Image search could not be completed."}
			</div>
		);
	}

	if (images.length === 0) {
		return (
			<div className="rounded-[20px] bg-white/58 p-4 text-[13px] text-[#756F68] dark:bg-white/[0.04] dark:text-[#AEB4C0]">
				No images found for {query}.
			</div>
		);
	}

	return (
		<div className="[column-fill:_balance] gap-3 [column-count:2] md:[column-count:3]">
			{images.map((image, index) => (
				<a
					key={`${image.url}-${index}`}
					href={image.url}
					target="_blank"
					rel="noopener noreferrer"
					className="group mb-3 block break-inside-avoid overflow-hidden rounded-[22px] bg-white/58 transition hover:bg-white/82 dark:bg-white/[0.04] dark:hover:bg-white/[0.07]"
				>
					{/* eslint-disable-next-line @next/next/no-img-element */}
					<img
						src={image.thumbnailUrl}
						alt={image.title ?? ""}
						loading="lazy"
						decoding="async"
						referrerPolicy="no-referrer"
						className="block h-auto w-full object-cover"
					/>
					<div className="px-3 py-2">
						<p className="line-clamp-2 text-[12px] font-medium text-[#2E3137] dark:text-white">
							{image.title ?? "Image result"}
						</p>
						<p className="mt-1 line-clamp-2 text-[11px] leading-relaxed text-[#756F68] dark:text-[#AEB4C0]">
							Velion can use this visual to explain context,
							layout, product details, or place cues.
						</p>
					</div>
				</a>
			))}
		</div>
	);
}

function VideoResultsPanel({
	images,
	query,
	results,
}: {
	images: ImageHit[];
	query: string;
	results: WebSearchResult[];
}) {
	const videoMatches = results.filter(isVideoLikeResult);
	const items = (videoMatches.length > 0 ? videoMatches : results).slice(
		0,
		4,
	);
	if (items.length === 0) {
		return <SearchVerticalEmpty label="video" query={query} />;
	}

	return (
		<div className="space-y-3">
			{items.map((result, index) => (
				<a
					key={result.url}
					href={result.url}
					target="_blank"
					rel="noopener noreferrer"
					className="grid gap-4 rounded-[24px] bg-white/58 p-4 transition hover:bg-white/82 dark:bg-white/[0.04] dark:hover:bg-white/[0.07] md:grid-cols-[220px_minmax(0,1fr)]"
				>
					<div className="relative aspect-video overflow-hidden rounded-[18px] bg-black/[0.04] dark:bg-white/[0.06]">
						{images[index]?.thumbnailUrl ? (
							// eslint-disable-next-line @next/next/no-img-element
							<img
								src={images[index].thumbnailUrl}
								alt=""
								className="h-full w-full object-cover"
								loading="lazy"
								decoding="async"
							/>
						) : null}
						<span className="absolute inset-0 grid place-items-center">
							<span className="grid size-10 place-items-center rounded-full bg-white/80 text-[#1A1A1A] shadow-sm">
								▶
							</span>
						</span>
					</div>
					<div className="min-w-0">
						<p className="text-[12px] font-medium text-[#9A9188]">
							{safeHostname(result.url)}
						</p>
						<h3 className="mt-1 line-clamp-2 text-[17px] font-semibold text-[#1A1A1A] dark:text-white">
							{result.title ?? result.url}
						</h3>
						<p className="mt-3 text-[13px] leading-relaxed text-[#756F68] dark:text-[#AEB4C0]">
							Velion would summarize the clip, identify useful
							moments, and cite it only if the page gives enough
							context.
						</p>
					</div>
				</a>
			))}
		</div>
	);
}

function MapGuidePanel({
	query,
	results,
}: {
	query: string;
	results: WebSearchResult[];
}) {
	const places = results.slice(0, 4);
	return (
		<div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_280px]">
			<div className="min-h-[360px] rounded-[28px] bg-white/50 p-5 dark:bg-white/[0.04]">
				<p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#9A9188]">
					AI map guide
				</p>
				<h3 className="mt-3 text-[24px] font-semibold tracking-[-0.03em] text-[#1A1A1A] dark:text-white">
					{query}
				</h3>
				<div className="relative mt-6 h-64 overflow-hidden rounded-[24px] bg-[linear-gradient(90deg,rgba(0,0,0,0.045)_1px,transparent_1px),linear-gradient(rgba(0,0,0,0.045)_1px,transparent_1px)] bg-[length:42px_42px] dark:bg-[linear-gradient(90deg,rgba(255,255,255,0.06)_1px,transparent_1px),linear-gradient(rgba(255,255,255,0.06)_1px,transparent_1px)]">
					{places.map((place, index) => (
						<span
							key={place.url}
							className="absolute grid size-8 place-items-center rounded-full bg-[#111111] text-[11px] font-semibold text-white shadow-lg"
							style={{
								left: `${18 + index * 18}%`,
								top: `${24 + (index % 2) * 34}%`,
							}}
						>
							{index + 1}
						</span>
					))}
				</div>
			</div>
			<div className="space-y-2">
				{places.map((place, index) => (
					<a
						key={place.url}
						href={place.url}
						target="_blank"
						rel="noopener noreferrer"
						className="flex min-w-0 items-center gap-3 rounded-[20px] bg-white/58 p-3 transition hover:bg-white/82 dark:bg-white/[0.04] dark:hover:bg-white/[0.07]"
					>
						<span className="grid size-9 shrink-0 place-items-center rounded-full bg-black/[0.04] text-[12px] font-semibold text-[#514B44] dark:bg-white/[0.07] dark:text-[#D4D6DC]">
							{index + 1}
						</span>
						<span className="min-w-0 flex-1">
							<span className="block truncate text-[14px] font-semibold text-[#1A1A1A] dark:text-white">
								{place.title ?? safeHostname(place.url)}
							</span>
							<span className="mt-0.5 block truncate text-[12px] text-[#9A9188] dark:text-[#737780]">
								{safeHostname(place.url)}
							</span>
						</span>
						<ExternalLink
							className="size-3.5 shrink-0 text-[#B6AFA7]"
							aria-hidden="true"
						/>
					</a>
				))}
			</div>
		</div>
	);
}

function ShoppingResultsPanel({
	query,
	results,
}: {
	query: string;
	results: WebSearchResult[];
}) {
	const items = results.slice(0, 5);
	if (items.length === 0) {
		return <SearchVerticalEmpty label="shopping" query={query} />;
	}

	return (
		<div className="space-y-3">
			<section className="rounded-[24px] bg-white/58 p-4 dark:bg-white/[0.04]">
				<p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#9A9188]">
					AI shopping/booking guide
				</p>
				<p className="mt-2 text-[13.5px] leading-relaxed text-[#756F68] dark:text-[#AEB4C0]">
					Velion compares booking references, shopping pages, similar
					articles, and source credibility before recommending an
					action.
				</p>
			</section>
			{items.map((item, index) => (
				<a
					key={item.url}
					href={item.url}
					target="_blank"
					rel="noopener noreferrer"
					className="flex gap-4 rounded-[24px] bg-white/58 p-4 transition hover:bg-white/82 dark:bg-white/[0.04] dark:hover:bg-white/[0.07]"
				>
					<span className="grid size-12 shrink-0 place-items-center rounded-[18px] bg-black/[0.04] text-[12px] font-semibold text-[#514B44] dark:bg-white/[0.07] dark:text-[#D4D6DC]">
						{index + 1}
					</span>
					<span className="min-w-0 flex-1">
						<span className="text-[12px] font-medium text-[#9A9188]">
							{safeHostname(item.url)}
						</span>
						<span className="mt-1 block line-clamp-2 text-[16px] font-semibold text-[#1A1A1A] dark:text-white">
							{item.title ?? item.url}
						</span>
						<span className="mt-2 block text-[13px] leading-relaxed text-[#756F68] dark:text-[#AEB4C0]">
							{item.snippet ??
								"Potential buying, booking, reference, or comparison source."}
						</span>
					</span>
				</a>
			))}
		</div>
	);
}

function SearchVerticalEmpty({
	label,
	query,
}: {
	label: string;
	query: string;
}) {
	return (
		<div className="rounded-[24px] bg-white/58 p-4 text-[13px] text-[#756F68] dark:bg-white/[0.04] dark:text-[#AEB4C0]">
			Velion needs more reliable {label} evidence for {query}. Try a more
			specific place, product, brand, or source name.
		</div>
	);
}

function isVideoLikeResult(result: WebSearchResult) {
	const haystack =
		`${safeHostname(result.url)} ${result.title ?? ""} ${result.snippet ?? ""}`.toLowerCase();
	return /youtube|youtu\.be|vimeo|tiktok|video|watch|clip|film|reel/.test(
		haystack,
	);
}

function SearchInsightRail({
	activeTab,
	answer,
	images,
	query,
	sourceItems,
}: {
	activeTab: SearchResultTab;
	answer: string;
	images: ImageHit[];
	query: string;
	sourceItems: Array<{ url: string; title: string; hostname: string }>;
}) {
	return (
		<div className="space-y-3">
			<section className="rounded-[24px] bg-white/50 p-4 dark:bg-white/[0.04]">
				<p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#9A9188]">
					Sources
				</p>
				<ExpandedSourcesList sourceItems={sourceItems} compact />
			</section>

			<section className="rounded-[24px] bg-white/50 p-4 dark:bg-white/[0.04]">
				<p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#9A9188]">
					AI notes
				</p>
				<p className="mt-2 line-clamp-5 text-[13px] leading-relaxed text-[#756F68] dark:text-[#AEB4C0]">
					{answer ||
						`Velion is organizing ${activeTab.toLowerCase()} evidence for "${query}" across sources, visuals, and useful next actions.`}
				</p>
			</section>

			<section className="rounded-[24px] bg-white/50 p-4 dark:bg-white/[0.04]">
				<p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#9A9188]">
					Tips
				</p>
				<ul className="mt-2 space-y-2 text-[12.5px] leading-relaxed text-[#756F68] dark:text-[#AEB4C0]">
					{searchRailTips(activeTab).map((tip) => (
						<li key={tip} className="flex gap-2">
							<span
								className="mt-[0.55em] size-1.5 shrink-0 rounded-full bg-[#C9C1B8]"
								aria-hidden="true"
							/>
							<span>{tip}</span>
						</li>
					))}
				</ul>
			</section>

			{images[0] ? (
				<section className="overflow-hidden rounded-[24px] bg-white/50 dark:bg-white/[0.04]">
					{/* eslint-disable-next-line @next/next/no-img-element */}
					<img
						src={images[0].thumbnailUrl}
						alt={images[0].title ?? ""}
						className="h-36 w-full object-cover"
						loading="lazy"
						decoding="async"
					/>
					<p className="p-3 text-[12px] leading-relaxed text-[#756F68] dark:text-[#AEB4C0]">
						Visual context can help Velion explain places, products,
						screenshots, or layout-specific details.
					</p>
				</section>
			) : null}
		</div>
	);
}

function searchRailTips(activeTab: SearchResultTab) {
	switch (activeTab) {
		case "Map":
			return [
				"Use the map view to compare location, travel context, opening hours, and booking confidence.",
				"Velion can turn place results into a short route or visit plan when map data is available.",
			];
		case "Images":
			return [
				"Images are treated as visual evidence, not final answers, until Velion can cite the source page.",
				"Velion can explain product, venue, layout, or screenshot details from the image context.",
			];
		case "Videos":
			return [
				"Video sources should be summarized with timestamps and cited source pages when available.",
				"Velion can extract the useful moments before suggesting that a user watches the full clip.",
			];
		case "Shopping":
			return [
				"Shopping and booking results should be cross-checked against official pages and recent articles.",
				"Velion can compare price, availability, source confidence, and similar alternatives.",
			];
		default:
			return [
				"Use sources to inspect where the answer came from before trusting or reusing it.",
				"Ask a follow-up to make Velion narrow, compare, crawl, or summarize the result set.",
			];
	}
}

function ExpandedSourcesList({
	compact = false,
	sourceItems,
}: {
	compact?: boolean;
	sourceItems: Array<{ url: string; title: string; hostname: string }>;
}) {
	if (sourceItems.length === 0) {
		return (
			<p
				className={cn(
					"text-[13px] leading-relaxed text-[#756F68] dark:text-[#AEB4C0]",
					compact ? "mt-3" : "",
				)}
			>
				Kilder vises når Velion har sikre treff.
			</p>
		);
	}

	return (
		<div className={cn("space-y-2", compact ? "mt-3" : "")}>
			{sourceItems.map((source) => (
				<a
					key={source.url}
					href={source.url}
					target="_blank"
					rel="noopener noreferrer"
					className="flex min-w-0 items-center gap-2 rounded-[15px] bg-white/62 px-3 py-2.5 text-[12px] transition hover:bg-white dark:bg-white/[0.03] dark:hover:bg-white/[0.06]"
				>
					<span className="grid size-8 shrink-0 place-items-center rounded-full bg-black/[0.04] text-[11px] font-semibold text-[#6F6860] dark:bg-white/[0.07] dark:text-[#D4D6DC]">
						{source.hostname.slice(0, 1).toUpperCase()}
					</span>
					<span className="min-w-0 flex-1">
						<span className="block truncate font-semibold text-[#2E3137] dark:text-white">
							{source.title}
						</span>
						<span className="block truncate text-[#9A9188] dark:text-[#737780]">
							{source.hostname}
						</span>
					</span>
					<ExternalLink
						className="size-3.5 shrink-0 text-[#B6AFA7]"
						aria-hidden="true"
					/>
				</a>
			))}
		</div>
	);
}

function KnowledgePanel() {
	const [expanded, setExpanded] = useState(false);

	return (
		<div className="velion-panel-in velion-dashboard-composer-card rounded-[28px] bg-white p-5 shadow-[0_20px_60px_rgba(20,21,24,0.08)] ring-1 ring-black/[0.03] dark:bg-[#141516] dark:ring-white/[0.06]">
			<div className="flex items-center gap-3">
				<span className="grid size-10 place-items-center rounded-[12px] bg-[#F4F5F1] text-[#6B6560]">
					<Sparkles className="size-4" />
				</span>
				<div className="min-w-0 flex-1">
					<p className="text-[14px] font-semibold text-[#1A1A1A]">
						Kunnskapsbase
					</p>
					<p className="text-[13px] text-[#7A756F]">
						Se status for indeksert innhold og koblede kilder.
					</p>
				</div>
				<button
					type="button"
					onClick={() => setExpanded(!expanded)}
					title={
						expanded ? "Skjul kunnskapsbase" : "Vis kunnskapsbase"
					}
					className="h-10 rounded-[12px] border border-black/[0.08] px-4 text-[13px] font-medium text-[#333] transition-colors hover:bg-black/[0.04]"
				>
					{expanded ? "Skjul" : "Vis"}
				</button>
			</div>
			{expanded ? (
				<div className="velion-fade-up mt-4 grid gap-2 sm:grid-cols-3">
					<Link
						href={"/knowledge" as Route}
						className="rounded-[14px] bg-[#F7F7F8] p-3 text-[12px] font-medium text-[#333] transition-colors hover:bg-[#EFEFF1]"
					>
						Koble kilde
					</Link>
					<Link
						href={"/knowledge" as Route}
						className="rounded-[14px] bg-[#F7F7F8] p-3 text-[12px] font-medium text-[#333] transition-colors hover:bg-[#EFEFF1]"
					>
						Importer dokumenter
					</Link>
					<Link
						href={"/settings" as Route}
						className="rounded-[14px] bg-[#F7F7F8] p-3 text-[12px] font-medium text-[#333] transition-colors hover:bg-[#EFEFF1]"
					>
						Tilganger
					</Link>
				</div>
			) : null}
		</div>
	);
}

function DashboardImageCard({
	card,
	onPrompt,
}: {
	card: DashboardCard;
	onPrompt: (card: DashboardCard) => void;
}) {
	if (card.id === "weather") {
		return <WeatherDashboardCard card={card} onPrompt={onPrompt} />;
	}

	if (card.id === "traffic") {
		return <TrafficDashboardCard card={card} onPrompt={onPrompt} />;
	}

	if (card.id === "news") {
		return <NewsDashboardCard card={card} onPrompt={onPrompt} />;
	}

	const aboveFold = aboveFoldDashboardCardIds.has(card.id);

	return (
		<div className="velion-dashboard-card group relative h-full overflow-hidden rounded-[18px] bg-white p-2.5 shadow-[0_2px_10px_rgba(0,0,0,0.05)] transition-transform duration-300 hover:-translate-y-0.5 dark:bg-[#141516]">
			<div className="velion-dashboard-card-label pointer-events-none absolute left-0 top-0 z-30 bg-white px-5 pb-4 pt-5 text-[11px] font-semibold tracking-wide text-[#1A1A1A] dark:bg-[#141516] dark:text-white">
				{card.category}
			</div>

			<Link href={card.href as Route} className="block" prefetch>
				<div className="velion-dashboard-card-media relative aspect-[4/3] overflow-hidden rounded-[15px]">
					<Image
						src={card.image ?? "/imagens/arched-corridor-1.jpeg"}
						alt={card.title}
						fill
						priority={aboveFold}
						loading={aboveFold ? "eager" : "lazy"}
						sizes="(max-width: 640px) 100vw, (max-width: 1024px) 70vw, 31vw"
						className="object-cover transition-transform duration-700 group-hover:scale-[1.02]"
					/>
					<div className="absolute inset-x-0 bottom-0 z-10 h-3/5 bg-gradient-to-t from-black/70 via-black/30 to-transparent" />
					<div className="velion-dashboard-card-copy pointer-events-none absolute inset-x-0 bottom-0 z-20 p-4 pb-16">
						<h3 className="text-[16px] font-semibold leading-snug text-white">
							{card.title}
						</h3>
						<p className="mt-1 line-clamp-2 text-[12px] leading-relaxed text-white/70">
							{card.description}
						</p>
					</div>
				</div>
			</Link>

			<button
				type="button"
				onClick={() => onPrompt(card)}
				className="velion-dashboard-card-action liquid-action absolute -bottom-px right-2.5 z-40 h-[86px] w-40"
				aria-label={`Start chat for ${card.title}`}
				title={`Start chat for ${card.title}`}
			>
				<LiquidCorner cardId={card.id} className="size-full" />
			</button>
		</div>
	);
}

function LiquidCorner({
	cardId,
	className,
}: {
	cardId: string;
	className?: string;
}) {
	const gradientId = `buttonGradient-${cardId}`;

	return (
		<svg
			className={className}
			viewBox="0 0 420 300"
			preserveAspectRatio="none"
			aria-hidden="true"
		>
			<defs>
				<linearGradient
					id={gradientId}
					x1="0%"
					y1="0%"
					x2="0%"
					y2="100%"
				>
					<stop offset="0%" stopColor="#FFFFFF" />
					<stop offset="100%" stopColor="#FAFAFA" />
				</linearGradient>
			</defs>

			<path
				d="M 0 260 C 39 250, 52 200, 78 160 C 98 120, 128 95, 170 90 L 280 90 C 335 90, 370 85, 395 70 C 410 55, 420 20, 420 0 L 420 270 L 0 270 Z"
				fill="white"
			/>
			<rect
				x="96"
				y="150"
				width="300"
				height="90"
				rx="45"
				fill="rgba(0, 0, 0, 0.04)"
			/>
			<rect
				x="101"
				y="150"
				width="290"
				height="88"
				rx="44"
				fill={`url(#${gradientId})`}
				stroke="#E8853D"
				strokeWidth="2"
			/>
			<text
				x="243"
				y="210"
				textAnchor="middle"
				fontSize="31"
				fontWeight="700"
				letterSpacing="0.22em"
				fill="#E8853D"
				style={{ pointerEvents: "none" }}
			>
				CHAT
			</text>
		</svg>
	);
}
