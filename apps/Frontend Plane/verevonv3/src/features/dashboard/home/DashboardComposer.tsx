import { useNavigate } from "@solidjs/router";
import {
	AlignCenter,
	AlignJustify,
	AlignLeft,
	ArrowLeft,
	ArrowUp,
	AudioWaveform,
	Blocks,
	Briefcase,
	BookOpen,
	Camera,
	Calendar,
	Check,
	ChevronDown,
	ChevronRight,
	CirclePlus,
	Clock3,
	EyeOff,
	FileText,
	Globe2,
	ImagePlus,
	Lightbulb,
	LayoutGrid,
	Loader2,
	Maximize2,
	MessageSquare,
	Minimize2,
	Mic,
	Paperclip,
	Pin,
	Search,
	SlidersHorizontal,
	Sparkles,
	Square,
	Telescope,
	Upload,
	WandSparkles,
	X,
	Zap,
	type LucideProps,
} from "@/shared/icons";
import {
	createEffect,
	createMemo,
	createSignal,
	flush,
	For,
	Match,
	onCleanup,
	Show,
	Switch,
	untrack,
	type Component,
} from "solid-js";
import { Dynamic, Portal, type JSX } from "@solidjs/web";
import { createResource } from "@/shared/lib/create-resource-compat";
import { dictateAudioBlob } from "@/shared/api/audio-client";
import {
	actionKey,
	BUILTIN_ACTIONS,
	loadSpecializedActions,
	type SpecializedAction,
	type SpecializedActionKind,
} from "@/shared/api/chat-actions-client";
import {
	loadComposerSettingsItems,
	type ComposerSettingsItem,
} from "@/shared/api/composer-settings-client";
import { selectChatThread } from "@/features/chat/lib/chat-thread-history";
import { writePendingChatLaunch } from "@/features/chat/lib/pending-chat-launch";
import {
	groupChatModels,
	hasZeroRetentionModel,
	isExpensiveModel,
	listChatThreads,
	listModels,
	saveChatThreadSnapshot,
	verevonModeById,
	VEREVON_BALANCE_MODE_ID,
	VEREVON_MODES,
	type ChatThreadSession,
	type ModelGroup,
	type ModelInfo,
} from "@/shared/api/chat-client";
import {
	listChatGptSubscriptions,
	OPENAI_CODEX_SUBSCRIPTION_PROVIDER,
} from "@/shared/api/chatgpt-subscription-client";
import {
	isClaimedPrivacyTier,
	isSelectablePrivacyTier,
	privacyTierBadgeLabel,
	privacyTierBadgeTitle,
	sovereignCatalogNotice,
	type PrivacyTier,
} from "@/shared/api/privacy-tier";
import { useI18n } from "@/shared/i18n";
import { cn } from "@/shared/lib/cn";
import { getSession } from "@/shared/session/session-store";
import {
	readAiModelSelection,
	rememberAiModelSelection,
} from "@/shared/ai/model-selection";

type ResponseMode = "auto" | "quick" | "deep";

type ResponseModeOption = {
	announcement: {
		en: string;
		no: string;
	};
	icon: Component<LucideProps>;
	id: ResponseMode;
	label: {
		en: string;
		no: string;
	};
};

type ComposerTone = "concise" | "balanced" | "detailed";

type ComposerFile = {
	id: string;
	name: string;
	size: number;
	type: string;
	url: string;
};

type ComposerTurn = {
	id: string;
	body: string;
	browseWeb: boolean;
	createdAt: string;
	createdAtIso: string;
	deepSearch: boolean;
	files: string[];
	model: string;
	responseMode: ResponseMode;
};

type HistoryPanelItem = {
	fallbackTime: string;
	id: string;
	meta: string;
	threadId?: string;
	title: string;
	updatedAt: string;
	/** Undefined for a live in-composer turn, which has no thread to pin yet. */
	pinned?: boolean;
	/** Latest server-owned run state, when this history item is a durable thread. */
	runStatus?: string;
};

type ComposerSettings = {
	tone: ComposerTone;
	voiceLang: string;
};

type SettingsView = "main" | "skills" | "connectors";
type EntityKind = "date" | "file";

type EntityToken = {
	kind: EntityKind;
	start: number;
	text: string;
};

type AutocompleteItem = {
	action?: "file" | "image";
	icon: Component<LucideProps>;
	id: string;
	label: string;
	meta?: EntityKind | "slash";
	specialized?: SpecializedAction;
};

type AutocompleteState = {
	category: string;
	items: AutocompleteItem[];
	triggerLen: number;
	triggerStart: number;
};

type TriggerContext =
	| { type: "date"; dayIndex: number; rawLen: number; start: number }
	| { type: "slash"; query: string; rawLen: number; start: number };

export type DashboardComposerAttachment = {
	id: string;
	name: string;
	size: number;
	type: string;
	url: string;
};

/** A skill/capability/connector activated via the `/` menu. */
export type ComposerActiveAction = {
	id: string;
	name: string;
	kind: "skill" | "capability" | "connector";
};

export type DashboardComposerSubmitPayload = {
	actions: ComposerActiveAction[];
	attachments: DashboardComposerAttachment[];
	model?: string;
	/** Provider route used only for a connected, user-owned model subscription. */
	provider?: string;
	/** Opaque Integration Core connection id; this is not a ChatGPT credential. */
	subscriptionConnectionId?: string;
	text: string;
	tools: Array<"image" | "research" | "search">;
	/**
	 * Response-style dial from composer settings. Sent so the setting has an
	 * effect: the gateway maps it to a verbosity directive. Omitted for
	 * "balanced", which is already the model's natural default there.
	 */
	tone?: "concise" | "detailed";
	/** Temporary chat (Zero Data Retention) toggle state at send time. */
	zdr?: boolean;
	/**
	 * Privacy tier of the SELECTED CATALOG MODEL, carried only when the user
	 * picked one (intent modes carry none — the backend resolves them
	 * server-side). Omitted from the wire unless set: unspecified means no
	 * constraint, byte-identical to today's behavior.
	 */
	minPrivacyTier?: PrivacyTier;
	/**
	 * Reasoning effort from the response-mode selector: "Raskt svar" → 'quick',
	 * "Dyp research" → 'deep'; Auto carries nothing. Maps to the wire's
	 * `effort` field, which model-gateway turns into a real thinking budget
	 * (quick=1024, deep=4096 tokens). This selector existed and rode the
	 * payload as `responseMode` with ZERO downstream readers — the dial's read
	 * path was fully built (gateway → provider → reasoning_delta → the Innsikt
	 * popover) while no product surface ever wrote it.
	 */
	effort?: "quick" | "deep";
};

type PanelPosition = {
	bottom?: number;
	left?: number;
	maxHeight: number;
	right?: number;
	top?: number;
};

const responseModes: ResponseModeOption[] = [
	{
		id: "auto",
		label: { no: "Auto", en: "Auto" },
		announcement: { no: "Auto-modus", en: "Auto mode" },
		icon: WandSparkles,
	},
	{
		id: "quick",
		label: { no: "Raskt svar", en: "Quick response" },
		announcement: {
			no: "Raskt svar aktivert",
			en: "Quick response activated",
		},
		icon: Zap,
	},
	{
		id: "deep",
		label: { no: "Dyp research", en: "Deep research" },
		announcement: { no: "Dyp research-modus", en: "Deep research mode" },
		icon: Lightbulb,
	},
];
const voiceLanguages = [
	{ value: "en-US", label: { no: "Engelsk (USA)", en: "English (US)" } },
	{ value: "en-GB", label: { no: "Engelsk (UK)", en: "English (UK)" } },
	{
		value: "nb-NO",
		label: { no: "Norsk (bokmål)", en: "Norwegian (Bokmål)" },
	},
	{
		value: "nn-NO",
		label: { no: "Norsk (nynorsk)", en: "Norwegian (Nynorsk)" },
	},
	{ value: "sv-SE", label: { no: "Svensk", en: "Swedish" } },
	{ value: "da-DK", label: { no: "Dansk", en: "Danish" } },
	{ value: "de-DE", label: { no: "Tysk", en: "German" } },
	{ value: "fr-FR", label: { no: "Fransk", en: "French" } },
	{ value: "es-ES", label: { no: "Spansk", en: "Spanish" } },
	{
		value: "pt-BR",
		label: { no: "Portugisisk (BR)", en: "Portuguese (BR)" },
	},
] as const;
const toneOptions: Array<{
	value: ComposerTone;
	label: { no: string; en: string };
	icon: Component<LucideProps>;
}> = [
	{
		value: "concise",
		label: { no: "Kortfattet", en: "Concise" },
		icon: AlignLeft,
	},
	{
		value: "balanced",
		label: { no: "Balansert", en: "Balanced" },
		icon: AlignCenter,
	},
	{
		value: "detailed",
		label: { no: "Detaljert", en: "Detailed" },
		icon: AlignJustify,
	},
];
const dayEntries = [
	{ name: "Monday", nameNo: "mandag", dayIndex: 1 },
	{ name: "Tuesday", nameNo: "tirsdag", dayIndex: 2 },
	{ name: "Wednesday", nameNo: "onsdag", dayIndex: 3 },
	{ name: "Thursday", nameNo: "torsdag", dayIndex: 4 },
	{ name: "Friday", nameNo: "fredag", dayIndex: 5 },
	{ name: "Saturday", nameNo: "lørdag", dayIndex: 6 },
	{ name: "Sunday", nameNo: "søndag", dayIndex: 0 },
] as const;
const slashCommands: AutocompleteItem[] = [
	{
		id: "cmd-file",
		icon: Upload,
		label: "Last opp fil",
		meta: "slash",
		action: "file",
	},
	{
		id: "cmd-image",
		icon: ImagePlus,
		label: "Generer bilde",
		meta: "slash",
		action: "image",
	},
];

const TEXTAREA_AUTO_MAX_PX = 240;

type ComposerSubmitState = {
	hasContent: boolean;
	submitting?: boolean;
	voiceMode: boolean;
	voiceRecording: boolean;
};

function dashboardComposerRootClass(input: { dragActive: boolean; appearance?: "chat" | "default" }) {
	return cn(
		"dashboard-composer-root",
		input.dragActive ? "dashboard-composer-root--drag-active" : "",
		input.appearance === "chat" ? "dashboard-composer-root--chat" : "",
	);
}

function composerChevronClass(open: boolean) {
	return cn(
		"size-4 dashboard-composer-model-button__chevron",
		open ? "dashboard-composer-model-button__chevron--open" : "",
	);
}

function composerWebButtonClass(active: boolean) {
	return cn(
		"dashboard-composer-web-button",
		active ? "dashboard-composer-web-button--active" : "",
	);
}

function composerImageButtonClass(active: boolean) {
	return cn(
		"dashboard-composer-image-button",
		active ? "dashboard-composer-image-button--active" : "",
	);
}

function composerSubmitButtonClass(enabled: boolean) {
	return cn(
		"dashboard-composer-submit-button",
		enabled ? "dashboard-composer-submit-button--active" : "",
	);
}

function textareaOverlayStyle(hasEntities: boolean): JSX.CSSProperties {
	return {
		color: hasEntities ? "transparent" : undefined,
		"caret-color": "currentColor",
	};
}

function modeAnnouncementDisplay(announcement: string | null, message: string) {
	if (!announcement || message) return "";
	return announcement;
}

function textareaPlaceholder(
	announcement: string | null,
	i18n: ReturnType<typeof useI18n>,
	suggestion: string | null,
) {
	if (announcement) return "";
	// A rotating, concrete suggestion beats the standing generic line: the old
	// placeholder told people the composer exists, not what it is good for.
	// The Tab hint is part of the string because the affordance is otherwise
	// invisible — nothing else on the surface says the key does anything.
	if (suggestion)
		return i18n.tr(
			`${suggestion}  —  Tab for å bruke`,
			`${suggestion}  —  Tab to use`,
		);
	return i18n.tr(
		"Spør om hva som helst, bruk / for spesialhandlinger.",
		"Ask anything, use / to activate specialized actions.",
	);
}

function textareaCursorPosition(element: HTMLTextAreaElement) {
	return element.selectionStart ?? element.value.length;
}

function shouldShowTextareaExpandButton(
	hasOverflow: boolean,
	expanded: boolean,
) {
	return hasOverflow || expanded;
}

function textareaExpandLabel(
	expanded: boolean,
	i18n: ReturnType<typeof useI18n>,
) {
	return expanded
		? i18n.tr("Slå sammen feltet", "Collapse input")
		: i18n.tr("Utvid for å se hele teksten", "Expand to see full text");
}

function textareaExpandTitle(
	expanded: boolean,
	i18n: ReturnType<typeof useI18n>,
) {
	return expanded
		? i18n.tr("Slå sammen", "Collapse")
		: i18n.tr("Utvid", "Expand");
}

function isVoiceInputLocked(voiceMode: boolean, voiceRecording: boolean) {
	return voiceMode || voiceRecording;
}

function isComposerSubmitEnabled(input: ComposerSubmitState) {
	return (
		input.hasContent &&
		!isVoiceInputLocked(input.voiceMode, input.voiceRecording) &&
		!input.submitting
	);
}

function shouldShowStopButton(
	submitting: boolean | undefined,
	onStop: (() => void) | undefined,
) {
	return Boolean(submitting && onStop);
}

function visibleTurnReceipt(
	showTurnReceipt: boolean | undefined,
	turn: ComposerTurn | undefined,
) {
	return (showTurnReceipt ?? true) ? turn : undefined;
}

function dragEventHasFiles(event: DragEvent) {
	return Array.from(event.dataTransfer?.types ?? []).includes("Files");
}

function droppedFiles(event: DragEvent) {
	return Array.from(event.dataTransfer?.files ?? []);
}

function pastedImageFiles(event: ClipboardEvent) {
	return Array.from(event.clipboardData?.files ?? []).filter((file) =>
		file.type.startsWith("image/"),
	);
}

export function DashboardComposer(props: {
	/** Calm, conversation-first presentation used by the dedicated chat page. */
	appearance?: "chat" | "default";
	browseWeb?: boolean;
	imageMode?: boolean;
	message: string;
	onBrowseWebChange?: (value: boolean) => void;
	onImageModeChange?: (value: boolean) => void;
	onMessageChange: (value: string) => void;
	onPlanModeChange?: (value: boolean) => void;
	planMode?: boolean;
	onStop?: () => void;
	onLaunchStart?: () => void;
	onSubmit?: (
		payload: DashboardComposerSubmitPayload,
	) => Promise<void> | void;
	showTurnReceipt?: boolean;
	submitting?: boolean;
	/**
	 * Let Enter submit while `submitting` is true, so a message typed during a
	 * live stream becomes a MID-RUN delivery instead of being swallowed.
	 *
	 * Off by default: the dashboard composer's `submitting` window is its own
	 * send round-trip, where a second submit would double-send. The chat page
	 * opts in because there `submitting` spans the whole model stream and the
	 * controller's `sendContent` routes a streaming-time submit into
	 * `deliverMidRun` (persist to thread -> queue into the run). Until this
	 * prop existed the guard below returned silently for the entire stream, so
	 * the server-side queued-input machinery was client-side dead and mid-run
	 * messages were still dropped — the exact bug it was built to fix. The
	 * Stop button is unaffected; it still replaces the send button.
	 */
	allowMidRunSubmit?: boolean;
	/** "Midlertidig samtale" (ChatGPT's Temporary Chat) — maps to the request's ZDR flag. */
	temporaryChat?: boolean;
	onTemporaryChatChange?: (value: boolean) => void;
	/** Once a message has been sent in a temporary thread, the toggle can no longer be turned off mid-conversation. */
	temporaryChatLocked?: boolean;
	/** Foreign-origin transcript currently displayed read-only in Chat. */
	disabled?: boolean;
	/** Explicitly persist selected files as durable organisation knowledge. */
	onKnowledgeImport?: (attachments: DashboardComposerAttachment[]) => Promise<void> | void;
}) {
	const i18n = useI18n();
	const navigate = useNavigate();
	let composerRootRef: HTMLDivElement | undefined;
	let fileInputRef: HTMLInputElement | undefined;
	let historyTriggerRef: HTMLSpanElement | undefined;
	let settingsTriggerRef: HTMLSpanElement | undefined;
	let textareaRef: HTMLTextAreaElement | undefined;
	let modeAnnouncementTimer: number | undefined;
	let mediaRecorder: MediaRecorder | undefined;
	let audioChunks: Blob[] = [];
	let pendingFilePosition: number | null = null;
	const [internalBrowseWeb, setInternalBrowseWeb] = createSignal(false);
	const browseWeb = () => props.browseWeb ?? internalBrowseWeb();
	const setBrowseWeb = (next: boolean | ((current: boolean) => boolean)) => {
		const value = typeof next === "function" ? next(browseWeb()) : next;
		if (props.browseWeb === undefined) setInternalBrowseWeb(value);
		props.onBrowseWebChange?.(value);
	};
	const [autocomplete, setAutocomplete] =
		createSignal<AutocompleteState | null>(null);
	const [autocompleteIndex, setAutocompleteIndex] = createSignal(0);
	const [deepSearch, setDeepSearch] = createSignal(false);
	const [dragActive, setDragActive] = createSignal(false);
	const [entities, setEntities] = createSignal<EntityToken[]>([]);
	const [files, setFiles] = createSignal<ComposerFile[]>([]);
	const [knowledgeImporting, setKnowledgeImporting] = createSignal(false);
	const [knowledgeImportNotice, setKnowledgeImportNotice] = createSignal<string | null>(null);
	const [historyPanelPosition, setHistoryPanelPosition] =
		createSignal<PanelPosition>({ bottom: 0, right: 0, maxHeight: 400 });
	const [historyThreads, setHistoryThreads] = createSignal<
		ChatThreadSession[]
	>([]);
	const [historyLoading, setHistoryLoading] = createSignal(false);
	const [historyError, setHistoryError] = createSignal<string | null>(null);
	const [historyOpen, setHistoryOpen] = createSignal(false);
	const [modeAnnouncement, setModeAnnouncement] = createSignal<string | null>(
		null,
	);
	const [modelOpen, setModelOpen] = createSignal(false);
	const [contextScopeOpen, setContextScopeOpen] = createSignal(false);
	const [responseMode, setResponseMode] = createSignal<ResponseMode>("auto");
	// The picker pins three first-class Verevon intent modes (Budget/Balance/Genius)
	// at the very top; the backend resolves these pseudo-model ids server-side. The
	// real catalog models come live from the gateway `/api/v1/models` (Model Plane)
	// and render below. Default = Verevon Balance. We never auto-select an expensive
	// catalog model; users pick opus/sonnet/gpt-5.x deliberately (cost badge nudges).
	const modelPreferenceOrgId = getSession().activeOrg?.id?.trim() ?? "";
	const initialModelSelection = readAiModelSelection(modelPreferenceOrgId);
	const [selectedModel, setSelectedModel] = createSignal<string>(
		initialModelSelection.model || VEREVON_BALANCE_MODE_ID,
	);
	const [selectedModelProvider, setSelectedModelProvider] = createSignal<string | undefined>(initialModelSelection.provider);
	const [models] = createResource(async () => {
		try {
			return await listModels();
		} catch {
			// Gateway unavailable / old shape → empty catalog. The pinned Verevon intent
			// modes still render (they don't depend on /v1/models), so the picker works.
			return [] as ModelInfo[];
		}
	});
	const activeOrgId = () => getSession().activeOrg?.id?.trim() || false;
	const [subscriptionConnections] = createResource(
		activeOrgId,
		async (orgId) => {
			try {
				return await listChatGptSubscriptions(orgId);
			} catch {
				return [];
			}
		},
	);
	const activeSubscriptionConnection = createMemo(() =>
		(subscriptionConnections() ?? []).find(
			(connection) => connection.status.toLocaleLowerCase() === "active",
		),
	);
	// Chat-capable models grouped by family/provider. Non-chat modalities
	// (image/video/embeddings/transcribe) are filtered out by `groupChatModels`.
	const chatModelGroups = createMemo<ModelGroup[]>(() =>
		groupChatModels(models() ?? []).filter(
			(group) => group.label !== "Subscription" || activeSubscriptionConnection(),
		),
	);
	// Temporary chat is only offerable if some deployment is ATTESTED zero-retention.
	// inference-core fails a ZDR request closed — it skips every provider whose
	// `supports_zdr` is false — so without an attested one the toggle produces a
	// turn that always dies with "The ephemeral inference request failed". The
	// catalogue already carries the answer per model; this reads it instead of
	// letting the user discover it by spending a message.
	//
	// `undefined` while the resource is still loading, so a slow catalogue does
	// not flash the control into a disabled state and back.
	const zeroRetentionAvailable = createMemo<boolean | undefined>(() =>
		models.loading || models() === undefined
			? undefined
			: hasZeroRetentionModel(models() ?? []),
	);
	const flatChatModels = createMemo<ModelInfo[]>(() =>
		chatModelGroups().flatMap((group) => group.models),
	);
	const selectedCatalogModel = createMemo(() => {
		const provider = selectedModelProvider();
		// No truthiness guard on `provider`: plenty of real catalog models have
		// no provider tag (ModelInfo.provider is optional), and `selectModel`
		// below already threads the clicked model's own provider straight into
		// both signals, so an undefined-vs-undefined match is a legitimate
		// selection, not an absent one. Bailing out here previously made
		// selectedPrivacyTier() (and the submit payload's minPrivacyTier) go
		// undefined for any selected model without a provider.
		return flatChatModels().find(
			(model) => model.id === selectedModel() && model.provider === provider,
		);
	});
	const selectedModelLabel = () => {
		const id = selectedModel();
		// Empty id → backend treats it as Verevon Balance (resilient fallback).
		if (!id) return "Verevon Balance";
		return (
			verevonModeById(id)?.label ??
			selectedCatalogModel()?.name ??
			id
		);
	};
	// The selected model's attested privacy tier, if any. Only a real catalog
	// selection can carry one: the pinned intent modes are resolved server-side,
	// so they never claim a tier here.
	const selectedPrivacyTier = createMemo(() => selectedCatalogModel()?.privacyTier);
	const selectedSubscriptionRoute = createMemo(() => {
		const model = selectedCatalogModel();
		const connection = activeSubscriptionConnection();
		return model?.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER && connection
			? { provider: OPENAI_CODEX_SUBSCRIPTION_PROVIDER, connectionId: connection.id }
			: undefined;
	});
	const selectModel = (id: string, provider?: string) => {
		setSelectedModel(id);
		setSelectedModelProvider(provider);
		const catalogModel = flatChatModels().find(
			(model) => model.id === id && model.provider === provider,
		);
		rememberAiModelSelection(activeOrgId() || "", {
			model: id,
			label: verevonModeById(id)?.label ?? catalogModel?.name ?? id,
			...(provider ? { provider } : {}),
			...(provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER && activeSubscriptionConnection()
				? { subscriptionConnectionId: activeSubscriptionConnection()!.id }
				: {}),
		});
		setModelOpen(false);
	};
	const [settings, setSettings] = createSignal<ComposerSettings>({
		tone: "balanced",
		voiceLang: "nb-NO",
	});
	const [settingsPanelPosition, setSettingsPanelPosition] =
		createSignal<PanelPosition>({ top: 0, left: 0, maxHeight: 460 });
	const [settingsOpen, setSettingsOpen] = createSignal(false);
	const [suggestionsOpen, setSuggestionsOpen] = createSignal(false);
	const [turns, setTurns] = createSignal<ComposerTurn[]>([]);
	const [voiceMode, setVoiceMode] = createSignal(false);
	const [voiceRecording, setVoiceRecording] = createSignal(false);
	const hasContent = createMemo(
		() => props.message.trim().length > 0 || files().length > 0,
	);
	const modelAttachmentCount = createMemo(
		() => files().filter((file) => file.type.startsWith("image/")).length,
	);
	const previewOnlyAttachmentCount = createMemo(
		() => files().filter((file) => !file.type.startsWith("image/")).length,
	);
	const imageMode = createMemo(() => props.imageMode ?? false);
	const planMode = createMemo(() => props.planMode ?? false);
	// Locked-true once the parent says the active thread is temporary — a
	// disabled control could theoretically still be flipped by, e.g., a
	// synthetic click, so the memo itself also refuses to report anything but
	// `true` while locked rather than relying on `disabled` alone.
	const temporaryChat = createMemo(
		() => props.temporaryChatLocked || (props.temporaryChat ?? false),
	);
	// Locked (already inside a temporary thread) still wins: that control is
	// disabled because the choice is settled, not because it is unavailable.
	const temporaryChatUnavailable = createMemo(
		() => !props.temporaryChatLocked && zeroRetentionAvailable() === false,
	);
	const hasEntityOverlay = createMemo(() => entities().length > 0);
	const [textareaExpanded, setTextareaExpanded] = createSignal(false);
	const [hasOverflow, setHasOverflow] = createSignal(false);
	const currentModeAnnouncement = createMemo(() =>
		modeAnnouncementDisplay(modeAnnouncement(), props.message),
	);
	const textareaLocked = createMemo(() =>
		isVoiceInputLocked(voiceMode(), voiceRecording()),
	);
	const showTextareaExpand = createMemo(() =>
		shouldShowTextareaExpandButton(hasOverflow(), textareaExpanded()),
	);
	const submitEnabled = createMemo(() =>
		isComposerSubmitEnabled({
			hasContent: hasContent(),
			submitting: props.submitting && !props.allowMidRunSubmit,
			voiceMode: voiceMode(),
			voiceRecording: voiceRecording(),
		}),
	);
	const stopButtonVisible = createMemo(() =>
		shouldShowStopButton(props.submitting, props.onStop),
	);
	const activeTurnReceipt = createMemo(() =>
		visibleTurnReceipt(props.showTurnReceipt, turns()[0]),
	);
	const [activeActions, setActiveActions] = createSignal<
		ComposerActiveAction[]
	>([]);
	const [specializedActions, setSpecializedActions] =
		createSignal<SpecializedAction[]>(BUILTIN_ACTIONS);
	let actionsLoadStarted = false;

	const openFileDialog = () => fileInputRef?.click();
	const focusTextareaAt = (caret?: number) => {
		const element = textareaRef;
		if (!element) return;
		element.focus();
		if (typeof caret === "number") {
			element.setSelectionRange(caret, caret);
		}
	};
	const focusTextareaAtNextFrame = (caret?: number) => {
		window.requestAnimationFrame(() => focusTextareaAt(caret));
	};

	// Lazily fetch skills/capabilities/connectors the first time `/` is used, so
	// the dashboard doesn't pay for the call until the menu is actually opened.
	const ensureActionsLoaded = () => {
		if (actionsLoadStarted) return;
		actionsLoadStarted = true;
		void loadSpecializedActions()
			.then((items) => {
				setSpecializedActions(
					items.length > 0 ? items : BUILTIN_ACTIONS,
				);
				untrack(() => {
					const state = autocomplete();
					if (state?.category === i18n.tr("Handlinger", "Actions")) {
						const query = props.message.slice(
							state.triggerStart + 1,
							state.triggerStart + state.triggerLen,
						);
						openActionMenu(
							query,
							state.triggerStart,
							state.triggerLen,
						);
					}
				});
			})
			.catch(() => undefined);
	};

	const buildSlashItems = (query: string): AutocompleteItem[] => {
		const normalized = query.trim().toLowerCase();
		const active = activeActions();
		return specializedActions()
			.filter(
				(action) =>
					action.kind === "builtin" ||
					!active.some(
						(a) => a.id === action.id && a.kind === action.kind,
					),
			)
			.filter(
				(action) =>
					normalized.length === 0 ||
					action.name.toLowerCase().includes(normalized) ||
					action.id.toLowerCase().includes(normalized),
			)
			.slice(0, 8)
			.map((action) => ({
				id: actionKey(action),
				icon: iconForAction(action),
				label: action.name,
				meta: "slash" as const,
				specialized: action,
			}));
	};

	const openActionMenu = (
		query: string,
		triggerStart: number,
		triggerLen: number,
	) => {
		const items = buildSlashItems(query);
		setAutocompleteIndex(0);
		setAutocomplete(
			items.length > 0
				? {
						category: i18n.tr("Handlinger", "Actions"),
						items,
						triggerStart,
						triggerLen,
					}
				: null,
		);
	};

	const removeActiveAction = (target: ComposerActiveAction) => {
		setActiveActions((current) =>
			current.filter(
				(a) => !(a.id === target.id && a.kind === target.kind),
			),
		);
	};

	const activateSpecialized = (action: SpecializedAction) => {
		if (action.kind === "builtin") {
			if (action.builtin === "file") openFileDialog();
			else if (action.builtin === "image")
				props.onImageModeChange?.(true);
			else if (action.builtin === "web_search") setBrowseWeb(true);
			return;
		}
		const kind = action.kind;
		setActiveActions((current) =>
			current.some((a) => a.id === action.id && a.kind === kind)
				? current
				: [...current, { id: action.id, name: action.name, kind }],
		);
	};

	// Judgment call: the DOM measurement (scrollHeight) is only meaningful right
	// after the "auto" height write below it, so the read/write sequence can't be
	// cleanly split into a side-effect-free compute. Compute instead captures the
	// two tracked dependencies (props.message — to re-run on every keystroke —
	// and textareaExpanded()), and the effect keeps the original read-then-write
	// DOM sequence untouched.
	createEffect(
		() => ({ message: props.message, expanded: textareaExpanded() }),
		({ expanded }) => {
			const element = textareaRef;
			if (!element) return;
			element.style.height = "auto";
			const naturalH = element.scrollHeight;
			const overflows = naturalH > TEXTAREA_AUTO_MAX_PX;
			setHasOverflow(overflows);
			if (expanded) {
				const maxH = Math.floor(window.innerHeight * 0.5);
				element.style.height = `${Math.min(naturalH, maxH)}px`;
				element.style.overflowY = naturalH > maxH ? "auto" : "hidden";
			} else {
				element.style.height = `${overflows ? TEXTAREA_AUTO_MAX_PX : naturalH}px`;
				element.style.overflowY = overflows ? "auto" : "hidden";
			}
		},
	);

	/**
	 * Rotating composer suggestions, Copilot-style.
	 *
	 * Context-derived where real context exists: an already-loaded recent
	 * thread becomes a "follow up on X" prompt. No fetch is added for this —
	 * `historyThreads` populates when the History panel is opened, and a
	 * placeholder is not worth a network call on every composer mount. When
	 * there is nothing to draw on it falls back to concrete examples rather
	 * than the old generic line.
	 */
	const composerSuggestions = createMemo(() => {
		const recent = historyThreads()
			.slice(0, 2)
			.map((thread) => thread.title?.trim())
			.filter((title): title is string => !!title && title.length > 3)
			.map((title) =>
				i18n.tr(`Følg opp «${title}»`, `Follow up on "${title}"`),
			);
		return [
			...recent,
			i18n.tr(
				"Oppsummer et dokument til beslutningspunkter",
				"Summarize a document into decision points",
			),
			i18n.tr(
				"Finn svaret i kunnskapsbasen, med kilder",
				"Find the answer in the knowledge base, with sources",
			),
			i18n.tr(
				"Lag et førsteutkast jeg kan redigere",
				"Draft a first version I can edit",
			),
		];
	});
	const [suggestionIndex, setSuggestionIndex] = createSignal(0);
	const activeSuggestion = () => {
		// Only offered for an empty composer: rotating text under something the
		// user is already typing would be noise, and Tab must keep its normal
		// focus-move behaviour the moment there is real input to tab away from.
		if (props.message.trim()) return null;
		const list = composerSuggestions();
		return list.length > 0 ? (list[suggestionIndex() % list.length] ?? null) : null;
	};
	createEffect(
		() => ({ count: composerSuggestions().length, idle: !props.message.trim() }),
		({ count, idle }) => {
			if (count <= 1 || !idle) return undefined;
			const timer = window.setInterval(() => {
				setSuggestionIndex((index) => (index + 1) % count);
			}, 6000);
			// Returned, not `onCleanup`: inside a two-argument effect's effect
			// function this fork drops onCleanup silently, leaking the interval.
			return () => window.clearInterval(timer);
		},
	);

	createEffect(
		() => ({
			modelOpen: modelOpen(),
			contextScopeOpen: contextScopeOpen(),
			historyOpen: historyOpen(),
			settingsOpen: settingsOpen(),
			suggestionsOpen: suggestionsOpen(),
			autocomplete: autocomplete(),
		}),
		(state) => {
			if (
				!state.modelOpen &&
				!state.contextScopeOpen &&
				!state.historyOpen &&
				!state.settingsOpen &&
				!state.suggestionsOpen &&
				!state.autocomplete
			)
				return;

			const closePanels = () => {
				setModelOpen(false);
				setContextScopeOpen(false);
				setHistoryOpen(false);
				setSettingsOpen(false);
				setSuggestionsOpen(false);
				setAutocomplete(null);
			};
			const handlePointerDown = (event: PointerEvent) => {
				const target = event.target;
				if (!(target instanceof Node)) return;
				if (composerRootRef?.contains(target)) return;
				if (
					target instanceof Element &&
					target.closest('[data-composer-floating-panel="true"]')
				)
					return;
				if (
					target instanceof Element &&
					target.closest('[data-dashboard-modal="true"]')
				)
					return;
				closePanels();
			};
			const handleKeyDown = (event: KeyboardEvent) => {
				if (event.key === "Escape") closePanels();
			};

			document.addEventListener("pointerdown", handlePointerDown, true);
			document.addEventListener("keydown", handleKeyDown);
			return () => {
				document.removeEventListener(
					"pointerdown",
					handlePointerDown,
					true,
				);
				document.removeEventListener("keydown", handleKeyDown);
			};
		},
	);

	const addFiles = (nextFiles: FileList | File[]) => {
		const incoming = Array.from(nextFiles);
		if (pendingFilePosition !== null && incoming[0]) {
			const file = incoming[0];
			const position = pendingFilePosition;
			const nextMessage = `${props.message.slice(0, position)}${file.name}${props.message.slice(position)}`;
			pendingFilePosition = null;
			props.onMessageChange(nextMessage);
			setEntities((current) => [
				...current,
				{ kind: "file", start: position, text: file.name },
			]);
			setAutocomplete(null);
			focusTextareaAtNextFrame(position + file.name.length);
			return;
		}

		const created = Array.from(nextFiles).map((file, index) => ({
			id: `${file.name}-${file.size}-${file.lastModified}-${Date.now()}-${index}`,
			name: file.name,
			size: file.size,
			type: file.type || "application/octet-stream",
			url: createFilePreviewUrl(file),
		}));

		if (created.length > 0) {
			setFiles((current) => [...current, ...created]);
		}
	};

	const removeFile = (id: string) => {
		setFiles((current) => {
			const removed = current.find((file) => file.id === id);
			if (removed) revokeFilePreviewUrl(removed);
			return current.filter((file) => file.id !== id);
		});
	};

	const enhanceAttachments = () => {
		if (files().length === 0) return;

		const names = files()
			.map((file) => file.name)
			.join(", ");
		const body = props.message.trim();
		props.onMessageChange(
			body
				? i18n.tr(
						`Analyser vedlagte fil(er) (${names}) og ${body}`,
						`Analyze the attached file(s) (${names}) and ${body}`,
					)
				: i18n.tr(
						`Beskriv og analyser vedlagte fil(er): ${names}`,
						`Describe and analyze the attached file(s): ${names}`,
					),
		);
		focusTextareaAt();
	};

	const importFilesToKnowledge = async () => {
		if (!props.onKnowledgeImport || files().length === 0 || knowledgeImporting()) return;
		if (temporaryChat()) {
			setKnowledgeImportNotice(i18n.tr(
				"Midlertidig chat kan ikke lagre filer i kunnskapsbasen.",
				"Temporary chat cannot save files to the knowledge base.",
			));
			return;
		}
		setKnowledgeImporting(true);
		setKnowledgeImportNotice(null);
		try {
			const importable = files().filter((file) => !file.type.startsWith("image/"));
			if (importable.length === 0) {
				setKnowledgeImportNotice(i18n.tr(
					"Bilder sendes til modellen og kan ikke lagres som kunnskapsdokumenter her.",
					"Images are sent to the model and cannot be saved as knowledge documents here.",
				));
				return;
			}
			await props.onKnowledgeImport(importable.map((file) => ({ ...file })));
			setKnowledgeImportNotice(i18n.tr(
				"Importjobb startet. Filene blir tilgjengelige i organisasjonskunnskapen når den er ferdig.",
				"Import job started. The files will be available in organisation knowledge when it finishes.",
			));
		} catch (error) {
			setKnowledgeImportNotice(error instanceof Error ? error.message : i18n.tr(
				"Kunne ikke starte importen.",
				"Could not start the import.",
			));
		} finally {
			setKnowledgeImporting(false);
		}
	};

	const toggleRecording = async () => {
		if (voiceRecording()) {
			mediaRecorder?.stop();
			return;
		}

		if (
			typeof MediaRecorder === "undefined" ||
			!navigator.mediaDevices?.getUserMedia
		) {
			setModeAnnouncement(
				i18n.tr(
					"Stemmeopptak støttes ikke i denne nettleseren.",
					"Voice recording is not supported in this browser.",
				),
			);
			return;
		}

		try {
			const voiceLang = settings().voiceLang;
			const onTranscript = props.onMessageChange;
			const stream = await navigator.mediaDevices.getUserMedia({
				audio: true,
			});
			audioChunks = [];
			const recorder = new MediaRecorder(stream);
			mediaRecorder = recorder;

			recorder.ondataavailable = (event) => {
				if (event.data.size > 0)
					audioChunks = [...audioChunks, event.data];
			};
			recorder.onstop = () => {
				const audio = new Blob(audioChunks, {
					type: recorder.mimeType || "audio/webm",
				});
				audioChunks = [];
				stream.getTracks().forEach((track) => track.stop());
				mediaRecorder = undefined;

				// Verevon Flow: STT + LLM cleanup in one round trip (fillers stripped,
				// punctuation fixed, self-corrections applied). Keep `voiceRecording`
				// on through the cleanup so the textarea stays locked and the loading
				// bar covers the processing phase; APPEND to the draft (the voice-modal
				// pattern) instead of overwriting whatever was already typed.
				void dictateAudioBlob(audio, voiceLang, "chat message")
					.then((dictation) => {
						const text = dictation.text || dictation.rawText;
						if (!text) return;
						const trimmed = props.message.trim();
						onTranscript(trimmed ? `${trimmed} ${text}` : text);
						focusTextareaAtNextFrame();
					})
					.catch(() => {
						setModeAnnouncement(
							i18n.tr(
								"Diktering er utilgjengelig.",
								"Dictation is unavailable.",
							),
						);
					})
					.finally(() => {
						setVoiceRecording(false);
					});
			};
			recorder.onerror = () => {
				stream.getTracks().forEach((track) => track.stop());
				mediaRecorder = undefined;
				setVoiceRecording(false);
			};

			recorder.start();
			setVoiceRecording(true);
		} catch {
			setVoiceRecording(false);
			setModeAnnouncement(
				i18n.tr(
					"Mikrofontilgang var ikke tilgjengelig.",
					"Microphone access was not available.",
				),
			);
		}
	};

	const clearFiles = () => {
		files().forEach(revokeFilePreviewUrl);
		setFiles([]);
	};

	const addScreenshotFile = async () => {
		if (!navigator.mediaDevices?.getDisplayMedia) return;

		try {
			const stream = await navigator.mediaDevices.getDisplayMedia({
				video: true,
			});
			const video = document.createElement("video");
			video.srcObject = stream;
			await new Promise<void>((resolve) => {
				video.onloadedmetadata = () => resolve();
			});
			await video.play();
			await new Promise((resolve) => window.setTimeout(resolve, 100));

			const canvas = document.createElement("canvas");
			canvas.width = video.videoWidth;
			canvas.height = video.videoHeight;
			canvas.getContext("2d")?.drawImage(video, 0, 0);
			stream.getTracks().forEach((track) => track.stop());

			const blob = await new Promise<Blob | null>((resolve) =>
				canvas.toBlob(resolve, "image/png"),
			);
			if (!blob) return;

			const file = new File([blob], `screenshot-${Date.now()}.png`, {
				type: "image/png",
			});
			addFiles([file]);
		} catch {
			// Browser throws when the user cancels capture; the composer stays unchanged.
		}
	};

	const closeSecondaryPanels = () => {
		flush(() => {
			setHistoryOpen(false);
			setSettingsOpen(false);
			setSuggestionsOpen(false);
			setContextScopeOpen(false);
			setAutocomplete(null);
		});
	};

	let historyRequestSeq = 0;
	const refreshChatHistory = async () => {
		const requestSeq = ++historyRequestSeq;
		setHistoryLoading(true);
		setHistoryError(null);
		try {
			const sessions = await listChatThreads();
			if (requestSeq === historyRequestSeq) setHistoryThreads(sessions);
		} catch {
			if (requestSeq === historyRequestSeq)
				setHistoryError(
					i18n.tr(
						"Kunne ikke laste samtaler.",
						"Could not load conversations.",
					),
				);
		} finally {
			if (requestSeq === historyRequestSeq) setHistoryLoading(false);
		}
	};

	// Pin/unpin is server-owned (the gateway thread index), so it follows the
	// user across devices. Optimistic locally, then re-read: a failed save must
	// not leave the sidebar claiming a pin the server does not have.
	const toggleThreadPin = async (threadId: string, pinned: boolean) => {
		setHistoryThreads((threads) =>
			threads.map((thread) =>
				thread.threadId === threadId ? { ...thread, pinned } : thread,
			),
		);
		try {
			await saveChatThreadSnapshot(threadId, { pinned });
		} catch {
			setHistoryError(
				i18n.tr(
					"Kunne ikke feste samtalen.",
					"Could not pin the conversation.",
				),
			);
		}
		await refreshChatHistory();
	};

	const openChatThread = (threadId: string) => {
		selectChatThread(threadId);
		setHistoryOpen(false);
		if (window.location.pathname !== "/chat") navigateToChat(navigate);
	};

	const resetComposerDraft = () => {
		flush(() => {
			props.onMessageChange("");
			clearFiles();
			setActiveActions([]);
			closeSecondaryPanels();
			setModelOpen(false);
			setEntities([]);
		});
	};

	const openHistoryPanel = () => {
		const nextOpen = !historyOpen();
		if (nextOpen) {
			const rect = historyTriggerRef?.getBoundingClientRect();
			if (rect) {
				setHistoryPanelPosition({
					bottom: window.innerHeight - rect.top + 8,
					maxHeight: Math.max(180, Math.min(400, rect.top - 16)),
					right: window.innerWidth - rect.right,
				});
			}
			void refreshChatHistory();
		}

		setHistoryOpen(nextOpen);
		setSettingsOpen(false);
		setModelOpen(false);
		setSuggestionsOpen(false);
		setContextScopeOpen(false);
		setAutocomplete(null);
	};

	const openSettingsPanel = () => {
		if (!settingsOpen()) {
			const rect = settingsTriggerRef?.getBoundingClientRect();
			if (rect) {
				const width = 256;
				const gap = 8;
				const left = Math.min(
					rect.right + gap,
					window.innerWidth - width - gap,
				);
				setSettingsPanelPosition({
					left: Math.max(gap, left),
					maxHeight: Math.max(
						220,
						Math.min(460, window.innerHeight - rect.top - 16),
					),
					top: rect.top,
				});
			}
		}

		setSettingsOpen((current) => !current);
		setHistoryOpen(false);
		setModelOpen(false);
		setSuggestionsOpen(false);
		setContextScopeOpen(false);
		setAutocomplete(null);
	};

	const updateAutocomplete = (text: string, position: number) => {
		const trigger = detectTrigger(text, position);
		setAutocompleteIndex(0);

		if (!trigger) {
			setAutocomplete(null);
			return;
		}

		if (trigger.type === "date") {
			const items = getUpcomingDates(trigger.dayIndex, i18n);
			setAutocomplete(
				items.length > 0
					? {
							category: i18n.tr("Plan", "Schedule"),
							items,
							triggerStart: trigger.start,
							triggerLen: trigger.rawLen,
						}
					: null,
			);
			return;
		}

		if (trigger.type === "slash") {
			ensureActionsLoaded();
			openActionMenu(trigger.query, trigger.start, trigger.rawLen);
			return;
		}

	};

	const handleMessageInput = (value: string, cursorPosition: number) => {
		if (modeAnnouncement()) {
			setModeAnnouncement(null);
			if (modeAnnouncementTimer) {
				window.clearTimeout(modeAnnouncementTimer);
				modeAnnouncementTimer = undefined;
			}
		}

		props.onMessageChange(value);
		updateAutocomplete(value, cursorPosition);
		setEntities((current) =>
			current.filter((entity) => {
				const end = entity.start + entity.text.length;
				return (
					end <= value.length &&
					value.slice(entity.start, end) === entity.text
				);
			}),
		);
	};

	const autocompleteTriggerParts = (state: AutocompleteState) => ({
		after: props.message.slice(state.triggerStart + state.triggerLen),
		before: props.message.slice(0, state.triggerStart),
	});

	const clearAutocompleteTrigger = (state: AutocompleteState) => {
		const parts = autocompleteTriggerParts(state);
		props.onMessageChange(parts.before + parts.after);
		setAutocomplete(null);
		return parts.before.length;
	};

	const selectSpecializedAutocomplete = (
		state: AutocompleteState,
		action: SpecializedAction,
	) => {
		const caret = clearAutocompleteTrigger(state);
		activateSpecialized(action);
		focusTextareaAtNextFrame(caret);
	};

	const selectFileAutocompleteCommand = (state: AutocompleteState) => {
		pendingFilePosition = state.triggerStart;
		clearAutocompleteTrigger(state);
		openFileDialog();
	};

	const selectImageAutocompleteCommand = (state: AutocompleteState) => {
		const parts = autocompleteTriggerParts(state);
		const prefix = "/image ";
		props.onMessageChange(`${parts.before}${prefix}${parts.after}`);
		props.onImageModeChange?.(true);
		setAutocomplete(null);
		focusTextareaAtNextFrame(parts.before.length + prefix.length);
	};

	const insertAutocompleteItem = (
		state: AutocompleteState,
		item: AutocompleteItem,
	) => {
		const parts = autocompleteTriggerParts(state);
		const inserted = item.label;
		props.onMessageChange(parts.before + inserted + parts.after);

		const meta = item.meta;
		if (meta === "date" || meta === "file") {
			setEntities((current) => [
				...current,
				{ kind: meta, start: state.triggerStart, text: inserted },
			]);
		}

		setAutocomplete(null);
		focusTextareaAtNextFrame(state.triggerStart + inserted.length);
	};

	const applyAutocompleteSelection = (item: AutocompleteItem) => {
		const state = autocomplete();
		if (!state) return;

		if (item.specialized) {
			selectSpecializedAutocomplete(state, item.specialized);
			return;
		}

		const command = slashCommands.find(
			(candidate) => candidate.id === item.id,
		);
		if (command?.action === "file") {
			selectFileAutocompleteCommand(state);
			return;
		}
		if (command?.action === "image") {
			selectImageAutocompleteCommand(state);
			return;
		}

		insertAutocompleteItem(state, item);
	};

	const handleAutocompleteKeyDown = (
		event: KeyboardEvent & { currentTarget: HTMLTextAreaElement },
		state: AutocompleteState,
	) => {
		switch (event.key) {
			case "ArrowDown":
				event.preventDefault();
				setAutocompleteIndex((current) =>
					Math.min(current + 1, state.items.length - 1),
				);
				return true;
			case "ArrowUp":
				event.preventDefault();
				setAutocompleteIndex((current) => Math.max(current - 1, 0));
				return true;
			case "Enter":
			case "Tab": {
				event.preventDefault();
				const item = state.items[autocompleteIndex()];
				if (item) applyAutocompleteSelection(item);
				return true;
			}
			case "Escape":
				event.preventDefault();
				setAutocomplete(null);
				return true;
			default:
				return false;
		}
	};

	const handleComposerKeyDown = (
		event: KeyboardEvent & { currentTarget: HTMLTextAreaElement },
	) => {
		const state = autocomplete();
		if (state && handleAutocompleteKeyDown(event, state)) return;

		// Tab accepts the rotating suggestion. Deliberately only when the
		// composer is empty and a suggestion is actually showing: `Tab` is the
		// keyboard user's way out of a textarea, and swallowing it whenever
		// this component has focus would trap them. `activeSuggestion()` is
		// already null once anything is typed, so the guard is the same one the
		// placeholder uses.
		if (event.key === "Tab" && !event.shiftKey) {
			const suggestion = activeSuggestion();
			if (suggestion) {
				event.preventDefault();
				props.onMessageChange(suggestion);
				textareaRef?.focus();
				return;
			}
		}

		if (event.key === "Enter" && !event.shiftKey) {
			event.preventDefault();
			void submitComposer();
		}
	};

	const handleResponseMode = (mode: ResponseMode) => {
		if (mode === responseMode()) return;

		const nextMode = responseModes.find(
			(candidate) => candidate.id === mode,
		);
		setResponseMode(mode);
		if (nextMode) {
			setModeAnnouncement(
				i18n.tr(nextMode.announcement.no, nextMode.announcement.en),
			);
			if (modeAnnouncementTimer)
				window.clearTimeout(modeAnnouncementTimer);
			modeAnnouncementTimer = window.setTimeout(
				() => setModeAnnouncement(null),
				3000,
			);
		}
	};

	const createSubmissionSnapshot = () => {
		const now = new Date();
		const body = props.message.trim();
		const submittedText =
			body ||
			i18n.tr(
				"Vedlegg sendt til Verevon.",
				"Attachment sent to Verevon.",
			);
		return {
			actions: activeActions(),
			body,
			files: files(),
			now,
			submittedText,
		};
	};

	const submitComposer = async () => {
		if (props.disabled) return;
		if (
			autocomplete() ||
			!hasContent() ||
			voiceMode() ||
			voiceRecording() ||
			(props.submitting && !props.allowMidRunSubmit)
		)
			return;

		const snapshot = createSubmissionSnapshot();
		const productResearch = shouldResearchLinkedProducts({
			browseWeb: browseWeb(),
			message: snapshot.body,
		});
		const effectiveDeepSearch = deepSearch() || productResearch;
		const subscriptionRoute = selectedSubscriptionRoute();
		const payload = createComposerSubmitPayload({
			actions: snapshot.actions,
			browseWeb: browseWeb(),
			deepSearch: effectiveDeepSearch,
			files: snapshot.files,
			imageMode: imageMode(),
			minPrivacyTier: selectedPrivacyTier(),
			model: selectedModel(),
			provider: subscriptionRoute?.provider,
			responseMode: responseMode(),
			subscriptionConnectionId: subscriptionRoute?.connectionId,
			text: snapshot.submittedText,
			tone: settings().tone,
			trimmedMessage: snapshot.body,
			zdr: temporaryChat(),
		});

		setTurns((current) =>
			[
				createComposerTurn({
					body: snapshot.submittedText,
					browseWeb: browseWeb(),
					deepSearch: effectiveDeepSearch,
					files: snapshot.files,
					model: selectedModelLabel(),
					now: snapshot.now,
					responseMode: responseMode(),
				}),
				...current,
			].slice(0, 6),
		);

		if (props.onSubmit) {
			await props.onSubmit(payload);
			resetComposerDraft();
			return;
		}

		await writePendingChatLaunch(payload);
		resetComposerDraft();
		props.onLaunchStart?.();
		// Navigate immediately inside a View Transition: the composer carries
		// `view-transition-name: verevon-composer`, so it morphs (travels + resizes) in
		// place into the chat page's composer instead of fading out first and
		// crossfading the whole route. Reduced-motion users get a plain navigate.
		navigateToChat(navigate);
	};

	onCleanup(() => {
		mediaRecorder?.stream.getTracks().forEach((track) => track.stop());
		if (modeAnnouncementTimer) window.clearTimeout(modeAnnouncementTimer);
		files().forEach(revokeFilePreviewUrl);
	});

	return (
		<div
			ref={(element) => {
				composerRootRef = element;
			}}
			class={dashboardComposerRootClass({ appearance: props.appearance, dragActive: dragActive() })}
			aria-disabled={props.disabled ? "true" : "false"}
			inert={props.disabled}
			onDragOver={(event) => {
				if (dragEventHasFiles(event)) {
					event.preventDefault();
					setDragActive(true);
				}
			}}
			onDragLeave={(event) => {
				if (
					!event.currentTarget.contains(
						event.relatedTarget as Node | null,
					)
				) {
					setDragActive(false);
				}
			}}
			onDrop={(event) => {
				const dropped = droppedFiles(event);
				if (dropped.length > 0) {
					event.preventDefault();
					addFiles(dropped);
				}
				setDragActive(false);
			}}
			onPaste={(event) => {
				const images = pastedImageFiles(event);
				if (images.length > 0) {
					event.preventDefault();
					addFiles(images);
				}
			}}
		>
			<Show when={dragActive()}>
				<div class="dashboard-composer-drop-overlay">
					{i18n.tr("Slipp for å legge ved", "Drop to attach")}
				</div>
			</Show>

			<Show when={props.appearance === "chat" && shouldResearchLinkedProducts({ browseWeb: browseWeb(), message: props.message }) && !deepSearch()}>
				<div class="dashboard-composer-research-notice" role="status">
					<Telescope class="size-3.5" />
					<span>{i18n.tr("Produktlenker oppdaget — Verevon undersøker flere kilder per produkt.", "Product links detected — Verevon will research multiple sources per product.")}</span>
				</div>
			</Show>

			<div class="dashboard-composer-controls">
				<div class="dashboard-composer-controls__left">
					<div class="dashboard-composer-model-wrap">
						<button
							type="button"
							aria-expanded={modelOpen() ? "true" : "false"}
							aria-label={i18n.tr(
								"Velg AI-modell",
								"Choose AI model",
							)}
							title={i18n.tr("Velg AI-modell", "Choose AI model")}
							onClick={() => {
								setModelOpen((current) => !current);
								closeSecondaryPanels();
							}}
							class="dashboard-composer-model-button verevon-composer-control"
						>
							<Zap class="size-4 dashboard-composer-model-button__zap" />
							<span>{selectedModelLabel()}</span>
							<ChevronDown
								class={composerChevronClass(modelOpen())}
							/>
						</button>

						<Show when={modelOpen()}>
							<div class="dashboard-composer-model-menu verevon-popover">
								<div class="dashboard-composer-model-group">
									<p class="dashboard-composer-model-group__label">
										Verevon
									</p>
									<For each={VEREVON_MODES}>
										{(mode) => (
											<button
												type="button"
												title={`${mode.label} — ${mode.description}`}
												onClick={() =>
													selectModel(mode.id)
												}
											class={{
												"dashboard-composer-model-menu__item--active":
													!selectedModelProvider() && selectedModel() ===
													mode.id,
												}}
											>
												<span>
													<span>
														<Zap class="size-3" />
													</span>
													<span>{mode.label}</span>
												</span>
												<span class="dashboard-composer-model-menu__right">
													<Show
														when={
															mode.badge ===
															"premium"
														}
														fallback={
															<span class="dashboard-composer-model-badge dashboard-composer-model-badge--cheap">
																{i18n.tr(
																	"Rimelig",
																	"Low cost",
																)}
															</span>
														}
													>
														<span
															class="dashboard-composer-model-badge dashboard-composer-model-badge--premium"
															title={i18n.tr(
																"Premium - beste svar, dyrere",
																"Premium - best answers, more expensive",
															)}
														>
															$$
														</span>
													</Show>
													<Show
												when={
													!selectedModelProvider() && selectedModel() ===
													mode.id
														}
													>
														<Check class="size-4" />
													</Show>
												</span>
											</button>
										)}
									</For>
								</div>

								{/* Design section 3.5: the composer offers an effort dial and
								    nothing else, and the pinned Verevon modes above "just need to
								    stop exposing the raw provider catalog underneath". UX spec
								    section 6 keeps advanced model transparency as a secondary
								    control, so the catalog is one deliberate click away rather
								    than deleted -- picking a tiered model is how a privacy tier
								    reaches the payload (audit item 20). */}
								<details class="dashboard-composer-model-advanced">
									<summary>
										{i18n.tr('Velg modell selv', 'Choose a model yourself')}
									</summary>
									<div
										class="dashboard-composer-model-catalog"
										role="region"
										aria-label={i18n.tr(
											"Tilgjengelige AI-modeller",
											"Available AI models",
										)}
										tabindex={0}
									>
										<For each={chatModelGroups()}>
											{(group) => (
												<div class="dashboard-composer-model-group">
													<p class="dashboard-composer-model-group__label">
												{group.label}
													</p>
											<For each={group.models}>
												{(model) => (
													<button
														type="button"
														title={i18n.tr(
															`Bruk ${model.name}`,
															`Use ${model.name}`,
														)}
																onClick={() =>
																	selectModel(model.id, model.provider)
																}
																class={{
																	"dashboard-composer-model-menu__item--active":
																		selectedModel() ===
																			model.id &&
																		selectedModelProvider() === model.provider,
																}}
													>
														<span>
															<span>
																<Zap class="size-3" />
															</span>
															<span>
																{model.name}
															</span>
														</span>
														<span class="dashboard-composer-model-menu__right">
															{/* `unspecified` states nothing — no badge for it either.
																Evaluates to the tier or false so the Show callback narrows. */}
															<Show
																when={isSelectablePrivacyTier(model.privacyTier)
																	? model.privacyTier
																	: false}
															>
																{(tier) => (
																	<span
																		class={cn(
																			"dashboard-composer-model-badge",
																			privacyTierBadgeClass(tier()),
																		)}
																		data-tone={
																			isClaimedPrivacyTier(tier())
																				? "claimed"
																				: "plain"
																		}
																		title={privacyTierBadgeTitle(
																			i18n,
																			tier(),
																		)}
																	>
																		{privacyTierBadgeLabel(i18n, tier())}
																	</span>
																)}
															</Show>
															<Show when={isExpensiveModel(model)}>
																<span
																	class="dashboard-composer-model-badge dashboard-composer-model-badge--premium"
																	title={i18n.tr(
																		"Dyrere modell - velg bevisst",
																		"More expensive model - choose deliberately",
																	)}
																>
																	$$
																</span>
															</Show>
															<Show
																when={selectedModel() === model.id && selectedModelProvider() === model.provider}
															>
																<Check class="size-4" />
															</Show>
														</span>
													</button>
												)}
											</For>
												</div>
											)}
										</For>
									</div>
								</details>
								<Show when={selectedPrivacyTier() === "sovereign"}>
									<p class="dashboard-composer-model-tier-note" role="note">
										{sovereignCatalogNotice(i18n)}
									</p>
								</Show>
							</div>
						</Show>
				</div>

					<a
						link
						href="/agents"
						title={i18n.tr("Opprett agent", "Create agent")}
						class="dashboard-composer-agent-button verevon-composer-control"
						onClick={() => {
							props.onMessageChange(
								i18n.tr(
									"Opprett en agent som håndterer kundesamtaler med kunnskapsbase, tone og eskaleringer.",
									"Create an agent that handles customer conversations with a knowledge base, tone, and escalations.",
								),
							);
							setSuggestionsOpen(true);
							setModelOpen(false);
						}}
					>
						<Sparkles class="dashboard-composer-agent-button__icon" />
						<span>{i18n.tr("Opprett agent", "Create agent")}</span>
					</a>
				</div>

				<div class="dashboard-composer-controls__right">
					<span class="dashboard-composer-context-wrap">
													<ComposerIconButton
							active={contextScopeOpen()}
							expanded={contextScopeOpen()}
													label={i18n.tr(
														`Kontekst for dette svaret: ${browseWeb() ? "kunnskap og nett" : "kunnskap"}`,
														`Context for this answer: ${browseWeb() ? "knowledge and web" : "knowledge"}`,
													)}
							onClick={() => {
								setContextScopeOpen((current) => !current);
								setModelOpen(false);
								setHistoryOpen(false);
								setSettingsOpen(false);
								setSuggestionsOpen(false);
							}}
							variant="chip"
													>
														<BookOpen class="size-3.5" />
													</ComposerIconButton>
													<span class="dashboard-composer-context-label" aria-hidden="true">
														{browseWeb() ? i18n.tr("Kunnskap + nett", "Knowledge + web") : i18n.tr("Kunnskap", "Knowledge")}
													</span>
						<Show when={contextScopeOpen()}>
							<div class="dashboard-composer-context-panel verevon-popover" role="dialog" aria-label={i18n.tr("Kontekst for svaret", "Answer context")}>
								<div class="dashboard-composer-context-panel__heading">
									<BookOpen class="size-4" />
									<strong>{i18n.tr("Kontekst", "Context")}</strong>
								</div>
								<p>{i18n.tr("Dette er hva Verevon kan bruke i denne meldingen.", "This is what Verevon can use for this message.")}</p>
								<ul class="dashboard-composer-context-panel__list">
									<li>
										<span class="dashboard-composer-context-dot dashboard-composer-context-dot--on" />
										<span><strong>{i18n.tr("Organisasjonskunnskap", "Organisation knowledge")}</strong><small>{i18n.tr("Tillatte kilder brukes når de gir treff.", "Permitted sources are used when they provide a match.")}</small></span>
									</li>
									<li>
										<span class={cn("dashboard-composer-context-dot", browseWeb() ? "dashboard-composer-context-dot--on" : "dashboard-composer-context-dot--off")} />
										<span><strong>{i18n.tr("Nettkilder", "Web sources")}</strong><small>{browseWeb() ? i18n.tr("Søk er aktivert.", "Search is enabled.") : i18n.tr("Slå på Søk for å hente fra nettet.", "Turn on Search to use the web.")}</small></span>
									</li>
									<li>
										<span class={cn("dashboard-composer-context-dot", files().length > 0 ? "dashboard-composer-context-dot--on" : "dashboard-composer-context-dot--off")} />
										<span><strong>{i18n.tr("Vedlegg", "Attachments")}</strong><small>{files().length > 0
											? modelAttachmentCount() > 0
												? i18n.tr(
													`${modelAttachmentCount()} bilde${modelAttachmentCount() === 1 ? "" : "r"} sendes til modellen${previewOnlyAttachmentCount() > 0 ? ` · ${previewOnlyAttachmentCount()} fil${previewOnlyAttachmentCount() === 1 ? "" : "er"} vises bare her` : ""}.`,
													`${modelAttachmentCount()} image${modelAttachmentCount() === 1 ? "" : "s"} will be sent to the model${previewOnlyAttachmentCount() > 0 ? ` · ${previewOnlyAttachmentCount()} file${previewOnlyAttachmentCount() === 1 ? "" : "s"} are preview-only here` : ""}.`,
												)
												: i18n.tr(
													`${previewOnlyAttachmentCount()} fil${previewOnlyAttachmentCount() === 1 ? "" : "er"} vises bare her; last opp til kunnskapsbasen hvis du vil lagre dem.`,
													`${previewOnlyAttachmentCount()} file${previewOnlyAttachmentCount() === 1 ? " is" : "s are"} preview-only here; upload to knowledge if you want to save them.`,
												)
											: i18n.tr("Ingen filer lagt ved.", "No files attached.")}</small></span>
									</li>
									<li>
										<span class={cn("dashboard-composer-context-dot", deepSearch() ? "dashboard-composer-context-dot--on" : "dashboard-composer-context-dot--off")} />
										<span><strong>{i18n.tr("Dyp research", "Deep research")}</strong><small>{deepSearch() ? i18n.tr("Utvidet research er aktivert.", "Extended research is enabled.") : i18n.tr("Ikke aktivert.", "Not enabled.")}</small></span>
									</li>
								</ul>
							</div>
						</Show>
					</span>
					<Show when={props.onPlanModeChange}>
						<Show
							when={props.appearance === "chat"}
							fallback={
								<span class={cn("dashboard-composer-intent-toggle", planMode() ? "dashboard-composer-intent-toggle--active" : "")}>
									<ComposerIconButton
										active={planMode()}
										label={i18n.tr(
											planMode()
												? "Do-modus - agenten planlegger og ber om godkjenning før risikable verktøy"
												: "Ask-modus - les, hent og foreslå uten å utføre risikable handlinger",
											planMode()
												? "Do mode - the agent plans and asks for approval before risky tools"
												: "Ask mode - read, retrieve, and propose without executing risky actions",
										)}
										onClick={() => props.onPlanModeChange?.(!planMode())}
										variant="chip"
									>
										<WandSparkles class="size-3.5" />
									</ComposerIconButton>
									<span class="dashboard-composer-intent-toggle__label" aria-live="polite">
										{planMode() ? "Do" : "Ask"}
									</span>
								</span>
							}
						>
							<div class="dashboard-composer-intent-segment" role="group" aria-label={i18n.tr("Arbeidsmodus", "Work mode")}>
								<button
									type="button"
									aria-pressed={!planMode() ? "true" : "false"}
									class={{ "is-active": !planMode() }}
									onClick={() => props.onPlanModeChange?.(false)}
								>
									{i18n.tr("Spør", "Ask")}
								</button>
								<button
									type="button"
									aria-pressed={planMode() ? "true" : "false"}
									class={{ "is-active": planMode() }}
									onClick={() => props.onPlanModeChange?.(true)}
								>
									{i18n.tr("Utfør", "Do")}
								</button>
							</div>
						</Show>
					</Show>
					<Show when={props.onTemporaryChatChange}>
						<ComposerIconButton
							active={temporaryChat()}
							disabled={
								props.temporaryChatLocked ||
								temporaryChatUnavailable()
							}
							label={
								temporaryChatUnavailable()
									? i18n.tr(
											"Midlertidig samtale er utilgjengelig – ingen av modellene i katalogen er attestert for null datalagring",
											"Temporary chat is unavailable – no model in the catalogue is attested for zero data retention",
										)
									: i18n.tr(
											"Midlertidig samtale – ingen historikk, ingen minne",
											"Temporary chat – no history, no memory",
										)
							}
							onClick={() =>
								props.onTemporaryChatChange?.(!temporaryChat())
							}
							variant="chip"
						>
							<EyeOff class="size-3.5" />
						</ComposerIconButton>
					</Show>
					<span
						ref={(element) => {
							historyTriggerRef = element;
						}}
					>
						<ComposerIconButton
							active={historyOpen()}
							label={i18n.tr("Historikk", "History")}
							onClick={openHistoryPanel}
							variant="chip"
						>
							<Clock3 class="size-3.5" />
						</ComposerIconButton>
					</span>
					<span
						ref={(element) => {
							settingsTriggerRef = element;
						}}
					>
						<ComposerIconButton
							active={settingsOpen()}
							label={i18n.tr("Innstillinger", "Settings")}
							onClick={openSettingsPanel}
							variant="chip"
						>
							<SlidersHorizontal class="size-3.5" />
						</ComposerIconButton>
					</span>
				</div>
			</div>

			<div class="dashboard-composer-field-wrap">
				<input
					ref={(element) => {
						fileInputRef = element;
					}}
					type="file"
					accept="image/*,.pdf,.docx,.txt,.md,.markdown,.csv,.json,.html,.htm"
					multiple
					class="sr-only"
					aria-label={i18n.tr("Legg til filer", "Add files")}
					onChange={(event) => {
						if (event.currentTarget.files)
							addFiles(event.currentTarget.files);
						event.currentTarget.value = "";
					}}
				/>

				<Show when={suggestionsOpen()}>
					<div class="dashboard-composer-suggestions verevon-popover verevon-popover-up">
						<button
							type="button"
							onClick={() =>
								props.onMessageChange(
									i18n.tr(
										"Finn de viktigste kundesakene fra siste uke.",
										"Find the most important customer cases from last week.",
									),
								)
							}
						>
							{i18n.tr(
								"Finn de viktigste kundesakene fra siste uke.",
								"Find the most important customer cases from last week.",
							)}
						</button>
						<button
							type="button"
							onClick={() =>
								props.onMessageChange(
									i18n.tr(
										"Lag et kort svarutkast med kildehenvisninger.",
										"Create a short draft reply with source references.",
									),
								)
							}
						>
							{i18n.tr(
								"Lag et kort svarutkast med kildehenvisninger.",
								"Create a short draft reply with source references.",
							)}
						</button>
						<button
							type="button"
							onClick={() =>
								props.onMessageChange(
									i18n.tr(
										"Oppsummer kunnskapsbasen og pek på mangler.",
										"Summarize the knowledge base and point out gaps.",
									),
								)
							}
						>
							{i18n.tr(
								"Oppsummer kunnskapsbasen og pek på mangler.",
								"Summarize the knowledge base and point out gaps.",
							)}
						</button>
					</div>
				</Show>

				<Show when={autocomplete()}>
					{(state) => (
						<div class="dashboard-composer-autocomplete">
							<AutocompleteDropdown
								category={state().category}
								items={state().items}
								selectedIndex={autocompleteIndex()}
								onHover={setAutocompleteIndex}
								onSelect={applyAutocompleteSelection}
							/>
						</div>
					)}
				</Show>

				<form
					class="verevon-dashboard-composer-card dashboard-composer-card verevon-composer-shell"
					onSubmit={(event) => {
						event.preventDefault();
						void submitComposer();
					}}
				>
					<AttachmentPreview
						attachments={files()}
						i18n={i18n}
						onEnhance={enhanceAttachments}
						onKnowledgeImport={props.onKnowledgeImport ? importFilesToKnowledge : undefined}
						onRemove={removeFile}
						knowledgeImportDisabled={temporaryChat() || Boolean(props.disabled)}
						knowledgeImporting={knowledgeImporting()}
						knowledgeImportNotice={knowledgeImportNotice()}
					/>

					<Show when={activeActions().length > 0}>
						<div class="dashboard-composer-actions-bar">
							<For each={activeActions()}>
								{(action) => (
									<span
										class={cn(
											"dashboard-composer-action-chip",
											`dashboard-composer-action-chip--${action.kind}`,
										)}
									>
										<Dynamic
											component={iconForKind(action.kind)}
											class="size-3"
										/>
										<span>{action.name}</span>
										<button
											type="button"
											aria-label={i18n.tr(
												`Fjern ${action.name}`,
												`Remove ${action.name}`,
											)}
											title={i18n.tr(
												`Fjern ${action.name}`,
												`Remove ${action.name}`,
											)}
											onClick={() =>
												removeActiveAction(action)
											}
										>
											<X class="size-2.5" />
										</button>
									</span>
								)}
							</For>
						</div>
					</Show>

					<div class="dashboard-composer-textarea-wrap">
						<Show when={hasEntityOverlay()}>
							<div
								class="dashboard-composer-entity-overlay"
								aria-hidden="true"
							>
								<EntityOverlay
									entities={entities()}
									message={props.message}
								/>
							</div>
						</Show>
						<Show when={currentModeAnnouncement()}>
							<div
								class="dashboard-composer-mode-announcement"
								aria-hidden="true"
							>
								<SplitText text={currentModeAnnouncement()} />
							</div>
						</Show>
						<label class="sr-only" for="verevon-dashboard-input">
							{i18n.tr("Meld Verevon", "Message Verevon")}
						</label>
						<textarea
							ref={(element) => {
								textareaRef = element;
							}}
							id="verevon-dashboard-input"
							aria-label={i18n.tr(
								"Meld Verevon",
								"Message Verevon",
							)}
							class="verevon-dashboard-textarea"
							aria-busy={voiceRecording() ? "true" : "false"}
							placeholder={textareaPlaceholder(
								modeAnnouncement(),
								i18n,
								activeSuggestion(),
							)}
							rows="3"
							style={textareaOverlayStyle(hasEntityOverlay())}
							value={props.message}
							onInput={(event) =>
								handleMessageInput(
									event.currentTarget.value,
									textareaCursorPosition(event.currentTarget),
								)
							}
							onKeyDown={handleComposerKeyDown}
							disabled={textareaLocked()}
						/>
						<Show when={showTextareaExpand()}>
							<button
								type="button"
								class="dashboard-composer-expand-btn"
								aria-label={textareaExpandLabel(
									textareaExpanded(),
									i18n,
								)}
								title={textareaExpandTitle(
									textareaExpanded(),
									i18n,
								)}
								onClick={() =>
									setTextareaExpanded((prev) => !prev)
								}
							>
								<Show
									when={textareaExpanded()}
									fallback={
										<Maximize2 size={11} strokeWidth={2} />
									}
								>
									<Minimize2 size={11} strokeWidth={2} />
								</Show>
							</button>
						</Show>
					</div>

					<Show when={voiceRecording()}>
						<div class="dashboard-composer-loading-bar">
							<span />
						</div>
					</Show>

					<div class="dashboard-composer-toolbar">
						<div class="dashboard-composer-toolbar__left">
							<button
								type="button"
								class="dashboard-composer-attach"
								aria-label={i18n.tr(
									"Legg til filer",
									"Add files",
								)}
								title={i18n.tr("Legg til filer", "Add files")}
								onClick={openFileDialog}
							>
								<span>
									<CirclePlus class="size-4" />
								</span>
								<span>
									{i18n.tr("legg til filer", "add files")}
								</span>
							</button>
							<div class="dashboard-composer-toolbar__divider" />
							<ComposerIconButton
								active={suggestionsOpen()}
								label={i18n.tr("Forslag", "Suggestions")}
								onClick={() =>
									{
										setSuggestionsOpen((current) => !current)
										setContextScopeOpen(false)
									}
								}
								variant="toolbar"
							>
								<Lightbulb class="size-4" />
							</ComposerIconButton>
							<ComposerIconButton
								active={deepSearch()}
								label={i18n.tr("Dyp research", "Deep search")}
								onClick={() =>
									setDeepSearch((current) => !current)
								}
								variant="toolbar"
							>
								<Telescope class="size-4" />
							</ComposerIconButton>
							<button
								type="button"
								aria-pressed={browseWeb() ? "true" : "false"}
								aria-label={i18n.tr(
									"Søk på nett",
									"Browse web",
								)}
								title={i18n.tr("Søk på nett", "Browse web")}
								onClick={() =>
									setBrowseWeb((current) => !current)
								}
								class={composerWebButtonClass(browseWeb())}
							>
								<Globe2 class="size-4" />
								{i18n.tr("Søk", "Search")}
							</button>
							<Show when={props.onImageModeChange}>
								<button
									type="button"
									aria-pressed={imageMode() ? "true" : "false"}
									aria-label={i18n.tr(
										"Generer bilde",
										"Generate image",
									)}
									title={i18n.tr(
										"Generer bilde",
										"Generate image",
									)}
									onClick={() =>
										props.onImageModeChange?.(!imageMode())
									}
									class={composerImageButtonClass(
										imageMode(),
									)}
								>
									<ImagePlus class="size-4" />
									{i18n.tr("Bilde", "Image")}
								</button>
							</Show>
						</div>

						<div class="dashboard-composer-toolbar__right">
							<Show
								when={props.appearance === "chat"}
								fallback={
									<div class="dashboard-composer-response-group">
										<For each={responseModes}>
											{(mode) => (
												<ComposerIconButton
													active={responseMode() === mode.id}
													label={i18n.tr(mode.label.no, mode.label.en)}
													onClick={() => handleResponseMode(mode.id)}
													variant="toolbar"
												>
													<Dynamic component={mode.icon} class="size-4" />
												</ComposerIconButton>
											)}
										</For>
									</div>
								}
							>
								<label class="dashboard-composer-effort-select">
									<SlidersHorizontal class="size-3.5" aria-hidden="true" />
									<span class="sr-only">{i18n.tr("Svarinnsats", "Response effort")}</span>
									<select
										aria-label={i18n.tr("Svarinnsats", "Response effort")}
										value={responseMode()}
										onChange={(event) => handleResponseMode(event.currentTarget.value as ResponseMode)}
									>
										<option value="quick">{i18n.tr("Rask", "Quick")}</option>
										<option value="auto">{i18n.tr("Standard", "Standard")}</option>
										<option value="deep">{i18n.tr("Grundig", "Deep")}</option>
									</select>
								</label>
							</Show>
							<ComposerIconButton
								active={voiceMode()}
								label={i18n.tr("Stemmemodus", "Voice mode")}
								onClick={() => setVoiceMode(true)}
								variant="toolbar"
							>
								<AudioWaveform class="size-4" />
							</ComposerIconButton>
							<ComposerIconButton
								active={voiceRecording()}
								label={i18n.tr("Stemmeinndata", "Voice input")}
								onClick={() => void toggleRecording()}
								variant="toolbar"
							>
								<Mic class="size-4" />
							</ComposerIconButton>
							<Show
								when={stopButtonVisible()}
								fallback={
									<button
										type="submit"
										disabled={!submitEnabled()}
										aria-label={i18n.tr(
											"Send melding",
											"Send message",
										)}
										title={i18n.tr(
											"Send melding",
											"Send message",
										)}
										class={composerSubmitButtonClass(
											submitEnabled(),
										)}
									>
										<ArrowUp class="size-4" />
									</button>
								}
							>
								<button
									type="button"
									aria-label={i18n.tr(
										"Stopp svar",
										"Stop response",
									)}
									title={i18n.tr(
										"Stopp svar",
										"Stop response",
									)}
									onClick={() => props.onStop?.()}
									class="dashboard-composer-submit-button dashboard-composer-submit-button--active"
								>
									<Square class="size-3.5" />
								</button>
							</Show>
						</div>
					</div>
				</form>
			</div>

			<Show when={activeTurnReceipt()}>
				{(turn) => <TurnReceipt i18n={i18n} turn={turn()} />}
			</Show>

			{/* Portal floating panels/overlays to <body>: they use position:fixed and
          the composer subtree carries an identity transform (verevonPanelIn) that
          would otherwise become their containing block and mis-place them. */}
			<Show when={historyOpen()}>
				<Portal>
					<HistoryPanel
						error={historyError()}
						i18n={i18n}
						loading={historyLoading()}
						position={historyPanelPosition()}
						threads={historyThreads()}
						turns={turns()}
						onClose={() => setHistoryOpen(false)}
						onThreadSelect={openChatThread}
						onTogglePin={toggleThreadPin}
					/>
				</Portal>
			</Show>

			<Show when={settingsOpen()}>
				<Portal>
					<SettingsPanel
						i18n={i18n}
						position={settingsPanelPosition()}
						settings={settings()}
						onAddFiles={() => {
							setSettingsOpen(false);
							openFileDialog();
						}}
						onScreenshot={() => {
							setSettingsOpen(false);
							void addScreenshotFile();
						}}
						onSettingsChange={setSettings}
					/>
				</Portal>
			</Show>
			<Show when={voiceMode()}>
				<Portal>
					<RealtimeVoiceModal
						i18n={i18n}
						language={settings().voiceLang}
						model={selectedModelLabel()}
						onClose={() => setVoiceMode(false)}
						onListeningChange={setVoiceRecording}
						onTranscript={(text) => {
							const trimmed = props.message.trim();
							props.onMessageChange(
								trimmed ? `${trimmed} ${text}` : text,
							);
						}}
					/>
				</Portal>
			</Show>
		</div>
	);
}

function iconForKind(kind: SpecializedActionKind): Component<LucideProps> {
	switch (kind) {
		case "skill":
			return WandSparkles;
		case "capability":
			return Blocks;
		case "connector":
			return Briefcase;
		case "builtin":
			return Sparkles;
	}
}

function iconForAction(action: SpecializedAction): Component<LucideProps> {
	if (action.kind === "builtin") {
		if (action.builtin === "file") return Upload;
		if (action.builtin === "image") return ImagePlus;
		if (action.builtin === "web_search") return Globe2;
		return Sparkles;
	}
	return iconForKind(action.kind);
}

function privacyTierBadgeClass(tier: PrivacyTier): string {
	return `dashboard-composer-model-badge--tier-${tier}`;
}

function getComposerTools(input: {
	browseWeb: boolean;
	deepSearch: boolean;
	imageMode: boolean;
	message: string;
	responseMode: ResponseMode;
}): DashboardComposerSubmitPayload["tools"] {
	const tools: DashboardComposerSubmitPayload["tools"] = [];
	if (input.browseWeb) tools.push("search");
	// `responseMode === "deep"` used to also push a `"reason"` tool. It never
	// reached the model: `SendOptions.tools` is consumed only to draw the user
	// turn's chips, `streamChat` has no `tools` key, and `createSelectedAgentToolSpecs`
	// drops bare strings anyway (they have no `.name`). The backend has no
	// `reason` tool either — "deep" is already honoured through the `effort`
	// field, which this same responseMode sets and which does reach the wire.
	// Removed rather than wired: it was a second, dead encoding of a dial that
	// already works.
	if (input.deepSearch) tools.push("research");
	if (
		input.imageMode ||
		input.message.trimStart().toLowerCase().startsWith("/image ")
	)
		tools.push("image");
	return [...new Set(tools)];
}

const PRODUCT_COMPARISON_WORDS = [
	"best",
	"better",
	"compare",
	"comparison",
	"recommend",
	"versus",
	" vs ",
	"hvilken",
	"sammenlign",
	"sammenlikn",
	"anbefal",
];

function shouldResearchLinkedProducts(input: { browseWeb: boolean; message: string }): boolean {
	if (!input.browseWeb) return false;
	const urls = input.message.match(/https?:\/\/[^\s<>()]+/giu) ?? [];
	const distinctLinks = new Set(urls.map((url) => url.replace(/[?#].*$/u, "").toLowerCase()));
	if (distinctLinks.size < 2) return false;
	const prose = input.message.replace(/https?:\/\/[^\s<>()]+/giu, " ");
	const normalized = ` ${prose.toLocaleLowerCase()} `;
	return PRODUCT_COMPARISON_WORDS.some((word) => normalized.includes(word));
}

function createComposerSubmitPayload(input: {
	actions: ComposerActiveAction[];
	browseWeb: boolean;
	deepSearch: boolean;
	files: ComposerFile[];
	imageMode: boolean;
	minPrivacyTier?: PrivacyTier;
	model: string;
	provider?: string;
	responseMode: ResponseMode;
	subscriptionConnectionId?: string;
	text: string;
	tone: ComposerTone;
	trimmedMessage: string;
	zdr: boolean;
}): DashboardComposerSubmitPayload {
	return {
		actions: input.actions,
		attachments: input.files.map((file) => ({
			id: file.id,
			name: file.name,
			size: file.size,
			type: file.type || "application/octet-stream",
			url: file.url,
		})),
			model: input.model || undefined,
			...(input.provider ? { provider: input.provider } : {}),
			...(input.subscriptionConnectionId
				? { subscriptionConnectionId: input.subscriptionConnectionId }
				: {}),
			// Key omitted entirely when no tier applies (intent modes, unspecified):
			// callers and tests treat presence as "the user pinned a tier".
			...(input.minPrivacyTier ? { minPrivacyTier: input.minPrivacyTier } : {}),
			text: input.text,
			tools: getComposerTools({
				browseWeb: input.browseWeb,
				deepSearch: input.deepSearch,
				imageMode: input.imageMode,
				message: input.trimmedMessage,
				responseMode: input.responseMode,
			}),
			zdr: input.zdr || undefined,
		// Presence-only, like minPrivacyTier: Auto omits the key so the wire
		// body stays byte-identical for the default mode.
		...(input.responseMode === "quick"
			? { effort: "quick" as const }
			: input.responseMode === "deep"
				? { effort: "deep" as const }
				: {}),
		// Presence-only for the same reason as effort: "balanced" is the
		// gateway's no-directive default, so sending it would be a no-op key on
		// every ordinary turn.
		...(input.tone === "concise" || input.tone === "detailed"
			? { tone: input.tone }
			: {}),
	};
}

function createComposerTurn(input: {
	body: string;
	browseWeb: boolean;
	deepSearch: boolean;
	files: ComposerFile[];
	model: string;
	now: Date;
	responseMode: ResponseMode;
}): ComposerTurn {
	return {
		id: `turn-${input.now.getTime()}`,
		body: input.body,
		browseWeb: input.browseWeb,
		createdAt: formatComposerTurnTime(input.now),
		createdAtIso: input.now.toISOString(),
		deepSearch: input.deepSearch,
		files: input.files.map((file) => file.name),
		model: input.model,
		responseMode: input.responseMode,
	};
}

function detectTrigger(text: string, position: number): TriggerContext | null {
	const before = text.slice(0, position);
	// `@` mentions are intentionally not handled in the global composer:
	// there is no global Control-owned roster or agent-ref contract here.
	// Space rooms own governed agent mentions, so silently searching the
	// knowledge endpoint and labelling those results as people would be a
	// misleading invocation control.

	const slashMatch = before.match(/\/(\w*)$/);
	if (slashMatch) {
		const query = slashMatch[1] ?? "";
		return {
			type: "slash",
			query: query.toLowerCase(),
			start: position - slashMatch[0].length,
			rawLen: slashMatch[0].length,
		};
	}

	const wordMatch = before.match(/\b([A-Za-zÆØÅæøå]{3,})$/);
	if (!wordMatch) return null;
	const word = wordMatch[1] ?? "";
	const normalizedWord = word.toLowerCase();
	const day = dayEntries.find(
		(entry) =>
			entry.name.toLowerCase().startsWith(normalizedWord) ||
			entry.nameNo.toLowerCase().startsWith(normalizedWord),
	);
	if (!day) return null;
	return {
		type: "date",
		dayIndex: day.dayIndex,
		start: position - word.length,
		rawLen: word.length,
	};
}

function getUpcomingDates(
	dayIndex: number,
	i18n: ReturnType<typeof useI18n>,
): AutocompleteItem[] {
	const items: AutocompleteItem[] = [];
	const date = new Date();

	while (items.length < 2) {
		date.setDate(date.getDate() + 1);
		if (date.getDay() === dayIndex) {
			const label = date.toLocaleDateString(
				i18n.locale() === "no" ? "nb-NO" : "en-US",
				{
					day: "numeric",
					month: "short",
					weekday: "short",
				},
			);
			items.push({
				id: `date-${label}`,
				icon: Calendar,
				label,
				meta: "date",
			});
		}
	}

	return items;
}

function EntityOverlay(props: { entities: EntityToken[]; message: string }) {
	const parts = () => {
		const output: JSX.Element[] = [];
		let position = 0;
		const validEntities = props.entities
			.filter(
				(entity) =>
					entity.start >= 0 &&
					entity.start + entity.text.length <= props.message.length &&
					props.message.slice(
						entity.start,
						entity.start + entity.text.length,
					) === entity.text,
			)
			.sort((a, b) => a.start - b.start);

		validEntities.forEach((entity) => {
			if (entity.start > position) {
				output.push(
					<span class="dashboard-composer-entity-text">
						{props.message.slice(position, entity.start)}
					</span>,
				);
			}
			output.push(
				<span
					class={cn(
						"dashboard-composer-entity-token",
						`dashboard-composer-entity-token--${entity.kind}`,
					)}
				>
					<Show when={entity.kind === "file"}>
						<span />
					</Show>
					{entity.text}
				</span>,
			);
			position = entity.start + entity.text.length;
		});

		if (position < props.message.length) {
			output.push(
				<span class="dashboard-composer-entity-text">
					{props.message.slice(position)}
				</span>,
			);
		}

		return output.length > 0 ? (
			output
		) : (
			<span>{props.message || "\u200b"}</span>
		);
	};

	return <>{parts()}</>;
}

function AutocompleteDropdown(props: {
	category: string;
	items: AutocompleteItem[];
	onHover: (index: number) => void;
	onSelect: (item: AutocompleteItem) => void;
	selectedIndex: number;
}) {
	return (
		<div
			class="dashboard-composer-autocomplete-menu verevon-popover verevon-popover-up"
			data-composer-floating-panel="true"
		>
			<div class="dashboard-composer-autocomplete-menu__category">
				{props.category}
			</div>
			<For each={props.items}>
				{(item, index) => (
					<button
						type="button"
						onMouseDown={(event) => {
							event.preventDefault();
							props.onSelect(item);
						}}
						onMouseEnter={() => props.onHover(index())}
						class={{
							"dashboard-composer-autocomplete-menu__item--active":
								index() === props.selectedIndex,
						}}
					>
						<Dynamic component={item.icon} class="size-4" />
						<span>{item.label}</span>
					</button>
				)}
			</For>
		</div>
	);
}

type SpeechResultLike = {
	readonly 0: { transcript: string };
	isFinal: boolean;
};
type SpeechEventLike = {
	resultIndex: number;
	results: ArrayLike<SpeechResultLike>;
};
type SpeechRecognitionLike = {
	continuous: boolean;
	interimResults: boolean;
	lang: string;
	onend: (() => void) | null;
	onerror: (() => void) | null;
	onresult: ((event: SpeechEventLike) => void) | null;
	start: () => void;
	stop: () => void;
};
type SpeechRecognitionCtor = new () => SpeechRecognitionLike;

function getSpeechRecognitionCtor(): SpeechRecognitionCtor | null {
	const win = window as unknown as {
		SpeechRecognition?: SpeechRecognitionCtor;
		webkitSpeechRecognition?: SpeechRecognitionCtor;
	};
	return win.SpeechRecognition ?? win.webkitSpeechRecognition ?? null;
}

function RealtimeVoiceModal(props: {
	i18n: ReturnType<typeof useI18n>;
	language: string;
	model: string;
	onClose: () => void;
	onListeningChange: (listening: boolean) => void;
	onTranscript: (text: string) => void;
}) {
	let recognition: SpeechRecognitionLike | null = null;
	const [interim, setInterim] = createSignal("");
	const [listening, setListening] = createSignal(false);
	const [transcript, setTranscript] = createSignal("");
	const supported = () => getSpeechRecognitionCtor() !== null;
	const liveText = () =>
		`${transcript()}${interim() ? ` ${interim()}` : ""}`.trim();

	const stop = () => {
		recognition?.stop();
		recognition = null;
		setListening(false);
		props.onListeningChange(false);
		setInterim("");
	};

	const startListening = () => {
		const Ctor = getSpeechRecognitionCtor();
		if (!Ctor) return;
		const nextRecognition = new Ctor();
		nextRecognition.lang = props.language || "nb-NO";
		nextRecognition.continuous = true;
		nextRecognition.interimResults = true;
		nextRecognition.onresult = (event) => {
			let finalChunk = "";
			let interimChunk = "";
			for (
				let index = event.resultIndex;
				index < event.results.length;
				index += 1
			) {
				const result = event.results[index];
				if (!result) continue;
				const text = result[0]?.transcript ?? "";
				if (result.isFinal) finalChunk += text;
				else interimChunk += text;
			}
			if (finalChunk) {
				setTranscript((current) =>
					current
						? `${current} ${finalChunk.trim()}`
						: finalChunk.trim(),
				);
			}
			setInterim(interimChunk);
		};
		nextRecognition.onend = () => {
			setListening(false);
			props.onListeningChange(false);
		};
		nextRecognition.onerror = () => {
			setListening(false);
			props.onListeningChange(false);
		};
		recognition = nextRecognition;
		setListening(true);
		props.onListeningChange(true);
		nextRecognition.start();
	};

	const close = () => {
		stop();
		setTranscript("");
		props.onClose();
	};

	const insert = () => {
		const text = liveText();
		if (text) props.onTranscript(text);
		close();
	};

	let dialogRef: HTMLDialogElement | undefined;
	onCleanup(() => {
		stop();
		// Close the native modal so it leaves the top layer cleanly on unmount.
		dialogRef?.close();
	});

	return (
		<dialog
			// Use the native modal (showModal) instead of the static `open` attribute
			// so the browser provides Escape-to-close, focus trapping, and an inert
			// backdrop — none of which a plain `<dialog open>` receives. Routing the
			// `cancel` event (Escape) through close() also stops the recogniser and
			// clears the transcript, matching the scrim/X buttons.
			ref={(el) => {
				dialogRef = el;
				queueMicrotask(() => {
					if (!el.open) el.showModal();
				});
			}}
			class="realtime-voice-modal"
			aria-label={props.i18n.tr("Stemmemodus", "Voice mode")}
			data-dashboard-modal="true"
			onCancel={(event: Event) => {
				event.preventDefault();
				close();
			}}
		>
			<button
				type="button"
				aria-label={props.i18n.tr(
					"Lukk stemmebakgrunn",
					"Dismiss voice backdrop",
				)}
				class="realtime-voice-modal__scrim"
				onClick={close}
			/>
			<div class="realtime-voice-modal__panel verevon-panel-in">
				<div class="realtime-voice-modal__header">
					<span>
						<AudioWaveform class="size-5" />
					</span>
					<div>
						<p>{props.i18n.tr("Stemmemodus", "Voice mode")}</p>
						<small>
							{props.model} · {props.language}
						</small>
					</div>
					<button
						type="button"
						onClick={close}
						aria-label={props.i18n.tr(
							"Lukk stemmemodus",
							"Close voice mode",
						)}
					>
						<X class="size-4" />
					</button>
				</div>

				<div class="realtime-voice-modal__body">
					<button
						type="button"
						onClick={() =>
							listening() ? stop() : startListening()
						}
						disabled={!supported()}
						class={cn(
							"realtime-voice-modal__mic",
							listening()
								? "realtime-voice-modal__mic--listening verevon-voice-pulse"
								: "",
						)}
						aria-label={
							listening()
								? props.i18n.tr(
										"Stopp diktering",
										"Stop dictation",
									)
								: props.i18n.tr(
										"Start diktering",
										"Start dictation",
									)
						}
					>
						<Mic class="size-6" />
					</button>
					<Show
						when={liveText()}
						fallback={
							<>
								<p>
									{listening()
										? props.i18n.tr(
												"Lytter ...",
												"Listening ...",
											)
										: supported()
											? props.i18n.tr(
													"Klar for diktering",
													"Ready for dictation",
												)
											: props.i18n.tr(
													"Stemme støttes ikke i denne nettleseren",
													"Voice is not supported in this browser",
												)}
								</p>
								<small>
									{supported()
										? props.i18n.tr(
												"Snakk fritt. Teksten settes inn i meldingen.",
												"Speak freely. The text will be inserted into the message.",
											)
										: props.i18n.tr(
												"Prøv Chrome/Edge, eller skriv meldingen i stedet.",
												"Try Chrome/Edge, or type the message instead.",
											)}
								</small>
							</>
						}
					>
						<p class="realtime-voice-modal__transcript">
							{transcript()}
							<Show when={interim()}>
								{(text) => <span> {text()}</span>}
							</Show>
						</p>
					</Show>
				</div>

				<div class="realtime-voice-modal__actions">
					<button
						type="button"
						onClick={() =>
							listening() ? stop() : startListening()
						}
						disabled={!supported()}
					>
						{listening()
							? props.i18n.tr("Stopp", "Stop")
							: props.i18n.tr(
									"Start diktering",
									"Start dictation",
								)}
					</button>
					<button
						type="button"
						onClick={insert}
						disabled={!liveText()}
					>
						{props.i18n.tr("Sett inn", "Insert")}
					</button>
				</div>
			</div>
		</dialog>
	);
}

function prefersReducedMotion() {
	return (
		typeof window.matchMedia === "function" &&
		window.matchMedia("(prefers-reduced-motion: reduce)").matches
	);
}

function navigateToChat(navigate: ReturnType<typeof useNavigate>) {
	const doc = document as Document & {
		startViewTransition?: (callback: () => void) => {
			finished: Promise<void>;
		};
	};

	if (
		!prefersReducedMotion() &&
		typeof doc.startViewTransition === "function"
	) {
		doc.startViewTransition(() => navigate("/chat"));
		return;
	}

	navigate("/chat");
}

function createFilePreviewUrl(file: File) {
	if (
		typeof URL === "undefined" ||
		typeof URL.createObjectURL !== "function"
	) {
		return "";
	}

	return URL.createObjectURL(file);
}

function revokeFilePreviewUrl(file: ComposerFile) {
	if (
		!file.url ||
		typeof URL === "undefined" ||
		typeof URL.revokeObjectURL !== "function"
	) {
		return;
	}

	URL.revokeObjectURL(file.url);
}

function formatFileSize(size: number) {
	if (size < 1024) return `${size} B`;
	if (size < 1024 * 1024) return `${Math.round(size / 1024)} KB`;
	return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

function AttachmentPreview(props: {
	attachments: ComposerFile[];
	i18n: ReturnType<typeof useI18n>;
	knowledgeImportDisabled?: boolean;
	knowledgeImporting?: boolean;
	knowledgeImportNotice?: string | null;
	onEnhance: () => void;
	onKnowledgeImport?: () => Promise<void> | void;
	onRemove: (id: string) => void;
}) {
	return (
		<Show when={props.attachments.length > 0}>
			<div class="verevon-attachment-preview dashboard-composer-attachments">
				<div class="dashboard-composer-attachments__list">
					<For each={props.attachments}>
						{(attachment) => (
							<div class="dashboard-composer-attachment">
								<Show
									when={
										attachment.type.startsWith("image/") &&
										attachment.url
									}
									fallback={
										<div class="dashboard-composer-attachment__file">
											<FileText class="size-5" />
											<span>{attachment.name}</span>
											<small>
												{formatFileSize(
													attachment.size,
												)}
											</small>
										</div>
									}
								>
									<img
										src={attachment.url}
										alt={attachment.name}
									/>
								</Show>
								<button
									type="button"
									onClick={() =>
										props.onRemove(attachment.id)
									}
									class="dashboard-composer-attachment__remove"
									aria-label={props.i18n.tr(
										`Fjern ${attachment.name}`,
										`Remove ${attachment.name}`,
									)}
									title={props.i18n.tr(
										`Fjern ${attachment.name}`,
										`Remove ${attachment.name}`,
									)}
								>
									<X class="size-2.5" />
								</button>
							</div>
						)}
					</For>
				</div>
				<button
					type="button"
					onClick={() => props.onEnhance()}
					class="dashboard-composer-attachments__enhance"
					aria-label={props.i18n.tr("AI-forbedre", "AI enhance")}
					title={props.i18n.tr("AI-forbedre", "AI enhance")}
				>
					<WandSparkles class="size-4" />
				</button>
				<Show when={props.onKnowledgeImport && props.attachments.some((file) => !file.type.startsWith("image/"))}>
					<button
						type="button"
						disabled={props.knowledgeImportDisabled || props.knowledgeImporting}
						onClick={() => void props.onKnowledgeImport?.()}
						class="dashboard-composer-attachments__knowledge"
						aria-label={props.i18n.tr("Legg til i kunnskapsbasen", "Add to knowledge base")}
						title={props.knowledgeImportDisabled
							? props.i18n.tr("Ikke tilgjengelig i midlertidig chat", "Unavailable in temporary chat")
							: props.i18n.tr("Lagre filene i organisasjonskunnskapen", "Save files to organisation knowledge")}
					>
						<Show when={!props.knowledgeImporting} fallback={<Loader2 class="size-4 animate-spin" />}>
							<Upload class="size-4" />
						</Show>
					</button>
				</Show>
			</div>
			<Show when={props.knowledgeImportNotice}>
				<p class="dashboard-composer-attachments__notice" role="status">{props.knowledgeImportNotice}</p>
			</Show>
		</Show>
	);
}

function responseModeReceiptLabel(
	mode: ResponseMode,
	i18n: ReturnType<typeof useI18n>,
): string {
	const option = responseModes.find((candidate) => candidate.id === mode);
	return option ? i18n.tr(option.label.no, option.label.en) : mode;
}

function TurnReceipt(props: {
	i18n: ReturnType<typeof useI18n>;
	turn: ComposerTurn;
}) {
	return (
		<div class="verevon-composer-turn-receipt verevon-fade-up">
			<div>
				<span>
					<ArrowUp class="size-3.5" />
				</span>
				<div>
					<p>{props.turn.body}</p>
					<small>
						{props.turn.model} ·{" "}
						{responseModeReceiptLabel(
							props.turn.responseMode,
							props.i18n,
						)}
						<Show when={props.turn.browseWeb}>
							{" "}
							· {props.i18n.tr("nett", "web")}
						</Show>
						<Show when={props.turn.deepSearch}>
							{" "}
							· {props.i18n.tr("dyp research", "deep search")}
						</Show>
					</small>
					<Show when={props.turn.files.length > 0}>
						<small>
							{props.i18n.tr(
								`${props.turn.files.length} vedlegg lagt til.`,
								`${props.turn.files.length} attachment${props.turn.files.length === 1 ? "" : "s"} added.`,
							)}
						</small>
					</Show>
				</div>
			</div>
		</div>
	);
}

function HistoryPanel(props: {
	error: string | null;
	i18n: ReturnType<typeof useI18n>;
	loading: boolean;
	onClose: () => void;
	onThreadSelect: (threadId: string) => void;
	onTogglePin: (threadId: string, pinned: boolean) => void;
	position: PanelPosition;
	threads: ChatThreadSession[];
	turns: ComposerTurn[];
}) {
	const [historyQuery, setHistoryQuery] = createSignal("");
	const allItems = createMemo(() => [
		...props.threads.map((thread) =>
			threadToHistoryItem(thread, props.i18n),
		),
		...props.turns.map((turn) => turnToHistoryItem(turn, props.i18n)),
	]);
	// Filters the ALREADY-LOADED list — the gateway returns up to 80 threads with
	// their title and preview, and this narrows those. It is a quick-switcher, not
	// full-text search: message BODIES are not here to match against, and searching
	// them needs a backend query over `messages.content` (the threads table has no
	// title/preview column at all — both are derived per query by LATERAL joins).
	// Scoped honestly in the placeholder so it does not read as a promise of more.
	const filteredItems = createMemo(() => {
		const needle = historyQuery().trim().toLowerCase();
		if (!needle) return allItems();
		return allItems().filter(
			(item) =>
				item.title.toLowerCase().includes(needle) ||
				item.meta.toLowerCase().includes(needle),
		);
	});
	const groups = createMemo(() => historyGroups(filteredItems(), props.i18n));

	return (
		<div
			data-composer-floating-panel="true"
			class="verevon-popover verevon-popover-up verevon-floating-panel verevon-floating-panel-sm verevon-floating-panel-compact dashboard-composer-floating-panel dashboard-composer-history-panel"
			style={panelPositionStyle(props.position)}
		>
			<div class="dashboard-composer-history-search">
				<Search class="size-3.5 shrink-0" strokeWidth={1.7} />
				<input
					type="search"
					value={historyQuery()}
					onInput={(event) =>
						setHistoryQuery(event.currentTarget.value)
					}
					placeholder={props.i18n.tr(
						"Filtrer titler og forhåndsvisninger",
						"Filter titles and previews",
					)}
					aria-label={props.i18n.tr(
						"Filtrer samtaler",
						"Filter conversations",
					)}
				/>
			</div>
			<div
				class="dashboard-composer-floating-panel__scroll"
				style={{ "max-height": `${props.position.maxHeight}px` }}
			>
				{/* The spinner is for a COLD load only. `refreshChatHistory` runs on
            every panel open, so gating the spinner on `props.loading` alone left
            it pinned above already-rendered results — and above search results,
            since a filtered view is still a background refresh underneath. Show
            it only when there is genuinely nothing to display yet; a refresh
            with data already in hand shows the data, not a spinner. */}
				<Show when={props.loading && allItems().length === 0}>
					<div class="verevon-menu-row dashboard-composer-history-row dashboard-composer-history-row--loading">
						<Loader2
							class="size-4 shrink-0 animate-spin"
							strokeWidth={1.7}
						/>
						<span>
							<span class="verevon-menu-label">
								{props.i18n.tr(
									"Laster samtaler ...",
									"Loading conversations ...",
								)}
							</span>
							<span class="verevon-menu-meta">
								{props.i18n.tr(
									"Historikk for innlogget bruker",
									"Signed-in user history",
								)}
							</span>
						</span>
					</div>
				</Show>
				{/* A query that matches nothing must say so. Without this the panel just
            empties, which reads as "history failed to load". */}
				<Show
					when={historyQuery().trim() && filteredItems().length === 0}
				>
					<div class="verevon-menu-row dashboard-composer-history-row">
						<span>
							<span class="verevon-menu-label">
								{props.i18n.tr("Ingen treff", "No matches")}
							</span>
							<span class="verevon-menu-meta">
								{props.i18n.tr(
									`Ingen av de ${allItems().length} lastede samtalene matcher.`,
									`None of the ${allItems().length} loaded conversations match.`,
								)}
							</span>
						</span>
					</div>
				</Show>
				<Show when={filteredItems().length > 0}>
					<For each={groups()}>
						{(group) => (
							<Show when={group.items.length > 0}>
								<div>
									<p class="dashboard-composer-history-group-label">
										{group.label}
									</p>
									<For each={group.items}>
										{(item) => (
											<div class="dashboard-composer-history-row-wrap">
												<button
													type="button"
													onClick={() =>
														item.threadId
															? props.onThreadSelect(
																	item.threadId,
																)
															: props.onClose()
													}
													class="verevon-menu-row dashboard-composer-history-row"
												>
													<Show
														when={item.pinned}
														fallback={
															<MessageSquare
																class="size-4 shrink-0"
																strokeWidth={
																	1.7
																}
															/>
														}
													>
														<Pin
															class="size-4 shrink-0"
															strokeWidth={1.7}
														/>
													</Show>
													<span>
														<span class="verevon-menu-label">
															{item.title ||
																props.i18n.tr(
																	"Uten tittel",
																	"Untitled",
																)}
														</span>
															<span class="verevon-menu-meta">
																{item.meta}
															</span>
															<Show
																when={historyRunStatusChip(item.runStatus, props.i18n)}
															>
																{(chip) => (
																	<span
																	class="dashboard-composer-history-status"
																	data-tone={chip().tone}
																	role="status"
																		aria-label={chip().label}
																	>
																		<span aria-hidden="true" />
																		{chip().label}
																	</span>
																)}
															</Show>
														</span>
													<span class="verevon-menu-meta dashboard-composer-history-row__time">
														{formatHistoryItemTime(
															item,
															group.isToday,
														)}
													</span>
												</button>
												{/* A SIBLING, not nested: a button inside a button is
                            invalid HTML and the inner click never fires. Only
                            rendered for real threads — a live composer turn has
                            no thread id to pin yet. */}
												<Show when={item.threadId}>
													{(threadId) => (
														<button
															type="button"
															class="dashboard-composer-history-pin"
															aria-pressed={
																item.pinned ===
																true
																	? "true"
																	: "false"
															}
															title={
																item.pinned
																	? props.i18n.tr(
																			"Løsne samtalen",
																			"Unpin conversation",
																		)
																	: props.i18n.tr(
																			"Fest samtalen øverst",
																			"Pin conversation to the top",
																		)
															}
															aria-label={
																item.pinned
																	? `${props.i18n.tr("Løsne", "Unpin")}: ${item.title}`
																	: `${props.i18n.tr("Fest", "Pin")}: ${item.title}`
															}
															onClick={() =>
																props.onTogglePin(
																	threadId(),
																	item.pinned !==
																		true,
																)
															}
														>
															<Pin
																class="size-3.5"
																strokeWidth={
																	1.7
																}
															/>
														</button>
													)}
												</Show>
											</div>
										)}
									</For>
								</div>
							</Show>
						)}
					</For>
				</Show>
				<Show
					when={
						!props.loading &&
						!historyQuery().trim() &&
						allItems().length === 0
					}
				>
					<div class="dashboard-composer-history-empty">
						<MessageSquare class="size-5" strokeWidth={1.5} />
						<p>
							{props.error ??
								props.i18n.tr(
									"Fant ingen samtaler for denne brukeren.",
									"No conversations found for this user.",
								)}
						</p>
					</div>
				</Show>

				<div class="dashboard-composer-menu-divider" />
				<a href="/chat" link onClick={props.onClose} class="verevon-menu-row">
					<LayoutGrid class="size-4 shrink-0" strokeWidth={1.7} />
					<span class="verevon-menu-label">
						{props.i18n.tr(
							"Vis alle samtaler",
							"View all conversations",
						)}
					</span>
				</a>
			</div>
		</div>
	);
}

function SettingsPanel(props: {
	i18n: ReturnType<typeof useI18n>;
	onAddFiles: () => void;
	onScreenshot: () => void;
	onSettingsChange: (settings: ComposerSettings) => void;
	position: PanelPosition;
	settings: ComposerSettings;
}) {
	const [view, setView] = createSignal<SettingsView>("main");
	const currentLangLabel = () => {
		const label = voiceLanguages.find(
			(language) => language.value === props.settings.voiceLang,
		)?.label;
		return label
			? props.i18n.tr(label.no, label.en)
			: props.settings.voiceLang;
	};
	const updateSetting = <K extends keyof ComposerSettings>(
		key: K,
		value: ComposerSettings[K],
	) => {
		props.onSettingsChange({ ...props.settings, [key]: value });
	};

	return (
		<div
			data-composer-floating-panel="true"
			class="verevon-popover verevon-popover-side verevon-floating-panel verevon-floating-panel-xs verevon-floating-panel-compact dashboard-composer-floating-panel dashboard-composer-settings-panel"
			style={panelPositionStyle(props.position)}
		>
			<div
				class="dashboard-composer-floating-panel__scroll"
				style={{ "max-height": `${props.position.maxHeight}px` }}
			>
				<Switch>
					<Match when={view() === "main"}>
						<div class="verevon-settings-view">
							<div class="verevon-menu-row">
								<Mic
									class="size-[17px] shrink-0"
									strokeWidth={1.7}
								/>
								<span class="verevon-menu-label">
									{props.i18n.tr(
										"Stemmespråk",
										"Voice language",
									)}
								</span>
								<select
									value={props.settings.voiceLang}
									title={currentLangLabel()}
									onChange={(event) =>
										updateSetting(
											"voiceLang",
											event.currentTarget.value,
										)
									}
									class="dashboard-composer-settings-select"
								>
									<For each={voiceLanguages}>
										{(language) => (
											<option value={language.value}>
												{props.i18n.tr(
													language.label.no,
													language.label.en,
												)}
											</option>
										)}
									</For>
								</select>
							</div>

							<For each={toneOptions}>
								{(option) => (
									<ComposerMenuRow
										icon={
											<Dynamic
												component={option.icon}
												class="size-[17px]"
												strokeWidth={1.7}
											/>
										}
										label={props.i18n.tr(
											option.label.no,
											option.label.en,
										)}
										onClick={() =>
											updateSetting("tone", option.value)
										}
										right={
											props.settings.tone ===
											option.value ? (
												<Check
													class="size-3.5"
													strokeWidth={2.5}
												/>
											) : (
												<span class="dashboard-composer-menu-spacer" />
											)
										}
									/>
								)}
							</For>

							<div class="dashboard-composer-menu-divider" />
							<ComposerMenuRow
								icon={
									<Paperclip
										class="size-[17px]"
										strokeWidth={1.7}
									/>
								}
								label={props.i18n.tr(
									"Legg til filer eller bilder",
									"Add files or photos",
								)}
								onClick={props.onAddFiles}
							/>
							<ComposerMenuRow
								icon={
									<Camera
										class="size-[17px]"
										strokeWidth={1.7}
									/>
								}
								label={props.i18n.tr(
									"Ta et skjermbilde",
									"Take a screenshot",
								)}
								onClick={props.onScreenshot}
							/>
							<ComposerMenuRow
								icon={
									<Blocks
										class="size-[17px]"
										strokeWidth={1.7}
									/>
								}
								label={props.i18n.tr("Ferdigheter", "Skills")}
								onClick={() => setView("skills")}
								right={<ChevronRight class="size-3.5" />}
							/>
							<ComposerMenuRow
								icon={
									<LayoutGrid
										class="size-[17px]"
										strokeWidth={1.7}
									/>
								}
								label={props.i18n.tr("Koblinger", "Connectors")}
								onClick={() => setView("connectors")}
								right={<ChevronRight class="size-3.5" />}
							/>
						</div>
					</Match>

					<Match when={view() === "skills"}>
						<SettingsSubView
							title={props.i18n.tr("Ferdigheter", "Skills")}
							onBack={() => setView("main")}
						>
							<RemoteSettingsList
								emptyLabel={props.i18n.tr(
									"Ingen ferdigheter er tilgjengelige ennå.",
									"No skills are available yet.",
								)}
								endpoint="/api/v1/skills"
								itemKey="skills"
								manageHref="/agents"
								manageLabel={props.i18n.tr(
									"Administrer ferdigheter",
									"Manage skills",
								)}
								i18n={props.i18n}
							/>
						</SettingsSubView>
					</Match>

					<Match when={view() === "connectors"}>
						<SettingsSubView
							title={props.i18n.tr("Koblinger", "Connectors")}
							onBack={() => setView("main")}
						>
							<RemoteSettingsList
								emptyLabel={props.i18n.tr(
									"Ingen koblinger er tilkoblet ennå.",
									"No connectors are connected yet.",
								)}
								endpoint="/api/v1/integrations/providers"
								itemKey="providers"
								manageHref="/settings/integrations"
								manageLabel={props.i18n.tr(
									"Koble til flere",
									"Connect more",
								)}
								i18n={props.i18n}
							/>
						</SettingsSubView>
					</Match>
				</Switch>
			</div>
		</div>
	);
}

function ComposerMenuRow(props: {
	icon: JSX.Element;
	label: string;
	onClick: () => void;
	right?: JSX.Element;
}) {
	return (
		<button
			type="button"
			onClick={() => props.onClick()}
			class="verevon-menu-row dashboard-composer-settings-row"
		>
			{props.icon}
			<span class="verevon-menu-label">{props.label}</span>
			{props.right}
		</button>
	);
}

function SettingsSubView(props: {
	children: JSX.Element;
	onBack: () => void;
	title: string;
}) {
	return (
		<div class="verevon-settings-view">
			<button
				type="button"
				onClick={() => props.onBack()}
				class="verevon-menu-row dashboard-composer-settings-back"
			>
				<ArrowLeft class="size-3.5" strokeWidth={1.8} />
				<span>{props.title}</span>
			</button>
			<div class="dashboard-composer-menu-divider" />
			{props.children}
		</div>
	);
}

function RemoteSettingsList(props: {
	emptyLabel: string;
	endpoint: string;
	i18n: ReturnType<typeof useI18n>;
	itemKey: string;
	manageHref?: string;
	manageLabel?: string;
}) {
	const [items] = createResource(
		() => [props.endpoint, props.itemKey] as const,
		([endpoint, itemKey]) => loadComposerSettingsItems(endpoint, itemKey),
	);

	return (
		<>
			<Show
				when={!items.loading}
				fallback={
					<div class="dashboard-composer-settings-loading">
						<Loader2 class="size-3.5" />
						<span>
							{props.i18n.tr("Laster ...", "Loading ...")}
						</span>
					</div>
				}
			>
				<Show
					when={!items.error}
					fallback={
						<div class="dashboard-composer-settings-error">
							<p>
								{props.i18n.tr(
									"Kunne ikke laste fra gatewayen.",
									"Could not load from the gateway.",
								)}
							</p>
							<small>
								{items.error instanceof Error
									? items.error.message
									: props.i18n.tr(
											"Tjenesten er utilgjengelig",
											"Service unavailable",
										)}
							</small>
						</div>
					}
				>
					<Show
						when={(items() ?? []).length > 0}
						fallback={
							<div class="dashboard-composer-settings-empty">
								{props.emptyLabel}
							</div>
						}
					>
						<For each={items()}>
							{(item) => (
								<RemoteSettingsItemRow
									i18n={props.i18n}
									item={item}
								/>
							)}
						</For>
					</Show>
				</Show>
			</Show>

			<Show
				when={
					props.manageHref && props.manageLabel
						? { href: props.manageHref, label: props.manageLabel }
						: null
				}
			>
				{(manage) => (
					<>
						<div class="dashboard-composer-menu-divider" />
						<a
							href={manage().href}
							link
							class="verevon-menu-row dashboard-composer-settings-link"
						>
							<span class="dashboard-composer-settings-item-icon">
								<Briefcase class="size-3.5" />
							</span>
							<span class="verevon-menu-label">
								{manage().label}
							</span>
						</a>
					</>
				)}
			</Show>
		</>
	);
}

function RemoteSettingsItemRow(props: {
	i18n: ReturnType<typeof useI18n>;
	item: ComposerSettingsItem;
}) {
	return (
		<div class="verevon-menu-row dashboard-composer-settings-data-row">
			<span class="dashboard-composer-settings-item-icon">
				<Briefcase class="size-3.5" />
			</span>
			<span class="dashboard-composer-settings-item-copy">
				<span class="verevon-menu-label">{props.item.name}</span>
				<Show when={props.item.description}>
					<span class="verevon-menu-meta">
						{props.item.description}
					</span>
				</Show>
			</span>
			<Show when={typeof props.item.connected === "boolean"}>
				<span class="verevon-menu-meta dashboard-composer-settings-item-status">
					{props.item.connected
						? props.i18n.tr("Klar", "Ready")
						: props.i18n.tr("Åpne", "Open")}
				</span>
			</Show>
		</div>
	);
}

function SplitText(props: { text: string }) {
	return (
		<span>
			<For each={props.text.split("")}>
				{(char, index) => (
					<span
						class="verevon-split-char"
						style={{ "animation-delay": `${index() * 25}ms` }}
					>
						{char === " " ? "\u00a0" : char}
					</span>
				)}
			</For>
		</span>
	);
}

type ComposerIconButtonVariant = "chip" | "toolbar";

function ComposerIconButton(props: {
	active?: boolean;
	children: JSX.Element;
	disabled?: boolean;
	expanded?: boolean;
	label: string;
	onClick: () => void;
	variant: ComposerIconButtonVariant;
}) {
	const baseClass = () =>
		props.variant === "toolbar"
			? "dashboard-composer-toolbar-icon"
			: "dashboard-composer-icon-chip";

	return (
		<button
			type="button"
			aria-label={props.label}
			aria-pressed={props.active ? "true" : "false"}
			aria-expanded={props.expanded == null ? undefined : props.expanded ? "true" : "false"}
			disabled={props.disabled}
			title={props.label}
			onClick={() => {
				if (!props.disabled) props.onClick();
			}}
			class={cn(
				baseClass(),
				props.active ? `${baseClass()}--active` : "",
			)}
		>
			{props.children}
		</button>
	);
}

function formatComposerTurnTime(date: Date) {
	return date.toLocaleTimeString(undefined, {
		hour: "2-digit",
		minute: "2-digit",
	});
}

function threadToHistoryItem(
	thread: ChatThreadSession,
	i18n: ReturnType<typeof useI18n>,
): HistoryPanelItem {
	return {
		fallbackTime: thread.updatedAt,
		id: thread.threadId,
		meta: thread.preview || i18n.tr("Chat-tråd", "Chat thread"),
		threadId: thread.threadId,
		title: thread.title || i18n.tr("Uten tittel", "Untitled"),
		updatedAt: thread.updatedAt,
		pinned: thread.pinned,
		runStatus: thread.latestRunStatus,
	};
}

function turnToHistoryItem(
	turn: ComposerTurn,
	i18n: ReturnType<typeof useI18n>,
): HistoryPanelItem {
	return {
		fallbackTime: turn.createdAt,
		id: turn.id,
		meta: turn.model,
		title: turn.body || i18n.tr("Uten tittel", "Untitled"),
		updatedAt: turn.createdAtIso,
	};
}

type HistoryRunStatusTone = "active" | "attention";

type HistoryRunStatusChip = {
	label: string;
	tone: HistoryRunStatusTone;
};

/**
 * Keep the history rail calm: completed/cancelled runs need no extra chrome,
 * while active or attention states make a durable thread discoverable without
 * polling every thread or opening the dense Work canvas.
 */
function historyRunStatusChip(
	value: string | undefined,
	i18n: ReturnType<typeof useI18n>,
): HistoryRunStatusChip | undefined {
	const status = value?.trim().toLowerCase();
	if (!status) return undefined;
	if (status === "queued") {
		return { label: i18n.tr("I kø", "Queued"), tone: "active" };
	}
	if (status === "running" || status === "in_progress") {
		return { label: i18n.tr("Pågår", "Running"), tone: "active" };
	}
	if (
		status === "awaiting_approval" ||
		status === "waiting_approval" ||
		status === "paused" ||
		status === "blocked" ||
		status === "ambiguous"
	) {
		return { label: i18n.tr("Trenger oppmerksomhet", "Needs attention"), tone: "attention" };
	}
	if (status === "failed") {
		return { label: i18n.tr("Mislyktes", "Failed"), tone: "attention" };
	}
	return undefined;
}

function historyGroups(
	items: HistoryPanelItem[],
	i18n: ReturnType<typeof useI18n>,
) {
	const now = new Date();
	const today = now.toDateString();
	const yesterday = new Date(now.getTime() - 86_400_000).toDateString();
	const groups = items.reduce<{
		today: HistoryPanelItem[];
		yesterday: HistoryPanelItem[];
		earlier: HistoryPanelItem[];
	}>(
		(accumulator, item) => {
			const date = new Date(item.updatedAt).toDateString();
			if (date === today)
				return { ...accumulator, today: [...accumulator.today, item] };
			if (date === yesterday)
				return {
					...accumulator,
					yesterday: [...accumulator.yesterday, item],
				};
			return { ...accumulator, earlier: [...accumulator.earlier, item] };
		},
		{ today: [], yesterday: [], earlier: [] },
	);

	return [
		{
			label: i18n.tr("I dag", "Today"),
			items: groups.today,
			isToday: true,
		},
		{
			label: i18n.tr("I går", "Yesterday"),
			items: groups.yesterday,
			isToday: false,
		},
		{
			label: i18n.tr("Tidligere", "Earlier"),
			items: groups.earlier,
			isToday: false,
		},
	] as const;
}

function formatHistoryItemTime(item: HistoryPanelItem, isToday: boolean) {
	const date = new Date(item.updatedAt);
	if (Number.isNaN(date.getTime())) return item.fallbackTime;

	return isToday
		? date.toLocaleTimeString(undefined, {
				hour: "2-digit",
				minute: "2-digit",
			})
		: date.toLocaleDateString(undefined, {
				month: "short",
				day: "numeric",
			});
}

function panelPositionStyle(position: PanelPosition): JSX.CSSProperties {
	const style: JSX.CSSProperties = {
		"max-height": `${position.maxHeight}px`,
		position: "fixed",
		"z-index": "var(--verevon-z-popover)",
	};
	if (position.bottom !== undefined) style.bottom = `${position.bottom}px`;
	if (position.left !== undefined) style.left = `${position.left}px`;
	if (position.right !== undefined) style.right = `${position.right}px`;
	if (position.top !== undefined) style.top = `${position.top}px`;
	return style;
}
