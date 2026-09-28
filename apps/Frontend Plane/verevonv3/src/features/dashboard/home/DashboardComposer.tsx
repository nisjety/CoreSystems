import { chatAppearanceActive } from "@/features/chat/lib/chat-appearance-preview";
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
	CalendarClock,
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
	Mail,
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
	UploadCloud,
	WandSparkles,
	X,
	Zap,
	type LucideProps,
} from "@/shared/icons";
import {
	createEffect,
	createMemo,
	createSignal,
	createUniqueId,
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
import {
	VoiceModeDialog,
	type VoiceReply,
	type VoiceSendResult,
} from "@/features/voice/VoiceModeDialog";
import {
	classifyDictateError,
	voiceErrorFromMic,
	voiceErrorMessage,
	voiceInputBlockedMessage,
	voiceModeBlockedMessage,
} from "@/features/voice/voice-mode-state";
import {
	classifyMicError,
	createVoiceRecorder,
	type VoiceRecorder,
	type VoiceRecording,
} from "@/features/voice/voice-recorder";
import { speechPlayer } from "@/features/voice/speech-playback";
import { PendingChatLaunchError, writePendingChatLaunch } from "@/features/chat/lib/pending-chat-launch";
import { ScheduleDialog } from "@/features/chat/components/ScheduleDialog";
import { prepareChatAttachments } from "@/features/chat/lib/prepare-chat-attachments";
import {
	extractChatDocument,
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
	browseConnection,
	fetchConnectionContent,
	listConnectorPickerTabs,
	type ConnectorBrowseOperation,
	type ConnectorContentOperation,
	type ConnectorPickerTab,
} from "@/shared/api/integrations-client";
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
import { translateApiError, useI18n } from "@/shared/i18n";
import { ApiError } from "@/shared/api/http";
import { cn } from "@/shared/lib/cn";
import { getSession } from "@/shared/session/session-store";
import {
	lastKnownAiModelPrivacyTier,
	readAiModelSelection,
	rememberAiModelPrivacyTier,
	rememberAiModelSelection,
} from "@/shared/ai/model-selection";
import {
	CATALOGUE_TIER_WAIT_MS,
	isIntentModeSelection,
	rememberedPrivacyTier,
	resolveSubmitPrivacyTier,
	waitForCatalogue,
} from "./submit-privacy-tier";

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

/**
 * Where a connector-attach picker result came from. Undefined for an
 * ordinary local upload/paste/screenshot — only a picked item carries one.
 */
export type ComposerAttachmentSource =
	| "onedrive"
	| "gmail"
	| "outlook"
	| "gdrive"
	| "gcal"
	| "outlook-cal";

type ComposerFile = {
	id: string;
	name: string;
	size: number;
	type: string;
	url: string;
	/**
	 * Set at pick time for a connector-sourced binary document (the network
	 * fetch already happened in the picker, so extracting immediately is
	 * free — see connector-picker spec §2.4). Undefined for a local upload,
	 * which is still extracted lazily at submit time.
	 */
	extractedText?: string;
	/** Provenance for a connector-attach picker result; see ComposerAttachmentSource. */
	source?: ComposerAttachmentSource;
	/** Opaque id of the picked item at its provider (Graph item id, Drive file id, message id, event id). */
	sourceItemId?: string;
	/** Integration Core connection id the item was fetched through. */
	connectionId?: string;
	/** ISO timestamp of when the content/browse call actually ran. */
	fetchedAt?: string;
};

type ComposerTurn = {
	id: string;
	body: string;
	browseWeb: boolean;
	createdAt: string;
	createdAtIso: string;
	deepSearch: boolean;
	files: string[];
	/** Parallel to `files` (same index) — a connector-attach picker source
	 * when that attachment was picked, undefined for a local upload. Used
	 * only for the receipt's per-source breakdown line. */
	fileSources: Array<ComposerAttachmentSource | undefined>;
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

type SettingsView = "main" | "skills" | "connectors" | "attach-source";
type EntityKind = "date" | "file";

type EntityToken = {
	kind: EntityKind;
	start: number;
	text: string;
};

type ComposerCommandArgument = {
	hint: { en: string; no: string };
	required: boolean;
};

/**
 * A `/` command: a name the user types, aliases in both languages, a one-line
 * description the menu shows, and — where it makes sense — an argument taken
 * from the rest of the line.
 */
export type ComposerCommand = {
	aliases: string[];
	argument?: ComposerCommandArgument;
	/** Local composer behaviour; absent for a backend skill/capability/connector. */
	builtin?: "deep_research" | "file" | "image" | "schedule" | "web_search";
	description: { en: string; no: string };
	icon: Component<LucideProps>;
	id: string;
	label: string;
	/** Canonical token after "/", lowercase and space-free. */
	name: string;
	specialized?: SpecializedAction;
};

type AutocompleteItem = {
	/** Localized name of what this command takes after its name, when it takes one. */
	argumentHint?: string;
	command?: ComposerCommand;
	description?: string;
	icon: Component<LucideProps>;
	id: string;
	label: string;
	meta?: EntityKind | "slash";
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
	extractedText?: string;
	id: string;
	name: string;
	size: number;
	type: string;
	url: string;
	/** Provenance for a connector-attach picker result; see ComposerAttachmentSource. */
	source?: ComposerAttachmentSource;
	/** Opaque id of the picked item at its provider (Graph item id, Drive file id, message id, event id). */
	sourceItemId?: string;
	/** Integration Core connection id the item was fetched through. */
	connectionId?: string;
	/** ISO timestamp of when the content/browse call actually ran. */
	fetchedAt?: string;
};

/** A skill/capability/connector activated via the `/` menu. */
export type ComposerActiveAction = {
	id: string;
	name: string;
	kind: "skill" | "capability" | "connector";
};

export type DashboardComposerSubmitPayload = {
    sourceScope?: import('@/shared/actions/chat-source-scope').ChatSourceScope;
    /**
     * Per-turn retrieval-source filter (chat-wave2 scope spec), orthogonal to
     * `sourceScope`: never locked, changeable every send. Omitted (or
     * `'auto'`) when the conversation-only toggle is on, or the picker never
     * left its default.
     */
    retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope;
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
	 * constraint, byte-identical to today's behavior. A catalogue model whose
	 * tier the composer cannot find out (catalogue slow or down, no tier
	 * stored for it) carries the strictest tier, never none — see
	 * `submit-privacy-tier.ts`.
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
// F-09 (CHAT_PARITY_AUDIT_2026-09-15.md §3.1). What stood here was a two-entry
// `slashCommands` array — "/Last opp fil", "/Generer bilde" — that no code path
// could reach: `applyAutocompleteSelection` looked it up by an id
// (`cmd-file`/`cmd-image`) that the menu builder never produced, since the menu
// has been built from `specializedActions()` for some time. So the audit's two
// "commands" were both unparameterised AND dead. These are commands: a typed
// name, aliases so Norwegian and English both work, a description the menu
// renders, and an argument read off the rest of the line where one makes sense.
// The two originals survive as `file` and `image`.
const BUILTIN_COMMANDS: ComposerCommand[] = [
	{
		aliases: ["fil", "upload", "vedlegg", "attach"],
		description: {
			no: "Legg ved en fil eller et bilde i meldingen.",
			en: "Attach a file or photo to your message.",
		},
		builtin: "file",
		icon: Upload,
		id: "builtin:__file",
		label: "Upload file",
		name: "file",
	},
	{
		aliases: ["bilde", "img", "tegn"],
		argument: {
			hint: { no: "beskrivelse", en: "description" },
			required: true,
		},
		builtin: "image",
		description: {
			no: "Lag et bilde fra beskrivelsen som følger.",
			en: "Create an image from the description that follows.",
		},
		icon: ImagePlus,
		id: "builtin:__image",
		label: "Generate image",
		name: "image",
	},
	{
		aliases: ["web", "søk", "sok", "nett"],
		builtin: "web_search",
		description: {
			no: "La Verevon søke på nett mens den svarer.",
			en: "Let Verevon search the web while it answers.",
		},
		icon: Globe2,
		id: "builtin:web_search",
		label: "Web search",
		name: "search",
	},
	{
		aliases: ["dyp", "deep", "dypdykk"],
		argument: {
			hint: { no: "spørsmål", en: "question" },
			required: false,
		},
		builtin: "deep_research",
		description: {
			no: "Kjør flere research-runder før svaret.",
			en: "Run several research rounds before answering.",
		},
		icon: Telescope,
		id: "builtin:deep_research",
		label: "Deep research",
		name: "research",
	},
	{
		aliases: ["planlegg", "gjenta", "recurring"],
		argument: {
			hint: {
				no: "hva som skal gjentas (valgfritt — bruker utkastet)",
				en: "what to repeat (optional — uses the draft)",
			},
			required: false,
		},
		builtin: "schedule",
		description: {
			no: "Gjør denne forespørselen om til en planlagt kjøring.",
			en: "Turn this request into a recurring job.",
		},
		icon: CalendarClock,
		id: "builtin:schedule",
		label: "Schedule",
		name: "schedule",
	},
];

// The `/` menu opens only when the slash starts the draft. A slash typed inside
// a sentence — "2026/09", "og/eller", "kr 200/mnd" — is ordinary text and stays
// ordinary text, which the old `\/(\w*)$` match (a slash ANYWHERE before the
// caret) did not honour.
const SLASH_TRIGGER_PATTERN = /^\s*\/([\p{L}\p{N}_-]*)$/u;
const DRAFT_COMMAND_PATTERN = /^\s*\/([\p{L}\p{N}_-]+)(?:\s+([\s\S]*))?$/u;

function normalizeCommandToken(value: string): string {
	return value.trim().toLowerCase();
}

function commandMatchesQuery(command: ComposerCommand, query: string): boolean {
	if (!query) return true;
	return (
		command.name.startsWith(query) ||
		command.aliases.some((alias) => alias.startsWith(query)) ||
		command.label.toLowerCase().includes(query)
	);
}

/**
 * True when running this command needs a wire capability the route may refuse.
 * `file` is the only one that does not: an attachment rides the request body,
 * so it survives every provider.
 */
function commandNeedsFullRoute(command: ComposerCommand): boolean {
	return command.builtin !== "file";
}

/** Backend skill/capability/connector names become typable command tokens. */
function slugifyCommandName(name: string): string {
	return (
		name
			.toLowerCase()
			.replace(/[^\p{L}\p{N}]+/gu, "-")
			.replace(/^-+|-+$/gu, "") || "action"
	);
}

/**
 * Resolve the draft's leading `/name` against the catalog, splitting off its
 * argument. Returns null for anything that is not a command — including an
 * UNKNOWN name — so a draft that merely happens to start with a slash is sent
 * as the plain text the user typed rather than swallowed as a failed command.
 */
export function matchDraftCommand(
	message: string,
	catalog: ComposerCommand[],
): { argument: string; command: ComposerCommand } | null {
	const match = DRAFT_COMMAND_PATTERN.exec(message);
	if (!match) return null;
	const token = normalizeCommandToken(match[1] ?? "");
	const command = catalog.find(
		(candidate) =>
			candidate.name === token || candidate.aliases.includes(token),
	);
	if (!command) return null;
	return { argument: (match[2] ?? "").trim(), command };
}

const TEXTAREA_AUTO_MAX_PX = 240;

/**
 * ChatGPT and Claude both convert a sufficiently long paste into a "pasted
 * text" attachment instead of dumping it inline into the composer
 * (CHAT_PARITY_AUDIT_2026-09-15.md §3.6 / F-... "long paste"). This is the
 * length, in `string.length` code units, above which Verevon does the same.
 * It sits in the "a few thousand characters" range those products use: short
 * enough that a genuine document-scale paste (a log dump, an article, a long
 * spec) becomes a tidy attachment chip, long enough that an ordinary
 * paragraph-or-two paste still lands inline exactly as before.
 */
export const PASTE_TEXT_ATTACHMENT_THRESHOLD = 4000;

// Inline rather than in global.css: `.dashboard-composer-autocomplete-menu
// button span` there sets every span in a row to 14px/600, which is right for a
// command's name and wrong for its description. These are the only new visual
// pieces the command menu needs, and keeping them here keeps the change inside
// this component.
const COMMAND_MENU_STYLE: JSX.CSSProperties = { "max-width": "380px" };
const COMMAND_BODY_STYLE: JSX.CSSProperties = {
	display: "flex",
	"flex-direction": "column",
	gap: "2px",
	"min-width": "0",
};
const COMMAND_LABEL_STYLE: JSX.CSSProperties = {
	display: "flex",
	"align-items": "baseline",
	gap: "6px",
};
const COMMAND_ARGUMENT_STYLE: JSX.CSSProperties = {
	"font-size": "12px",
	"font-weight": "500",
	opacity: "0.6",
};
const COMMAND_DESCRIPTION_STYLE: JSX.CSSProperties = {
	"font-size": "12px",
	"font-weight": "400",
	opacity: "0.7",
	"white-space": "normal",
};
const COMMAND_NOTE_STYLE: JSX.CSSProperties = {
	padding: "6px 16px 0",
	"font-size": "11.5px",
	"line-height": "1.4",
	opacity: "0.7",
};
const COMPOSER_NOTICE_STYLE: JSX.CSSProperties = {
	display: "flex",
	"align-items": "center",
	gap: "7px",
	margin: "0 14px 8px",
	"font-size": "11.5px",
	"line-height": "1.4",
	opacity: "0.8",
};

type ComposerSubmitState = {
	hasContent: boolean;
	submitting?: boolean;
	voiceMode: boolean;
	voiceRecording: boolean;
};

/**
 * What a submit attempt came to. Voice mode shows a failure in its own dialog,
 * because the composer's alert renders behind the modal.
 */
type ComposerSubmitResult = VoiceSendResult;

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

// Strips the most common markdown syntax and returns the first ~40 characters
// as a schedule name, mirroring the "short, user-editable" name field
// `ScheduleDialog` prefills (chat-wave2 "schedule" spec §2.1). No shared
// title-from-content helper exists elsewhere in this codebase to reuse.
const SCHEDULE_TITLE_MAX_CHARS = 40;
function scheduleTitleFrom(text: string): string {
	const stripped = text
		.replace(/[`*_#>~[\]]/g, "")
		.replace(/\s+/g, " ")
		.trim();
	if (stripped.length <= SCHEDULE_TITLE_MAX_CHARS) return stripped;
	return `${stripped.slice(0, SCHEDULE_TITLE_MAX_CHARS).trimEnd()}…`;
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

function pastedPlainText(event: ClipboardEvent) {
	return event.clipboardData?.getData("text/plain") ?? "";
}

/** A short, readable name for a paste-generated text attachment, e.g. "Limt inn tekst: some preview text….txt". */
function pastedTextAttachmentName(text: string, label: string): string {
	const trimmed = text.trim();
	const preview = trimmed.replace(/\s+/g, " ").slice(0, 60);
	if (preview.length === 0) {
		return `${label} ${new Date().toLocaleTimeString()}.txt`;
	}
	const truncated = trimmed.length > preview.length ? "…" : "";
	return `${label}: ${preview}${truncated}.txt`;
}

/** Wraps pasted text as a `.txt`-equivalent File so it flows through the existing attachment pipeline (`addFiles`). */
function pastedTextAttachmentFile(text: string, label: string): File {
	return new File([text], pastedTextAttachmentName(text, label), {
		type: "text/plain",
	});
}

export function DashboardComposer(props: {
    sourceScope?: import('@/shared/actions/chat-source-scope').ChatSourceScope;
    onSourceScopeChange?: (value: import('@/shared/actions/chat-source-scope').ChatSourceScope) => void;
    sourceScopeLocked?: boolean;
    /**
     * Per-turn retrieval-source filter (chat-wave2 scope spec), mirroring the
     * `sourceScope`/`onSourceScopeChange` pair above — deliberately no
     * "Locked" prop: this one is never locked, unlike `sourceScope`.
     */
    retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope;
    onRetrievalScopeChange?: (value: import('@/shared/actions/chat-source-scope').RetrievalScope) => void;
	/** Calm, conversation-first presentation used by the dedicated chat page. */
	appearance?: "chat" | "default";
	/** Compact Home dock; only changes the input's initial presentation. */
	compact?: boolean;
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
	/**
	 * The latest assistant turn, on a surface that shows replies (chat). Voice
	 * mode waits on it after a spoken turn and shows the reply; left undefined
	 * where a send navigates away (the dashboard), so Voice mode closes instead.
	 */
	voiceReply?: VoiceReply | null;
}) {
	const i18n = useI18n();
	const navigate = useNavigate();
	let composerRootRef: HTMLDivElement | undefined;
	let fileInputRef: HTMLInputElement | undefined;
	let historyTriggerRef: HTMLSpanElement | undefined;
	let settingsTriggerRef: HTMLSpanElement | undefined;
	let textareaRef: HTMLTextAreaElement | undefined;
	let voiceModeButtonRef: HTMLButtonElement | undefined;
	let modeAnnouncementTimer: number | undefined;
	// Voice input's take and its dictate call. The recorder is Voice mode's
	// (features/voice): one take, the 60 s / 1.4 MB caps, the tracks always
	// stopped, and a stream that arrives after a cancel handed straight back.
	let voiceInputTake: VoiceRecorder | undefined;
	let voiceInputDictation: AbortController | undefined;
	let composerDisposed = false;
	let pendingFilePosition: number | null = null;
	const [internalBrowseWeb, setInternalBrowseWeb] = createSignal(false);
	const [internalSourceScope, setInternalSourceScope] = createSignal<import('@/shared/actions/chat-source-scope').ChatSourceScope>('workspace');
	const sourceScope = () => props.sourceScope ?? internalSourceScope();
	const conversationOnly = () => sourceScope() === 'conversation';
	// Per-turn retrieval-source filter (chat-wave2 scope spec) — never locked,
	// unlike `sourceScope` above, so there is no analog of `sourceScopeLocked`.
	const [internalRetrievalScope, setInternalRetrievalScope] = createSignal<import('@/shared/actions/chat-source-scope').RetrievalScope>('auto');
	const retrievalScope = () => props.retrievalScope ?? internalRetrievalScope();
	const setRetrievalScope = (value: import('@/shared/actions/chat-source-scope').RetrievalScope) => {
		setInternalRetrievalScope(value);
		props.onRetrievalScopeChange?.(value);
		// Plan mode dispatches to execution-core's own agentic run — a separate
		// pipeline this per-turn source filter cannot reach (chat-wave2 scope
		// spec), so model-gateway rejects the combination outright. Turn Plan
		// mode back off here rather than let the next send hit that error.
		if (value !== 'auto') props.onPlanModeChange?.(false);
	};
	// `conversationOnly` already excludes both source families outright, so a
	// stale `knowledge_only`/`web_only` pick must not silently survive the
	// switch to "Kun samtalen" — mirrors the `temporaryChat()` reset pattern
	// used elsewhere in this file.
	createEffect(() => conversationOnly(), (restricted) => { if (restricted) setInternalRetrievalScope('auto'); });
	const browseWeb = () => !subscriptionBacked() && (props.browseWeb ?? internalBrowseWeb());
	const setBrowseWeb = (next: boolean | ((current: boolean) => boolean)) => {
		const value = typeof next === "function" ? next(browseWeb()) : next;
		if (props.browseWeb === undefined) setInternalBrowseWeb(value);
		props.onBrowseWebChange?.(value);
	};
	const [autocomplete, setAutocomplete] =
		createSignal<AutocompleteState | null>(null);
	const [autocompleteIndex, setAutocompleteIndex] = createSignal(0);
	const [deepSearch, setDeepSearch] = createSignal(false);
	// "/schedule" / "Planlegg dette" — `origin` is carried through only for
	// its own test/debug value today (schedule-from-chat.ts's
	// `threadOriginHint`); it does not yet change behaviour.
	const [scheduleDialogOpen, setScheduleDialogOpen] = createSignal(false);
	const [scheduleSeed, setScheduleSeed] = createSignal<{
		title: string;
		goal: string;
		origin: "composer" | "message-action";
	}>({ title: "", goal: "", origin: "composer" });
	const [dragActive, setDragActive] = createSignal(false);
	const [entities, setEntities] = createSignal<EntityToken[]>([]);
	const [files, setFiles] = createSignal<ComposerFile[]>([]);
	const [knowledgeImporting, setKnowledgeImporting] = createSignal(false);
	const [knowledgeImportNotice, setKnowledgeImportNotice] = createSignal<string | null>(null);
	const [handoffPending, setHandoffPending] = createSignal(false);
	const [submitError, setSubmitError] = createSignal<string | null>(null);
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
	// Mirrors chat-client.ts's own `subscriptionBacked` check
	// (buildChatWireBody, ~L651): for this provider the wire silently sends
	// browse_web/generate_image/plan_mode/deep_research as `false` and drops
	// every explicit skill pick, regardless of what these controls show here.
	// Before this memo the composer had no way to know that, so Søk/Bilde/
	// Utfør/Dyp research stayed fully enabled and unlabeled while doing
	// nothing server-side — CHAT_PARITY_AUDIT_2026-09-15.md F-10.
	const subscriptionBacked = createMemo(
		() => selectedModelProvider() === "openai-codex-subscription",
	);
	const subscriptionDisabledTitle = () =>
		i18n.tr(
			"Ikke tilgjengelig med denne modellen",
			"Not available with this model",
		);
	const subscriptionDisabledStyle = (): JSX.CSSProperties | undefined =>
		subscriptionBacked()
			? { opacity: 0.5, cursor: "not-allowed" }
			: undefined;
	// "Kun nett" makes web search unconditional for this turn — the toggle
	// becomes redundant, so it is shown permanently on and disabled, the same
	// visual treatment `subscriptionDisabledTitle`/`subscriptionDisabledStyle`
	// already give a control the user cannot change right now.
	const webOnlyForcedTitle = () =>
		i18n.tr(
			"Nett er slått på for denne meldingen (kildefilter: kun nett)",
			"Web is on for this message (source filter: web only)",
		);
	// One catalogue request, shared by the picker and by a send that has to
	// wait for it (see `resolveTurnPrivacyTier`). `settledCatalogue` is a plain
	// variable, not the resource, so a send reads the answer the moment the
	// request settles: `undefined` while in flight, `null` once it failed.
	let catalogueRequest: Promise<ModelInfo[]> | undefined;
	let settledCatalogue: ModelInfo[] | null | undefined;
	const requestCatalogue = (): Promise<ModelInfo[]> => {
		if (!catalogueRequest) {
			let request: Promise<ModelInfo[]>;
			try {
				request = Promise.resolve(listModels());
			} catch (error) {
				request = Promise.reject(error);
			}
			catalogueRequest = request;
			// Registered first, so it runs before the resource sees the result.
			request.then(
				(list) => {
					settledCatalogue = Array.isArray(list) ? list : null;
				},
				() => {
					settledCatalogue = null;
				},
			);
		}
		return catalogueRequest;
	};
	const [models] = createResource(async () => {
		try {
			return await requestCatalogue();
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
	// A connection lookup in progress is not evidence of disconnection. Keep
	// the draft editable and wait before enabling Send on a persisted selection.
	const subscriptionConnectionPending = createMemo(() =>
		subscriptionBacked() && (!activeOrgId() || subscriptionConnections.loading || subscriptionConnections() === undefined),
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
		// No truthiness guard on the provider: plenty of real catalog models have
		// no provider tag (ModelInfo.provider is optional), and `selectModel`
		// below already threads the clicked model's own provider straight into
		// both signals, so an undefined-vs-undefined match is a legitimate
		// selection, not an absent one. Bailing out here previously made
		// selectedPrivacyTier() (and the submit payload's minPrivacyTier) go
		// undefined for any selected model without a provider.
		return flatChatModels().find(
			(model) =>
				model.id === selectedModel() &&
				model.provider === selectedModelProvider(),
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
	// so they never claim a tier here. Display only (the sovereign notice): the
	// tier a turn sends comes from `resolveTurnPrivacyTier`, which does not
	// read `undefined` from a catalogue that has not loaded yet.
	const selectedPrivacyTier = createMemo(() => selectedCatalogModel()?.privacyTier);
	const selectedSubscriptionRoute = createMemo(() => {
		const connection = activeSubscriptionConnection();
		// Catalog loading is independent of connection ownership. A delayed
		// catalog must not make a connected, persisted selection look revoked.
		return selectedModelProvider() === OPENAI_CODEX_SUBSCRIPTION_PROVIDER && connection
			? { provider: OPENAI_CODEX_SUBSCRIPTION_PROVIDER, connectionId: connection.id }
			: undefined;
	});
	const [staleSubscriptionNotice, setStaleSubscriptionNotice] =
		createSignal(false);
	const selectModel = (id: string, provider?: string) => {
		setStaleSubscriptionNotice(false);
		setSubmitError(null);
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
			// The last-known tier a turn falls back on if a later page load
			// sends before the catalogue answers.
			...(catalogModel ? { privacyTier: rememberedPrivacyTier(catalogModel) } : {}),
		});
		setModelOpen(false);
	};
	// Keep that last-known tier current: every time the catalogue lists the
	// selected model, store what it says now. This also gives a selection
	// saved before the tier was stored (or by another picker) its tier.
	createEffect(
		() => selectedCatalogModel(),
		(listed) => {
			if (!listed) return;
			rememberAiModelPrivacyTier(
				activeOrgId() || "",
				{ model: listed.id, provider: listed.provider },
				rememberedPrivacyTier(listed),
			);
		},
	);
	// A disconnected subscription remains the user's selection. Explain what
	// needs reconnecting; never silently change the model or billing route.
	createEffect(
		() => ({ connections: subscriptionConnections(), active: activeSubscriptionConnection(), provider: selectedModelProvider() }),
		(state) => {
			if (state.connections === undefined) return;
			setStaleSubscriptionNotice(state.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER && !state.active);
		},
	);
	createEffect(() => subscriptionBacked(), (selected) => { if (selected) setDeepSearch(false); });
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
	// Voice input is waiting on the microphone permission prompt. It already
	// counts as holding the microphone, so Voice mode cannot open under it.
	const [voiceInputStarting, setVoiceInputStarting] = createSignal(false);
	// Voice input's failures, for screen readers: the visible copy is the
	// mode announcement, which is aria-hidden and hidden once the draft has text.
	const [voiceInputError, setVoiceInputError] = createSignal("");
	// A tap on Voice input in Temporary chat: say why it is off, visibly.
	const [voiceInputBlockedShown, setVoiceInputBlockedShown] = createSignal(false);
	const voiceInputBlockedId = createUniqueId();
	const voiceModeBlockedId = createUniqueId();
	/** Drop Voice input's take, recording or still on the prompt; nothing is sent. */
	const cancelVoiceInputTake = () => {
		const take = voiceInputTake;
		voiceInputTake = undefined;
		setVoiceInputStarting(false);
		take?.cancel();
	};
	const [activeActions, setActiveActions] = createSignal<
		ComposerActiveAction[]
	>([]);
	const [specializedActions, setSpecializedActions] =
		createSignal<SpecializedAction[]>(BUILTIN_ACTIONS);
	let actionsLoadStarted = false;
	// The commands offered right now. A subscription-backed route silently drops
	// browse_web / generate_image / deep_research and every explicit skill pick
	// (`buildChatWireBody`, chat-client.ts), so offering a command that sets one
	// would re-create F-10's unlabeled no-op behind a slash. Withhold them and
	// say why, the same rule the Søk/Bilde/Utfør/Dyp research toggles follow.
	const commandCatalog = createMemo<ComposerCommand[]>(() => {
		if (subscriptionBacked()) {
			return BUILTIN_COMMANDS.filter(
				(command) => !commandNeedsFullRoute(command),
			);
		}
		return [
			...BUILTIN_COMMANDS,
			...specializedActions()
				.filter((action) => action.kind !== "builtin")
				.map((action) => ({
					aliases: [],
					description: {
						no: action.description ?? "",
						en: action.description ?? "",
					},
					icon: iconForAction(action),
					id: actionKey(action),
					label: action.name,
					name: slugifyCommandName(action.name),
					specialized: action,
				})),
		];
	});
	// The draft's leading `/name`, when it resolves to a real command. Null for
	// an unknown name, so "/foobar hei" is just a message that starts with a
	// slash.
	const draftCommand = createMemo(() =>
		matchDraftCommand(props.message, commandCatalog()),
	);
	/** The draft minus its command prefix — what actually gets sent. */
	const commandBody = createMemo(() => {
		const parsed = draftCommand();
		return parsed ? parsed.argument : props.message;
	});
	// Measured on the body, not the raw draft: a bare "/image" is an unfinished
	// command, not a message, and must not arm the send button.
	const hasContent = createMemo(
		() => commandBody().trim().length > 0 || files().length > 0,
	);
	const imageMode = createMemo(() => !subscriptionBacked() && (props.imageMode ?? false));
	const planMode = createMemo(() => !subscriptionBacked() && (props.planMode ?? false));
	// Mirrors the `conversationOnly` reset above: Plan mode may also be turned
	// on directly (e.g. the segmented Ask/Do control), not only through
	// `setRetrievalScope`, so a stale `knowledge_only`/`web_only` pick must not
	// silently survive that switch either — model-gateway rejects the
	// combination outright (chat-wave2 scope spec).
	createEffect(() => planMode(), (planning) => { if (planning) setInternalRetrievalScope('auto'); });
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
	// Voice mode and Voice input fail closed in Temporary chat: the speech path
	// cannot yet attest zero data retention, so no audio is captured there at
	// all. Voice mode still opens, in its blocked state, which says why and
	// never asks for the microphone; Voice input stays focusable
	// (aria-disabled) and says why when pressed. Each also waits while the
	// other holds the microphone, so only one of them ever records.
	const voiceInputHoldsMic = createMemo(
		() => voiceRecording() || voiceInputStarting(),
	);
	const voiceModeTitle = () => {
		if (temporaryChat()) return voiceModeBlockedMessage(i18n.tr);
		if (voiceInputHoldsMic())
			return i18n.tr(
				"Stemmemodus er utilgjengelig mens stemmeinndata tar opp.",
				"Voice mode is unavailable while Voice input is recording.",
			);
		return i18n.tr("Stemmemodus", "Voice mode");
	};
	const voiceInputTitle = () => {
		if (temporaryChat()) return voiceInputBlockedMessage(i18n.tr);
		if (voiceMode())
			return i18n.tr(
				"Stemmeinndata er utilgjengelig mens stemmemodus er åpen.",
				"Voice input is unavailable while Voice mode is open.",
			);
		return undefined;
	};
	createEffect(
		() => temporaryChat(),
		(temporary) => {
			if (!temporary) {
				setVoiceInputBlockedShown(false);
				return;
			}
			// Temporary chat started mid-take: drop the take, nothing is sent.
			// A dictation already on its way was an ordinary one and may finish.
			cancelVoiceInputTake();
		},
	);
	const voiceLanguageLabel = () => {
		const language = voiceLanguages.find(
			(candidate) => candidate.value === settings().voiceLang,
		);
		return language
			? i18n.tr(language.label.no, language.label.en)
			: settings().voiceLang;
	};
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
			submitting: handoffPending() || subscriptionConnectionPending() || (props.submitting && !props.allowMidRunSubmit),
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
		const normalized = normalizeCommandToken(query);
		return commandCatalog()
			.filter((command) => {
				const action = command.specialized;
				return (
					!action ||
					!activeActions().some(
						(a) => a.id === action.id && a.kind === action.kind,
					)
				);
			})
			.filter((command) => commandMatchesQuery(command, normalized))
			.slice(0, 8)
			.map((command) => ({
				...(command.argument
					? {
							argumentHint: i18n.tr(
								command.argument.hint.no,
								command.argument.hint.en,
							),
						}
					: {}),
				command,
				description: i18n.tr(
					command.description.no,
					command.description.en,
				),
				icon: command.icon,
				id: command.id,
				label: command.label,
				meta: "slash" as const,
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

	const runCommand = (command: ComposerCommand) => {
		if (command.builtin === "file") {
			openFileDialog();
			return;
		}
		if (command.builtin === "image") {
			props.onImageModeChange?.(true);
			return;
		}
		if (command.builtin === "web_search") {
			setBrowseWeb(true);
			return;
		}
		if (command.builtin === "deep_research") {
			setDeepSearch(true);
			return;
		}
		// "schedule" deliberately has no effect here, unlike the other builtins
		// above: picking it from the menu only inserts the "/schedule " prefix
		// (the same as any other parameterised command — see `selectCommand`),
		// and there is nothing useful to seed a dialog with until the goal text
		// after it is actually typed. `submitComposer` intercepts the finished
		// "/schedule <goal>" line instead — see the comment there.
		const action = command.specialized;
		if (!action || action.kind === "builtin") return;
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
		() => ({ message: props.message, expanded: textareaExpanded(), compact: props.compact }),
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

	/** Connector-attach picker result: same `files` signal a local upload
	 * uses, so AttachmentPreview/removeFile/submit are all reused unchanged
	 * (connector-picker spec §2.4 step 5). */
	const attachConnectorFile = (file: ComposerFile) => {
		setFiles((current) => [...current, file]);
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
			// Ends the take; its dictation then runs as below.
			voiceInputTake?.stop();
			return;
		}
		if (voiceInputStarting()) {
			// A second tap while the permission prompt is open: nothing is
			// recorded, and a stream granted after this is released at once.
			cancelVoiceInputTake();
			return;
		}
		// Voice mode holds the microphone while its dialog is open.
		if (voiceMode()) return;

		const reportVoiceInputError = (message: string) => {
			setModeAnnouncement(message);
			setVoiceInputError(message);
		};
		setVoiceInputError("");

		// Fails closed in Temporary chat, before the microphone is asked for:
		// the speech path cannot yet attest zero data retention for the audio.
		if (temporaryChat()) {
			setVoiceInputBlockedShown(true);
			setVoiceInputError(voiceInputBlockedMessage(i18n.tr));
			return;
		}
		setVoiceInputBlockedShown(false);

		// Never record over a reply that is being read aloud: the take would
		// transcribe it. Pressing the mic is a clear request to speak instead.
		const player = speechPlayer();
		if (player.playingId() !== null) player.stop();

		const onTranscript = props.onMessageChange;
		const take = createVoiceRecorder({ silence: false });
		voiceInputTake = take;
		setVoiceInputStarting(true);
		try {
			await take.start();
		} catch (error) {
			// Cancelled meanwhile: whoever cancelled it has reset the state.
			if (voiceInputTake !== take) return;
			voiceInputTake = undefined;
			setVoiceInputStarting(false);
			reportVoiceInputError(
				voiceErrorMessage(voiceErrorFromMic(classifyMicError(error)), i18n.tr),
			);
			return;
		}
		// Cancelled while the prompt was open (a second tap, Temporary chat,
		// the composer going away): `start` already handed the stream back.
		if (voiceInputTake !== take) return;
		// Belt and braces: Voice mode cannot open while this waits, and
		// Temporary chat cancels the take, but nothing records under either.
		if (composerDisposed || voiceMode() || temporaryChat()) {
			cancelVoiceInputTake();
			return;
		}
		setVoiceInputStarting(false);
		setVoiceRecording(true);

		let recording: VoiceRecording | null;
		try {
			recording = await take.result;
		} catch {
			recording = null;
			if (!composerDisposed)
				reportVoiceInputError(voiceErrorMessage("mic-failed", i18n.tr));
		}
		if (voiceInputTake === take) voiceInputTake = undefined;
		// A cancel, or Temporary chat started mid-take: nothing is sent.
		if (!recording || composerDisposed || temporaryChat()) {
			setVoiceRecording(false);
			return;
		}

		// Verevon Flow: STT + LLM cleanup in one round trip (fillers stripped,
		// punctuation fixed, self-corrections applied). Keep `voiceRecording`
		// on through the cleanup so the textarea stays locked and the loading
		// bar covers the processing phase; APPEND to the draft (the voice-modal
		// pattern) instead of overwriting whatever was already typed. Only
		// the "chat message" context goes with it, never a clean-up model.
		const controller = new AbortController();
		voiceInputDictation = controller;
		try {
			const dictation = await dictateAudioBlob(
				recording.blob,
				settings().voiceLang,
				"chat message",
				controller.signal,
				temporaryChat(),
			);
			if (composerDisposed || controller.signal.aborted) return;
			const text = dictation.text || dictation.rawText;
			if (!text) return;
			const trimmed = props.message.trim();
			onTranscript(trimmed ? `${trimmed} ${text}` : text);
			focusTextareaAtNextFrame();
		} catch (error: unknown) {
			const kind = classifyDictateError(error);
			if (kind !== "aborted" && !composerDisposed)
				reportVoiceInputError(voiceErrorMessage(kind, i18n.tr));
		} finally {
			if (voiceInputDictation === controller) voiceInputDictation = undefined;
			if (!composerDisposed) setVoiceRecording(false);
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

		// The `/file` command parks the caret position so the chosen file's name
		// lands back where the command was. A cancelled file dialog fires no
		// event, so the next keystroke is what retires that position — otherwise
		// a much later drop would insert a name at a caret that moved long ago.
		pendingFilePosition = null;
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

	const selectCommand = (
		state: AutocompleteState,
		command: ComposerCommand,
	) => {
		if (command.builtin === "file") {
			// Remember where the trigger was so the chosen file's name lands
			// back in the draft at that spot.
			pendingFilePosition = state.triggerStart;
		}
		if (command.argument) {
			// A parameterised command is not finished the moment it is picked:
			// leave `/name ` in the draft with the caret after it, so the next
			// keystroke is the argument. The hint strip says what to type, and
			// the prefix is stripped again at submit.
			const parts = autocompleteTriggerParts(state);
			const prefix = `/${command.name} `;
			props.onMessageChange(`${parts.before}${prefix}${parts.after}`);
			setAutocomplete(null);
			runCommand(command);
			focusTextareaAtNextFrame(parts.before.length + prefix.length);
			return;
		}
		const caret = clearAutocompleteTrigger(state);
		runCommand(command);
		focusTextareaAtNextFrame(caret);
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

		if (item.command) {
			selectCommand(state, item.command);
			return;
		}

		insertAutocompleteItem(state, item);
	};

	// The "date" autocomplete (`detectTrigger`'s `date` branch, below) opens
	// ambiently on ANY typed word of 3+ letters that happens to prefix a day
	// name — "man", "fri", "tor"… all real, common Norwegian words on their
	// own — unlike the slash-command menu, which only opens after an explicit
	// "/" the user deliberately typed. Letting Enter confirm this ambient
	// suggestion (the same as Tab/Enter already do for a deliberately-opened
	// "/" menu) meant an ordinary sentence ending in one of those words
	// silently inserted a date instead of sending the message: F-03
	// (CHAT_PARITY_AUDIT_2026-09-15.md §3.1, §3.6) — reproduced with
	// "Send meg en oppsummering man" + Enter, which left the message unsent
	// and rewrote it to "…man. <next Monday>." instead.
	const isScheduleAutocomplete = (state: AutocompleteState) =>
		state.category === i18n.tr("Plan", "Schedule");

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
			case "Enter": {
				// Only an explicitly-opened menu (slash commands / actions) may
				// consume Enter. The ambient date suggestion stays reachable via
				// Tab, an arrow key + Tab, or a click — never via the same key
				// that sends the message.
				if (isScheduleAutocomplete(state)) return false;
				event.preventDefault();
				const item = state.items[autocompleteIndex()];
				if (item) applyAutocompleteSelection(item);
				return true;
			}
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
		// The command prefix is syntax, not prose: "/image en rød katt" sends
		// "en rød katt" with the image tool set, the way a command behaves
		// everywhere else. An unrecognised "/..." is left alone by `commandBody`.
		const body = commandBody().trim();
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

	/**
	 * The `minPrivacyTier` this turn carries. It used to be read straight off
	 * the loaded catalogue, so a turn sent before `/api/v1/models` answered
	 * went out with no tier: the loosest constraint rather than the model's.
	 * Now a catalogue model waits for the catalogue (bounded), then falls back
	 * to the tier last stored for this model, then to the strictest tier. An
	 * intent mode never waits: it carries no tier either way.
	 *
	 * Synchronous whenever no wait is needed (an intent mode, or a catalogue
	 * that has settled), so those sends reach `onSubmit` in the same tick as
	 * before; a Promise only while the catalogue is still in flight.
	 */
	const resolveTurnPrivacyTier = (
		model: string,
		provider: string | undefined,
	): PrivacyTier | undefined | Promise<PrivacyTier | undefined> => {
		if (isIntentModeSelection(model)) return undefined;
		const resolve = () =>
			resolveSubmitPrivacyTier({
				model,
				provider,
				catalogue: settledCatalogue ?? undefined,
				lastKnownTier: lastKnownAiModelPrivacyTier(activeOrgId() || "", {
					model,
					provider,
				}),
			}).tier;
		if (settledCatalogue !== undefined) return resolve();
		return waitForCatalogue(requestCatalogue(), CATALOGUE_TIER_WAIT_MS).then(
			resolve,
		);
	};

	const submitComposer = async (opts?: {
		/** A Voice mode turn: the dialog that locks the composer is the one sending. */
		fromVoiceMode?: boolean;
	}): Promise<ComposerSubmitResult> => {
		if (props.disabled || handoffPending()) return { ok: false };
		if (subscriptionConnectionPending())
			return {
				ok: false,
				message: i18n.tr(
					"Kontrollerer ChatGPT-abonnementet … Prøv igjen om et øyeblikk.",
					"Checking your ChatGPT subscription … Try again in a moment.",
				),
			};
		// A deliberately-opened menu (slash commands / actions) still blocks a
		// send — the user is mid-command. The ambient "date" suggestion must
		// NOT: it is the other half of the F-03 fix above. Without this, Enter
		// already stopped being swallowed by the keydown handler, but this
		// guard alone still silently dropped the submit whenever that
		// suggestion happened to be open (any message ending in "man", "fri",
		// "tor"…), which is the exact symptom F-03 reported.
		const pendingAutocomplete = autocomplete();
		if (
			(pendingAutocomplete && !isScheduleAutocomplete(pendingAutocomplete)) ||
			!hasContent() ||
			(voiceMode() && !opts?.fromVoiceMode) ||
			voiceRecording() ||
			(props.submitting && !props.allowMidRunSubmit)
		)
			return { ok: false };

		// A command whose required argument is still empty is unfinished, not a
		// message — sending it would ship the bare "/image" as prose. The hint
		// strip stays up and says what is missing.
		const pendingCommand = draftCommand();
		if (pendingCommand?.command.argument?.required && !pendingCommand.argument)
			return { ok: false };
		// Typed commands apply even when the menu was never opened: "/image en
		// rød katt" straight off the keyboard must carry the image tool, and
		// "/kvalitet rapport uke 12" must carry the skill.
		const commandBuiltin = pendingCommand?.command.builtin;
		const commandAction = pendingCommand?.command.specialized;

		// "/schedule" (and its Norwegian aliases) never becomes a chat turn: it
		// turns the typed request into a recurring job instead. Unlike
		// "web_search"/"deep_research"/"image" above, which just set a flag on
		// an otherwise-normal send, this must intercept BEFORE the normal
		// send machinery below ever runs — the goal is whatever followed the
		// command name (falling back to the raw draft for a bare "/schedule"
		// with no menu match, matching the argument's own "optional — uses the
		// draft" hint in `BUILTIN_COMMANDS`).
		if (commandBuiltin === "schedule") {
			const goal = (pendingCommand?.argument || props.message).trim();
			if (!goal) return { ok: false };
			setScheduleSeed({ title: scheduleTitleFrom(goal), goal, origin: "composer" });
			setScheduleDialogOpen(true);
			resetComposerDraft();
			return { ok: true };
		}

		const initialSnapshot = createSubmissionSnapshot();
		const submittedActions =
			commandAction && commandAction.kind !== "builtin" &&
			!initialSnapshot.actions.some(
				(a) => a.id === commandAction.id && a.kind === commandAction.kind,
			)
				? [
						...initialSnapshot.actions,
						{
							id: commandAction.id,
							name: commandAction.name,
							kind: commandAction.kind,
						},
					]
				: initialSnapshot.actions;
		const initialEffectiveBrowseWeb =
			browseWeb() || commandBuiltin === "web_search";
		const productResearch = shouldResearchLinkedProducts({
			browseWeb: initialEffectiveBrowseWeb,
			message: initialSnapshot.body,
		});
		const initialEffectiveDeepSearch =
			deepSearch() || productResearch || commandBuiltin === "deep_research";
		const subscriptionRoute = selectedSubscriptionRoute();
		if (subscriptionBacked() && !subscriptionRoute) {
			setStaleSubscriptionNotice(true);
			const message = i18n.tr("Koble til ChatGPT-abonnementet under Profil → ChatGPT-abonnement før du sender. Utkastet er beholdt.", "Connect your ChatGPT subscription under Profile → ChatGPT subscription before sending. Your draft has been kept.");
			setSubmitError(message);
			return { ok: false, message };
		}
		// Everything the payload is built from is read here, before the tier
		// lookup below may wait on the catalogue.
		const payloadInput = {
			actions: submittedActions,
			browseWeb: initialEffectiveBrowseWeb,
			deepSearch: initialEffectiveDeepSearch,
			files: initialSnapshot.files,
			imageMode: imageMode() || commandBuiltin === "image",
			model: selectedModel(),
			provider: subscriptionRoute?.provider,
			responseMode: responseMode(),
			subscriptionConnectionId: subscriptionRoute?.connectionId,
			text: initialSnapshot.submittedText,
			tone: settings().tone,
			zdr: temporaryChat(),
		};
		const modelProvider = selectedModelProvider();
		const submittedSourceScope = sourceScope();
		const submittedConversationOnly = conversationOnly();
		const submittedRetrievalScope = retrievalScope();

		// Pending from here, so the composer is inert (and a second Enter is
		// refused) while the tier lookup waits.
		setHandoffPending(true);
		setSubmitError(null);
		try {
			const turnTier = resolveTurnPrivacyTier(payloadInput.model, modelProvider);
			const payload = createComposerSubmitPayload({
				...payloadInput,
				minPrivacyTier: turnTier instanceof Promise ? await turnTier : turnTier,
			});
			payload.sourceScope = submittedSourceScope;
			// Omitted when 'auto' or conversation-only, matching the same
			// omit-when-default convention already used for effort/verbosity/
			// minPrivacyTier — a plain send stays byte-identical on the wire.
			if (!submittedConversationOnly && submittedRetrievalScope !== 'auto') {
				payload.retrievalScope = submittedRetrievalScope;
			}
			if (submittedConversationOnly) {
				payload.tools = [];
				payload.actions = [];
			}
			if (payload.attachments.length > 0) payload.attachments = await prepareChatAttachments(payload.attachments);
			if (props.onSubmit) await props.onSubmit(payload);
			else await writePendingChatLaunch(payload);
		} catch (error) {
			const message = error instanceof PendingChatLaunchError
				? error.reason === 'unreadable'
					? i18n.tr(`Kunne ikke lese «${error.filename}». Prøv igjen eller fjern filen.`, `Could not read “${error.filename}”. Retry or remove the file.`)
					: error.reason === 'too_large'
						? i18n.tr('Vedleggene er for store for chat. Velg mindre filer og prøv igjen.', 'The attachments are too large for chat. Choose smaller files and retry.')
						: i18n.tr('Kunne ikke overføre utkastet. Frigjør nettleserlagring og prøv igjen. Teksten og filene er beholdt.', 'Could not transfer the draft. Free browser storage and retry. Your text and files have been kept.')
				: error instanceof Error ? error.message : i18n.tr('Kunne ikke sende. Prøv igjen.', 'Could not send. Please retry.');
			setSubmitError(message);
			return { ok: false, message };
		} finally {
			setHandoffPending(false);
		}

		setTurns((current) =>
			[
				createComposerTurn({
					body: initialSnapshot.submittedText,
					browseWeb: initialEffectiveBrowseWeb,
					deepSearch: initialEffectiveDeepSearch,
					files: initialSnapshot.files,
					model: selectedModelLabel(),
					now: initialSnapshot.now,
					responseMode: responseMode(),
				}),
				...current,
			].slice(0, 6),
		);

		if (props.onSubmit) {
			resetComposerDraft();
			return { ok: true };
		}

		resetComposerDraft();
		props.onLaunchStart?.();
		// Navigate immediately inside a View Transition: the composer carries
		// `view-transition-name: verevon-composer`, so it morphs (travels + resizes) in
		// place into the chat page's composer instead of fading out first and
		// crossfading the whole route. Reduced-motion users get a plain navigate.
		navigateToChat(navigate);
		return { ok: true };
	};

	/**
	 * A spoken turn goes through the composer's own submit, so the model, the
	 * subscription route, Temporary chat, the source scope and the tools are
	 * exactly those of a typed message. Anything already typed is kept and sent
	 * with it, the way Voice input appends to the draft.
	 */
	const submitVoiceTurn = async (text: string): Promise<ComposerSubmitResult> => {
		const spoken = text.trim();
		if (!spoken) return { ok: false };
		const previousDraft = props.message;
		const typed = previousDraft.trim();
		flush(() => {
			// A menu left open behind the dialog is not a command in progress.
			setAutocomplete(null);
			props.onMessageChange(typed ? `${typed} ${spoken}` : spoken);
		});
		const result = await submitComposer({ fromVoiceMode: true });
		// The transcript stays in the dialog for another try; put back what was
		// typed so a retry does not send the spoken part twice.
		if (!result.ok) flush(() => props.onMessageChange(previousDraft));
		return result;
	};

	const closeVoiceMode = () => {
		flush(() => setVoiceMode(false));
		voiceModeButtonRef?.focus();
	};

	const insertVoiceTranscript = (text: string) => {
		const trimmed = props.message.trim();
		flush(() => {
			props.onMessageChange(trimmed ? `${trimmed} ${text}` : text);
			setVoiceMode(false);
		});
		focusTextareaAtNextFrame();
	};

	onCleanup(() => {
		composerDisposed = true;
		// Mid-take or still on the permission prompt: either way the microphone
		// is released, including a stream granted after this point.
		const take = voiceInputTake;
		voiceInputTake = undefined;
		take?.cancel();
		voiceInputDictation?.abort();
		voiceInputDictation = undefined;
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
			inert={props.disabled || handoffPending()}
			aria-busy={handoffPending() ? "true" : "false"}
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
					return;
				}

				const text = pastedPlainText(event);
				if (text.length > PASTE_TEXT_ATTACHMENT_THRESHOLD) {
					event.preventDefault();
					addFiles([
						pastedTextAttachmentFile(
							text,
							i18n.tr("Limt inn tekst", "Pasted text"),
						),
					]);
				}
			}}
		>
			<Show when={dragActive()}>
				<div class="dashboard-composer-drop-overlay">
					{i18n.tr("Slipp for å legge ved", "Drop to attach")}
				</div>
			</Show>

			<Show when={subscriptionConnectionPending()}>
				<div class="dashboard-composer-stale-model-notice" role="status" style={COMPOSER_NOTICE_STYLE}>
					<span>{i18n.tr("Kontrollerer ChatGPT-abonnementet …", "Checking your ChatGPT subscription …")}</span>
				</div>
			</Show>
			<Show when={staleSubscriptionNotice() && !subscriptionConnectionPending()}>
				<div
					class="dashboard-composer-stale-model-notice"
					role="status"
					style={COMPOSER_NOTICE_STYLE}
				>
					<Zap class="size-3.5" />
					<span>
						{i18n.tr(
							"ChatGPT-abonnementet er ikke lenger tilkoblet. Koble til på nytt under Profil → ChatGPT-abonnement, eller velg en annen modell.",
							"The ChatGPT subscription is no longer connected. Reconnect under Profile → ChatGPT subscription, or choose another model.",
						)}
					</span>
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
														`Kontekst for dette svaret: ${conversationOnly() ? "kun samtalen" : retrievalScope() === 'knowledge_only' ? "kun organisasjon" : retrievalScope() === 'web_only' ? "kun nett" : browseWeb() ? "kunnskap og nett" : "kunnskap"}`,
														`Context for this answer: ${conversationOnly() ? "conversation only" : retrievalScope() === 'knowledge_only' ? "organization only" : retrievalScope() === 'web_only' ? "web only" : browseWeb() ? "knowledge and web" : "knowledge"}`,
													)}
							onClick={() => {
								setContextScopeOpen((current) => !current);
								setModelOpen(false);
								setHistoryOpen(false);
								setSettingsOpen(false);
								setSuggestionsOpen(false);
							}}
							variant="chip"
							class={props.appearance === "chat" && chatAppearanceActive("context") ? "dashboard-composer-context-button" : undefined}
													>
														<BookOpen class="size-3.5" />
													<Show when={props.appearance === 'chat' && chatAppearanceActive('context')}><span class={`dashboard-composer-context-label${conversationOnly() ? ' dashboard-composer-context-label--restricted' : ''}`} aria-hidden="true">
														{conversationOnly()
															? i18n.tr("Kun samtalen", "Conversation only")
															: retrievalScope() === 'knowledge_only'
																? i18n.tr("Kun organisasjon", "Organization only")
																: retrievalScope() === 'web_only'
																	? i18n.tr("Kun nett", "Web only")
																	: browseWeb() ? i18n.tr("Kunnskap + nett", "Knowledge + web") : i18n.tr("Kunnskap", "Knowledge")}
													</span></Show>
													</ComposerIconButton>
													<Show when={!(props.appearance === 'chat' && chatAppearanceActive('context'))}><span class={`dashboard-composer-context-label${conversationOnly() ? ' dashboard-composer-context-label--restricted' : ''}`} aria-hidden="true">
														{conversationOnly()
															? i18n.tr("Kun samtalen", "Conversation only")
															: retrievalScope() === 'knowledge_only'
																? i18n.tr("Kun organisasjon", "Organization only")
																: retrievalScope() === 'web_only'
																	? i18n.tr("Kun nett", "Web only")
																	: browseWeb() ? i18n.tr("Kunnskap + nett", "Knowledge + web") : i18n.tr("Kunnskap", "Knowledge")}
													</span></Show>
						<Show when={contextScopeOpen()}>
							<div class="dashboard-composer-context-panel verevon-popover" role="dialog" aria-label={i18n.tr("Kontekst for svaret", "Answer context")}>
								<div class="dashboard-composer-context-panel__heading">
									<BookOpen class="size-4" />
									<strong>{i18n.tr("Kontekst", "Context")}</strong>
								</div>
								<p>{i18n.tr("Dette er hva Verevon kan bruke i denne meldingen.", "This is what Verevon can use for this message.")}</p>
								<label class="grid gap-2 text-sm">
									<span>{i18n.tr("Kilder for samtalen", "Conversation sources")}</span>
									<select class="rounded-md border border-current/20 bg-transparent p-2" value={sourceScope()} disabled={props.sourceScopeLocked || props.submitting} onChange={(event) => {
										const value = event.currentTarget.value === 'conversation' ? 'conversation' : 'workspace';
										setInternalSourceScope(value); props.onSourceScopeChange?.(value);
										if (value === 'conversation') { setBrowseWeb(false); setDeepSearch(false); props.onImageModeChange?.(false); props.onPlanModeChange?.(false); }
									}}>
										<option value="workspace">{i18n.tr("Tillatte arbeidsområdekilder", "Permitted workspace sources")}</option>
										<option value="conversation">{i18n.tr("Kun samtalen og vedlegg", "Conversation and attachments only")}</option>
									</select>
								</label>
								<Show when={!conversationOnly()}>
									<label class="grid gap-2 text-sm">
										<span>{i18n.tr("Kildefilter for denne meldingen", "Source filter for this message")}</span>
										<select class="rounded-md border border-current/20 bg-transparent p-2" value={retrievalScope()} disabled={props.submitting} onChange={(event) => {
											const raw = event.currentTarget.value;
											setRetrievalScope(raw === 'knowledge_only' || raw === 'web_only' ? raw : 'auto');
										}}>
											<option value="auto">{i18n.tr("Kunnskap + nett", "Knowledge + web")}</option>
											<option value="knowledge_only">{i18n.tr("Kun organisasjon", "Organization only")}</option>
											<option value="web_only">{i18n.tr("Kun nett", "Web only")}</option>
										</select>
									</label>
								</Show>
								<Show when={conversationOnly()}>
									<p>{i18n.tr("Bare tekst og vedlegg i denne samtalen brukes. Minner, organisasjonskunnskap, nettkilder og tilkoblede tjenester er av. Samtalen lagres, men brukes ikke til nye minner eller læring.", "Only this conversation and its attachments are used. Memory, workspace knowledge, web sources and connected services are off. The conversation is saved, but does not create memories or learned skills.")}</p>
								</Show>
								<Show when={props.sourceScopeLocked}><p>{i18n.tr("Start en ny samtale for å endre kildevalg.", "Start a new conversation to change sources.")}</p></Show>
								<Show when={!conversationOnly()}>
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
											? i18n.tr(
                                                `${files().length} vedlegg tas med i samtalen. Du får beskjed hvis en fil ikke kan leses.`,
                                                `${files().length} attachments will be included in this conversation. You will be told if a file cannot be read.`,
                                            )
                                            : i18n.tr("Ingen filer lagt ved.", "No files attached.")}</small></span>
									</li>
									<li>
										<span class={cn("dashboard-composer-context-dot", deepSearch() ? "dashboard-composer-context-dot--on" : "dashboard-composer-context-dot--off")} />
										<span><strong>{i18n.tr("Dyp research", "Deep research")}</strong><small>{deepSearch() ? i18n.tr("Utvidet research er aktivert.", "Extended research is enabled.") : i18n.tr("Ikke aktivert.", "Not enabled.")}</small></span>
									</li>
								</ul>
                                </Show>
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
										disabled={subscriptionBacked() || conversationOnly()}
										label={
											subscriptionBacked()
												? `${i18n.tr("Utfør", "Do")} — ${subscriptionDisabledTitle()}`
												: i18n.tr(
													planMode()
														? "Do-modus - agenten planlegger og ber om godkjenning før risikable verktøy"
														: "Ask-modus - les, hent og foreslå uten å utføre risikable handlinger",
													planMode()
														? "Do mode - the agent plans and asks for approval before risky tools"
														: "Ask mode - read, retrieve, and propose without executing risky actions",
												)
										}
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
									disabled={subscriptionBacked() || conversationOnly()}
									title={subscriptionBacked() ? subscriptionDisabledTitle() : undefined}
									onClick={() => props.onPlanModeChange?.(true)}
									style={subscriptionDisabledStyle()}
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
					accept="image/*,.pdf,.docx,.xlsx,.pptx,.txt,.md,.markdown,.csv,.json,.html,.htm"
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
								note={
									subscriptionBacked() &&
									!isScheduleAutocomplete(state())
										? i18n.tr(
												"Søk, bilde, dyp research og ferdigheter er skjult — ikke tilgjengelig med denne modellen.",
												"Search, image, deep research and skills are hidden — not available with this model.",
											)
										: undefined
								}
								selectedIndex={autocompleteIndex()}
								onHover={setAutocompleteIndex}
								onSelect={applyAutocompleteSelection}
							/>
						</div>
					)}
				</Show>

				{/* Deliberately NOT an `autocomplete()` state: everything held in that
				    signal blocks submit (the F-03 rule), and a command that already
				    has its argument must send on Enter like any other message. This
				    is a read-only strip that says which command the draft is
				    carrying and what it still wants. */}
				<Show when={draftCommand()}>
					{(parsed) => (
						<div
							class="dashboard-composer-command-hint"
							role="status"
							data-command={parsed().command.name}
							style={COMPOSER_NOTICE_STYLE}
						>
							<Dynamic
								component={parsed().command.icon}
								class="size-3.5"
							/>
							<span
								class="dashboard-composer-command-hint__name"
								style={{ "font-weight": "600" }}
							>
								{`/${parsed().command.name}`}
							</span>
							<span class="dashboard-composer-command-hint__description">
								{i18n.tr(
									parsed().command.description.no,
									parsed().command.description.en,
								)}
							</span>
							<Show when={parsed().command.argument}>
								{(argument) => (
									<span
										class="dashboard-composer-command-hint__arg"
										style={{ opacity: "0.75" }}
									>
										{parsed().argument
											? i18n.tr(
													"Trykk Enter for å sende",
													"Press Enter to send",
												)
											: i18n.tr(
													`Skriv ${argument().hint.no}`,
													`Type a ${argument().hint.en}`,
												)}
									</span>
								)}
							</Show>
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
					<Show when={submitError()}>
						<p class="dashboard-composer-attachments__notice" role="alert">{submitError()}</p>
					</Show>
					{/* Seen after a press on Voice input in Temporary chat; screen
					    readers get the same words from its description and status. */}
					<Show when={temporaryChat() && voiceInputBlockedShown()}>
						<p class="dashboard-composer-attachments__notice" aria-hidden="true">
							{voiceInputBlockedMessage(i18n.tr)}
						</p>
					</Show>
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
							placeholder={props.compact ? i18n.tr("Spør om hva som helst …", "Ask anything…") : textareaPlaceholder(
								modeAnnouncement(),
								i18n,
								activeSuggestion(),
							)}
							rows={props.compact ? '1' : '3'}
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
					<p class="sr-only" role="status">
						{voiceInputError()}
					</p>
					<Show when={temporaryChat()}>
						<span id={voiceModeBlockedId} class="sr-only">
							{voiceModeBlockedMessage(i18n.tr)}
						</span>
						<span id={voiceInputBlockedId} class="sr-only">
							{voiceInputBlockedMessage(i18n.tr)}
						</span>
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
								disabled={subscriptionBacked() || conversationOnly() || retrievalScope() === 'knowledge_only'}
								label={
									subscriptionBacked()
										? `${i18n.tr("Dyp research", "Deep search")} — ${subscriptionDisabledTitle()}`
										: retrievalScope() === 'knowledge_only'
											? `${i18n.tr("Dyp research", "Deep search")} — ${i18n.tr("Ikke tilgjengelig med kildefilteret Kun organisasjon", "Not available with the Organization only source filter")}`
											: i18n.tr("Dyp research", "Deep search")
								}
								onClick={() =>
									setDeepSearch((current) => !current)
								}
								variant="toolbar"
							>
								<Telescope class="size-4" />
							</ComposerIconButton>
							<button
								type="button"
								aria-pressed={(browseWeb() || retrievalScope() === 'web_only') ? "true" : "false"}
								aria-label={
									subscriptionBacked()
										? `${i18n.tr("Søk på nett", "Browse web")} — ${subscriptionDisabledTitle()}`
										: retrievalScope() === 'web_only'
											? `${i18n.tr("Søk på nett", "Browse web")} — ${webOnlyForcedTitle()}`
											: i18n.tr("Søk på nett", "Browse web")
								}
								title={
									subscriptionBacked()
										? subscriptionDisabledTitle()
										: retrievalScope() === 'web_only'
											? webOnlyForcedTitle()
											: i18n.tr("Søk på nett", "Browse web")
								}
								disabled={subscriptionBacked() || conversationOnly() || retrievalScope() === 'web_only'}
								onClick={() =>
									setBrowseWeb((current) => !current)
								}
								class={composerWebButtonClass(browseWeb() || retrievalScope() === 'web_only')}
								style={retrievalScope() === 'web_only' ? { opacity: 0.85, cursor: "not-allowed" } : subscriptionDisabledStyle()}
							>
								<Globe2 class="size-4" />
								{i18n.tr("Søk", "Search")}
							</button>
							<Show when={props.onImageModeChange}>
								<button
									type="button"
									aria-pressed={imageMode() ? "true" : "false"}
									aria-label={
										subscriptionBacked()
											? `${i18n.tr("Generer bilde", "Generate image")} — ${subscriptionDisabledTitle()}`
											: i18n.tr(
												"Generer bilde",
												"Generate image",
											)
									}
									title={
										subscriptionBacked()
											? subscriptionDisabledTitle()
											: i18n.tr(
												"Generer bilde",
												"Generate image",
											)
									}
									disabled={subscriptionBacked() || conversationOnly()}
									onClick={() =>
										props.onImageModeChange?.(!imageMode())
									}
									class={composerImageButtonClass(
										imageMode(),
									)}
									style={subscriptionDisabledStyle()}
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
							{/* In Temporary chat this opens Voice mode in its blocked
							    state, which says why and never asks for the microphone. */}
							<ComposerIconButton
								active={voiceMode()}
								buttonRef={(element) => {
									voiceModeButtonRef = element;
								}}
								describedBy={temporaryChat() ? voiceModeBlockedId : undefined}
								disabled={voiceInputHoldsMic()}
								hasPopup="dialog"
								label={i18n.tr("Stemmemodus", "Voice mode")}
								onClick={() => setVoiceMode(true)}
								title={voiceModeTitle()}
								variant="toolbar"
							>
								<AudioWaveform class="size-4" />
							</ComposerIconButton>
							{/* aria-disabled in Temporary chat: focusable, so the
							    reason is announced, and a press shows it. */}
							<ComposerIconButton
								active={voiceRecording()}
								describedBy={temporaryChat() ? voiceInputBlockedId : undefined}
								disabled={voiceMode()}
								label={i18n.tr("Stemmeinndata", "Voice input")}
								onClick={() => void toggleRecording()}
								title={voiceInputTitle()}
								unavailable={temporaryChat()}
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
						temporaryChat={temporaryChat()}
						onAddFiles={() => {
							setSettingsOpen(false);
							openFileDialog();
						}}
						onAttachFile={attachConnectorFile}
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
					<VoiceModeDialog
						awaitReply={props.voiceReply !== undefined}
						language={settings().voiceLang}
						languageLabel={voiceLanguageLabel()}
						model={selectedModelLabel()}
						onClose={closeVoiceMode}
						onInsert={insertVoiceTranscript}
						onSend={submitVoiceTurn}
						onStop={props.onStop}
						reply={props.voiceReply}
						submitting={props.submitting}
						temporaryChat={temporaryChat()}
					/>
				</Portal>
			</Show>
			<Show when={scheduleDialogOpen()}>
				<Portal>
					<ScheduleDialog
						onClose={() => setScheduleDialogOpen(false)}
						seedTitle={scheduleSeed().title}
						seedGoal={scheduleSeed().goal}
						threadOriginHint={scheduleSeed().origin}
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
	// The "/image " sniff that used to sit here is gone: the command prefix is
	// now parsed and stripped before submit, so `imageMode` already carries it
	// (and carried only the English spelling before, silently failing for a
	// Norwegian-labelled shortcut).
	if (input.imageMode) tools.push("image");
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
			...(file.extractedText !== undefined
				? { extractedText: file.extractedText }
				: {}),
			...(file.source ? { source: file.source } : {}),
			...(file.sourceItemId ? { sourceItemId: file.sourceItemId } : {}),
			...(file.connectionId ? { connectionId: file.connectionId } : {}),
			...(file.fetchedAt ? { fetchedAt: file.fetchedAt } : {}),
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
		fileSources: input.files.map((file) => file.source),
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

	const slashMatch = SLASH_TRIGGER_PATTERN.exec(before);
	if (slashMatch) {
		const query = slashMatch[1] ?? "";
		// Only the "/" and the token after it are the trigger — any leading
		// whitespace the pattern allowed through stays in the draft.
		const rawLen = query.length + 1;
		return {
			type: "slash",
			query: query.toLowerCase(),
			start: position - rawLen,
			rawLen,
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
	/** Why the list is shorter than usual, when something is being withheld. */
	note?: string;
	onHover: (index: number) => void;
	onSelect: (item: AutocompleteItem) => void;
	selectedIndex: number;
}) {
	return (
		<div
			class="dashboard-composer-autocomplete-menu verevon-popover verevon-popover-up"
			data-composer-floating-panel="true"
			role="listbox"
			aria-label={props.category}
			style={COMMAND_MENU_STYLE}
		>
			<div class="dashboard-composer-autocomplete-menu__category">
				{props.category}
			</div>
			<For each={props.items}>
				{(item, index) => (
					<button
						type="button"
						role="option"
						aria-selected={
							index() === props.selectedIndex ? "true" : "false"
						}
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
						<span
							class="dashboard-composer-autocomplete-menu__body"
							style={COMMAND_BODY_STYLE}
						>
							<span
								class="dashboard-composer-autocomplete-menu__label"
								style={COMMAND_LABEL_STYLE}
							>
								{item.label}
								{/* The argument is part of the command's signature, so
								    it belongs next to the name rather than in a
								    tooltip nobody opens. */}
								<Show when={item.argumentHint}>
									{(hint) => (
										<span
											class="dashboard-composer-autocomplete-menu__arg"
											style={COMMAND_ARGUMENT_STYLE}
										>
											{`<${hint()}>`}
										</span>
									)}
								</Show>
							</span>
							<Show when={item.description}>
								{(description) => (
									<span
										class="dashboard-composer-autocomplete-menu__description"
										style={COMMAND_DESCRIPTION_STYLE}
									>
										{description()}
									</span>
								)}
							</Show>
						</span>
					</button>
				)}
			</For>
			<Show when={props.note}>
				{(note) => (
					<p
						class="dashboard-composer-autocomplete-menu__note"
						style={COMMAND_NOTE_STYLE}
					>
						{note()}
					</p>
				)}
			</Show>
		</div>
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

/**
 * Sibling of createFilePreviewUrl for a Blob that never was a browser File —
 * the connector-attach picker builds one from a fetched provider file's
 * base64 content or from a synthesized mail/calendar text block.
 */
function createFilePreviewUrlFromBlob(blob: Blob) {
	if (
		typeof URL === "undefined" ||
		typeof URL.createObjectURL !== "function"
	) {
		return "";
	}

	return URL.createObjectURL(blob);
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
							<div
								class="dashboard-composer-attachment"
								data-attachment-source={attachment.source}
							>
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

const CONNECTOR_SOURCE_LABELS: Record<ComposerAttachmentSource, { no: string; en: string }> = {
	onedrive: { no: "OneDrive", en: "OneDrive" },
	gdrive: { no: "Google Disk", en: "Google Drive" },
	gmail: { no: "Gmail", en: "Gmail" },
	outlook: { no: "Outlook", en: "Outlook" },
	gcal: { no: "Google Kalender", en: "Google Calendar" },
	"outlook-cal": { no: "Outlook-kalender", en: "Outlook Calendar" },
};

/** "(1 fra OneDrive, 1 fra Outlook)" — empty string when no attachment in
 * the turn carries provenance (an ordinary local upload/paste/screenshot
 * turn stays byte-identical to today's receipt line). */
function connectorSourceBreakdown(
	sources: Array<ComposerAttachmentSource | undefined>,
	i18n: ReturnType<typeof useI18n>,
): string {
	const counts = new Map<ComposerAttachmentSource, number>();
	for (const source of sources) {
		if (!source) continue;
		counts.set(source, (counts.get(source) ?? 0) + 1);
	}
	if (counts.size === 0) return "";
	const parts = Array.from(counts.entries()).map(([source, count]) => {
		const label = CONNECTOR_SOURCE_LABELS[source];
		return i18n.tr(`${count} fra ${label.no}`, `${count} from ${label.en}`);
	});
	return ` (${parts.join(", ")})`;
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
							{connectorSourceBreakdown(props.turn.fileSources, props.i18n)}
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
		if (!historyQuery().trim()) return allItems();
		return allItems().filter(
			(item) =>
				item.title.toLowerCase().includes(historyQuery().trim().toLowerCase()) ||
				item.meta.toLowerCase().includes(historyQuery().trim().toLowerCase()),
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
				<a href="/chat" link onClick={() => props.onClose()} class="verevon-menu-row">
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
	onAttachFile: (file: ComposerFile) => void;
	onScreenshot: () => void;
	onSettingsChange: (settings: ComposerSettings) => void;
	position: PanelPosition;
	settings: ComposerSettings;
	/** ZDR gate (connector-picker spec §4): a temporary-chat turn must never
	 * be able to pull in durable-account connector content — fail closed by
	 * disabling the picker's attach action, not just hiding a row. */
	temporaryChat: boolean;
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
							<ComposerMenuRow
								icon={
									<UploadCloud
										class="size-[17px]"
										strokeWidth={1.7}
									/>
								}
								label={props.i18n.tr(
									"Fra en tilkoblet kilde",
									"From a connected source",
								)}
								onClick={() => setView("attach-source")}
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

					<Match when={view() === "attach-source"}>
						<SettingsSubView
							title={props.i18n.tr(
								"Fra en tilkoblet kilde",
								"From a connected source",
							)}
							onBack={() => setView("main")}
						>
							<ConnectorAttachPicker
								i18n={props.i18n}
								disabled={props.temporaryChat}
								onAttach={props.onAttachFile}
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

// --- Connector-attach picker ("Fra en tilkoblet kilde") ---
// Composer entry point for attaching a file, mail message or calendar event
// read live from a connected OneDrive-personal / Google-My-Drive / Outlook /
// Gmail / Google-Calendar / Outlook-Calendar connection. SharePoint team-site
// libraries are explicitly out of scope for this picker (owner decision,
// connector-picker spec §6.1). See connector-picker.md for the full design.

type ConnectorBrowseKind = "files" | "mail" | "calendar";

/** Un-resolved {no, en} labels, resolved with i18n.tr at render time (inside
 * JSX, a tracked scope) rather than once at component setup — otherwise a
 * language switch would leave these three tab labels stale. */
const CONNECTOR_KIND_OPTIONS: Array<{
	value: ConnectorBrowseKind;
	label: { no: string; en: string };
	icon: Component<LucideProps>;
}> = [
	{ value: "files", label: { no: "Filer", en: "Files" }, icon: FileText },
	{ value: "mail", label: { no: "E-post", en: "Mail" }, icon: Mail },
	{ value: "calendar", label: { no: "Kalender", en: "Calendar" }, icon: Calendar },
];

type ConnectorBrowseRow = {
	id: string;
	title: string;
	subtitle?: string;
	/** Undefined when the provider's listing never carries a size (a native
	 * Google Doc/Sheet/Slide, or Gmail's message list) — the row shows "—"
	 * rather than fabricating a number, and the actual cap is still enforced
	 * server-side on the content fetch either way. */
	sizeBytes?: number;
	mimeType?: string;
	raw: Record<string, unknown>;
};

function stringOf(value: unknown): string | undefined {
	return typeof value === "string" && value.trim() ? value : undefined;
}

function numberOf(value: unknown): number | undefined {
	return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function recordOf(value: unknown): Record<string, unknown> | undefined {
	return value && typeof value === "object" && !Array.isArray(value)
		? (value as Record<string, unknown>)
		: undefined;
}

function browseOperationFor(
	providerKey: "microsoft" | "google",
	kind: ConnectorBrowseKind,
): ConnectorBrowseOperation {
	if (kind === "files") return "drive.files";
	if (kind === "calendar") return "calendar.events";
	return providerKey === "microsoft" ? "mail.messages" : "gmail.messages";
}

/** Graph/Drive/Gmail/Calendar each name their list array differently. */
function browseResultArray(
	providerKey: "microsoft" | "google",
	kind: ConnectorBrowseKind,
	result: unknown,
): Record<string, unknown>[] {
	const record = recordOf(result);
	if (!record) return [];
	const key =
		providerKey === "microsoft"
			? "value" // Graph list responses always wrap in { value: [...] }
			: kind === "files"
				? "files"
				: kind === "calendar"
					? "items"
					: "messages";
	const array = record[key];
	return Array.isArray(array)
		? array.filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === "object")
		: [];
}

function connectorBrowseRow(
	i18n: ReturnType<typeof useI18n>,
	providerKey: "microsoft" | "google",
	kind: ConnectorBrowseKind,
	item: Record<string, unknown>,
): ConnectorBrowseRow | null {
	const id = stringOf(item.id);
	if (!id) return null;

	if (kind === "files") {
		if (providerKey === "microsoft") {
			if (item.folder) return null; // a folder cannot be attached
			return {
				id,
				title: stringOf(item.name) ?? i18n.tr("uten navn", "untitled"),
				subtitle: formatConnectorRowDate(stringOf(item.lastModifiedDateTime) ?? ""),
				sizeBytes: numberOf(item.size),
				mimeType: stringOf(recordOf(item.file)?.mimeType),
				raw: item,
			};
		}
		const mimeType = stringOf(item.mimeType);
		if (mimeType === "application/vnd.google-apps.folder") return null;
		return {
			id,
			title: stringOf(item.name) ?? i18n.tr("uten navn", "untitled"),
			subtitle: formatConnectorRowDate(stringOf(item.modifiedTime) ?? ""),
			sizeBytes: numberOf(item.size),
			mimeType,
			raw: item,
		};
	}

	if (kind === "mail") {
		if (providerKey === "microsoft") {
			const emailAddress = recordOf(recordOf(item.from)?.emailAddress);
			const from = emailAddress && (stringOf(emailAddress.name) ?? stringOf(emailAddress.address));
			return {
				id,
				title: stringOf(item.subject) ?? i18n.tr("uten emne", "no subject"),
				subtitle: from ?? formatConnectorRowDate(stringOf(item.receivedDateTime) ?? ""),
				raw: item,
			};
		}
		// Gmail's own list endpoint returns only id + threadId (confirmed in
		// connector-picker spec §1 — a search limitation, not something a
		// caller-supplied param fixes). A subject/from would have to come
		// from a further per-message get, which would turn a search result
		// page into N extra network calls; showing a generic label here
		// instead of fabricating fields the list never carries is the
		// honest reading.
		return { id, title: `${i18n.tr("E-post", "Mail")} ${id.slice(0, 10)}`, raw: item };
	}

	// calendar
	if (providerKey === "microsoft") {
		const start = stringOf(recordOf(item.start)?.dateTime);
		return {
			id,
			title: stringOf(item.subject) ?? i18n.tr("uten tittel", "untitled"),
			subtitle: start ? formatConnectorRowDate(start) : undefined,
			raw: item,
		};
	}
	const start = stringOf(recordOf(item.start)?.dateTime) ?? stringOf(recordOf(item.start)?.date);
	return {
		id,
		title: stringOf(item.summary) ?? i18n.tr("uten tittel", "untitled"),
		subtitle: start ? formatConnectorRowDate(start) : undefined,
		raw: item,
	};
}

function formatConnectorRowDate(value: string): string {
	if (!value) return "";
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return value;
	return date.toLocaleString(undefined, {
		day: "2-digit",
		month: "2-digit",
		year: "numeric",
		hour: "2-digit",
		minute: "2-digit",
	});
}

/** Minimal client-side HTML→text rendering for a calendar event's
 * description (Graph's `/me/events` never trims/strips the body — see
 * connector-picker spec §1). Display-quality only; the durable-record
 * HTML-stripping the mail path relies on lives server-side in
 * integration-corev2's internal/htmlstrip package. */
function stripHtmlClient(html: string): string {
	return html
		.replace(/<(script|style)[^>]*>[\s\S]*?<\/\1>/gi, " ")
		.replace(/<br\s*\/?>/gi, "\n")
		.replace(/<\/(p|div|li)>/gi, "\n")
		.replace(/<[^>]+>/g, " ")
		.replace(/&nbsp;/g, " ")
		.replace(/&amp;/g, "&")
		.replace(/&lt;/g, "<")
		.replace(/&gt;/g, ">")
		.replace(/&quot;/g, '"')
		.replace(/&#39;/g, "'")
		.replace(/[ \t]+/g, " ")
		.replace(/\n{3,}/g, "\n\n")
		.trim();
}

function base64ToUint8Array(base64: string): Uint8Array<ArrayBuffer> {
	const binary = atob(base64);
	const bytes = new Uint8Array(binary.length);
	for (let index = 0; index < binary.length; index += 1) {
		bytes[index] = binary.charCodeAt(index);
	}
	return bytes;
}

/** Synthesizes the mail attachment's plain-text body: "Emne/Fra/Dato" then
 * the message body. Microsoft's mail.messages.get result is the raw Graph
 * message (+ an added bodyText field); Google's is a flat synthesized
 * object — see integration-corev2's service.go for both shapes. */
function formatConnectorMailText(
	i18n: ReturnType<typeof useI18n>,
	providerKey: "microsoft" | "google",
	result: unknown,
): { subject: string; text: string } {
	const record = recordOf(result) ?? {};
	const bodyText = stringOf(record.bodyText) ?? "";
	let subject: string;
	let from: string;
	let date: string;
	if (providerKey === "microsoft") {
		subject = stringOf(record.subject) ?? "";
		const emailAddress = recordOf(recordOf(record.from)?.emailAddress);
		from = (emailAddress && (stringOf(emailAddress.name) ?? stringOf(emailAddress.address))) ?? "";
		date = stringOf(record.receivedDateTime) ?? "";
	} else {
		subject = stringOf(record.subject) ?? "";
		from = stringOf(record.from) ?? "";
		date = stringOf(record.date) ?? "";
	}
	const text = [
		`${i18n.tr("Emne", "Subject")}: ${subject}`,
		`${i18n.tr("Fra", "From")}: ${from}`,
		`${i18n.tr("Dato", "Date")}: ${formatConnectorRowDate(date) || date}`,
		"",
		bodyText,
	].join("\n");
	return { subject: subject || i18n.tr("uten emne", "no subject"), text };
}

/** Synthesizes the calendar attachment's plain-text body. Calendar never
 * calls the content endpoint — the browse step's row already carries the
 * full event (connector-picker spec §1's finding that neither provider
 * trims the event resource), so this reads straight from `row.raw`. */
function formatConnectorEventText(
	i18n: ReturnType<typeof useI18n>,
	providerKey: "microsoft" | "google",
	raw: Record<string, unknown>,
): { title: string; text: string } {
	let title: string;
	let time: string;
	let location: string;
	let attendees: string;
	let description: string;
	if (providerKey === "microsoft") {
		title = stringOf(raw.subject) ?? "";
		time = stringOf(recordOf(raw.start)?.dateTime) ?? "";
		location = stringOf(recordOf(raw.location)?.displayName) ?? "";
		attendees = (Array.isArray(raw.attendees) ? raw.attendees : [])
			.map((entry) => {
				const emailAddress = recordOf(recordOf(entry)?.emailAddress);
				return emailAddress && (stringOf(emailAddress.name) ?? stringOf(emailAddress.address));
			})
			.filter((value): value is string => Boolean(value))
			.join(", ");
		const bodyField = recordOf(raw.body);
		const rawDescription = bodyField ? (stringOf(bodyField.content) ?? "") : "";
		description = stringOf(bodyField?.contentType)?.toLowerCase() === "html"
			? stripHtmlClient(rawDescription)
			: rawDescription;
	} else {
		title = stringOf(raw.summary) ?? "";
		time = stringOf(recordOf(raw.start)?.dateTime) ?? stringOf(recordOf(raw.start)?.date) ?? "";
		location = stringOf(raw.location) ?? "";
		attendees = (Array.isArray(raw.attendees) ? raw.attendees : [])
			.map((entry) => {
				const record = recordOf(entry);
				return record && (stringOf(record.displayName) ?? stringOf(record.email));
			})
			.filter((value): value is string => Boolean(value))
			.join(", ");
		description = stringOf(raw.description) ?? "";
	}
	const text = [
		`${i18n.tr("Møte", "Meeting")}: ${title}`,
		`${i18n.tr("Tid", "Time")}: ${formatConnectorRowDate(time) || time}`,
		`${i18n.tr("Sted", "Location")}: ${location}`,
		`${i18n.tr("Deltakere", "Attendees")}: ${attendees}`,
		"",
		description,
	].join("\n");
	return { title: title || i18n.tr("uten tittel", "untitled"), text };
}

const CONNECTOR_TEXT_EXTRACT_EXTENSIONS = /\.(pdf|docx|xlsx|pptx)$/i;
const CONNECTOR_TEXT_INGEST_EXTENSIONS = /\.(txt|md|markdown|csv|json|html|htm)$/i;

/**
 * Turns one picked search result into a ComposerFile pushed onto the same
 * `files` signal a local upload uses (connector-picker spec §2.4 step 5) —
 * everything downstream (AttachmentPreview, removeFile, submit) is reused
 * unchanged. Throws a localized, user-facing message on any failure; the
 * caller is responsible for catching it and never lets a bad pick silently
 * become an unreadable chip.
 */
async function buildComposerFileFromPick(input: {
	i18n: ReturnType<typeof useI18n>;
	orgId: string;
	tab: ConnectorPickerTab;
	kind: ConnectorBrowseKind;
	row: ConnectorBrowseRow;
}): Promise<ComposerFile> {
	const { i18n, orgId, tab, kind, row } = input;
	const fetchedAt = new Date().toISOString();
	const fileId = `connector-${tab.providerKey}-${row.id}-${Date.now()}`;

	if (kind === "calendar") {
		const { title, text } = formatConnectorEventText(i18n, tab.providerKey, row.raw);
		const blob = new Blob([text], { type: "text/plain" });
		return {
			id: fileId,
			name: `${i18n.tr("Kalender", "Calendar")} – ${title}.txt`,
			size: blob.size,
			type: "text/plain",
			url: createFilePreviewUrlFromBlob(blob),
			source: tab.providerKey === "microsoft" ? "outlook-cal" : "gcal",
			sourceItemId: row.id,
			connectionId: tab.connectionId,
			fetchedAt,
		};
	}

	if (kind === "mail") {
		const operation: ConnectorContentOperation =
			tab.providerKey === "microsoft" ? "mail.messages.get" : "gmail.messages.get";
		const params = tab.providerKey === "microsoft" ? { messageId: row.id } : { id: row.id };
		const { result } = await fetchConnectionContent(orgId, tab.connectionId, operation, params);
		const { subject, text } = formatConnectorMailText(i18n, tab.providerKey, result);
		const blob = new Blob([text], { type: "text/plain" });
		return {
			id: fileId,
			name: `${i18n.tr("E-post", "Mail")} – ${subject}.txt`,
			size: blob.size,
			type: "text/plain",
			url: createFilePreviewUrlFromBlob(blob),
			source: tab.providerKey === "microsoft" ? "outlook" : "gmail",
			sourceItemId: row.id,
			connectionId: tab.connectionId,
			fetchedAt,
		};
	}

	// files
	const operation: ConnectorContentOperation = "drive.file.content";
	const params =
		tab.providerKey === "microsoft"
			? { itemId: row.id }
			: { fileId: row.id, mimeType: row.mimeType };
	const { result } = await fetchConnectionContent(orgId, tab.connectionId, operation, params);
	const content = recordOf(result) ?? {};
	const contentBase64 = stringOf(content.content_base64) ?? "";
	const mimeType = stringOf(content.mime_type) || row.mimeType || "application/octet-stream";
	const exportExtension = stringOf(content.export_extension) ?? "";
	const name =
		exportExtension && !row.title.toLowerCase().endsWith(exportExtension)
			? `${row.title}${exportExtension}`
			: row.title;

	if (mimeType.startsWith("image/")) {
		return {
			id: fileId,
			name,
			size: numberOf(content.size) ?? 0,
			type: mimeType,
			url: `data:${mimeType};base64,${contentBase64}`,
			source: tab.providerKey === "microsoft" ? "onedrive" : "gdrive",
			sourceItemId: row.id,
			connectionId: tab.connectionId,
			fetchedAt,
		};
	}

	const bytes = base64ToUint8Array(contentBase64);
	const blob = new Blob([bytes], { type: mimeType });
	let extractedText: string | undefined;
	if (CONNECTOR_TEXT_EXTRACT_EXTENSIONS.test(name)) {
		// Pre-flight readability check (connector-picker spec §6.2): the
		// network fetch already happened above, so extracting immediately is
		// free and lets the picker refuse a bad file before it becomes a
		// chip, instead of failing silently at send time.
		try {
			extractedText = (await extractChatDocument(name, contentBase64, mimeType)).content;
		} catch {
			throw new Error(
				i18n.tr(
					`Kunne ikke lese «${name}». Prøv igjen eller velg en annen fil.`,
					`Could not read "${name}". Try again or choose a different file.`,
				),
			);
		}
	} else if (mimeType.startsWith("text/") || CONNECTOR_TEXT_INGEST_EXTENSIONS.test(name)) {
		// Already have the bytes — decoding locally is free, no second
		// network round trip needed for a plain-text file.
		extractedText = new TextDecoder("utf-8").decode(bytes).trim();
	}

	return {
		id: fileId,
		name,
		size: blob.size,
		type: mimeType,
		url: createFilePreviewUrlFromBlob(blob),
		...(extractedText !== undefined ? { extractedText } : {}),
		source: tab.providerKey === "microsoft" ? "onedrive" : "gdrive",
		sourceItemId: row.id,
		connectionId: tab.connectionId,
		fetchedAt,
	};
}

function ConnectorAttachPicker(props: {
	i18n: ReturnType<typeof useI18n>;
	disabled?: boolean;
	onAttach: (file: ComposerFile) => void;
}) {
	const orgId = () => getSession().activeOrg?.id?.trim() || "";
	// ZDR gate: while disabled (temporary chat), the source returns null and
	// createResource never fetches at all — fail closed all the way down,
	// not just at the UI layer (connector-picker spec §4/§5.10).
	const [tabsResource] = createResource(
		() => (props.disabled ? null : orgId()),
		(id) => (id ? listConnectorPickerTabs(id) : Promise.resolve([])),
	);
	const tabs = createMemo(() => tabsResource() ?? []);

	const [activeConnectionId, setActiveConnectionId] = createSignal<string | null>(null);
	createEffect(
		() => tabs(),
		(list) => {
			const first = list[0];
			if (!first) return;
			if (!list.some((tab) => tab.connectionId === activeConnectionId())) {
				setActiveConnectionId(first.connectionId);
			}
		},
	);
	const activeTab = createMemo(() => tabs().find((tab) => tab.connectionId === activeConnectionId()) ?? null);

	const [kind, setKind] = createSignal<ConnectorBrowseKind>("files");
	const [queryInput, setQueryInput] = createSignal("");
	const [debouncedQuery, setDebouncedQuery] = createSignal("");
	let debounceHandle: ReturnType<typeof setTimeout> | undefined;
	onCleanup(() => {
		if (debounceHandle) clearTimeout(debounceHandle);
	});
	const onQueryInput = (value: string) => {
		setQueryInput(value);
		if (debounceHandle) clearTimeout(debounceHandle);
		debounceHandle = setTimeout(() => setDebouncedQuery(value), 300);
	};

	const [rowsResource] = createResource(
		() => {
			const tab = activeTab();
			return tab ? ([orgId(), tab.connectionId, tab.providerKey, kind(), debouncedQuery()] as const) : null;
		},
		async ([id, connectionId, providerKey, activeKind, query]) => {
			const { result } = await browseConnection(
				id,
				connectionId,
				browseOperationFor(providerKey, activeKind),
				query ? { query } : {},
			);
			return browseResultArray(providerKey, activeKind, result)
				.map((item) => connectorBrowseRow(props.i18n, providerKey, activeKind, item))
				.filter((row): row is ConnectorBrowseRow => row !== null);
		},
	);

	const [pendingRowId, setPendingRowId] = createSignal<string | null>(null);
	const [pickError, setPickError] = createSignal<string | null>(null);

	const pick = async (row: ConnectorBrowseRow) => {
		if (props.disabled || pendingRowId()) return;
		const tab = activeTab();
		const id = orgId();
		if (!tab || !id) return;
		setPendingRowId(row.id);
		setPickError(null);
		try {
			const file = await buildComposerFileFromPick({ i18n: props.i18n, orgId: id, tab, kind: kind(), row });
			props.onAttach(file);
		} catch (err) {
			if (
				err instanceof ApiError
				&& (err.code === "personal_connection_forbidden" || err.code === "personal_connection_no_delegated_user")
			) {
				// Owner ruling (5): the connection is no longer the caller's own
				// (e.g. ownership changed mid-session) — a clear, specific
				// message via the shared error-copy seam, not the raw English
				// gateway text and not the generic stale-pick copy below.
				setPickError(translateApiError(err, props.i18n.tr));
			} else if (err instanceof ApiError && err.status === 404) {
				setPickError(
					props.i18n.tr(
						"Elementet er ikke lenger tilgjengelig. Søk på nytt.",
						"That item is no longer available. Search again.",
					),
				);
			} else if (err instanceof Error) {
				setPickError(err.message);
			} else {
				setPickError(translateApiError(err, props.i18n.tr));
			}
		} finally {
			setPendingRowId(null);
		}
	};


	return (
		<div class="dashboard-composer-connector-picker">
			<Show when={props.disabled}>
				<div class="dashboard-composer-settings-empty">
					{props.i18n.tr(
						"Ikke tilgjengelig i midlertidig chat.",
						"Not available in temporary chat.",
					)}
				</div>
			</Show>
			<Show when={!props.disabled}>
				<Show
					when={!tabsResource.loading}
					fallback={
						<div class="dashboard-composer-settings-loading">
							<Loader2 class="size-3.5" />
							<span>{props.i18n.tr("Laster ...", "Loading ...")}</span>
						</div>
					}
				>
					<Show
						when={tabs().length > 0}
						fallback={
							<div class="dashboard-composer-settings-empty">
								{props.i18n.tr(
									"Ingen tilkoblede kilder ennå. Koble til OneDrive, Google Drive, Outlook eller Gmail under Koblinger.",
									"No connected sources yet. Connect OneDrive, Google Drive, Outlook or Gmail under Connectors.",
								)}
							</div>
						}
					>
						<div class="dashboard-composer-connector-tabs" role="tablist">
							<For each={tabs()}>
								{(tab) => (
									<button
										type="button"
										role="tab"
										aria-selected={activeTab()?.connectionId === tab.connectionId ? "true" : "false"}
										class={cn("dashboard-composer-connector-tab", {
											"dashboard-composer-connector-tab--active":
												activeTab()?.connectionId === tab.connectionId,
										})}
										onClick={() => setActiveConnectionId(tab.connectionId)}
									>
										{tab.label}
									</button>
								)}
							</For>
						</div>

						<div class="dashboard-composer-connector-kinds" role="tablist">
							<For each={CONNECTOR_KIND_OPTIONS}>
								{(option) => (
									<button
										type="button"
										role="tab"
										aria-selected={kind() === option.value ? "true" : "false"}
										class={cn("dashboard-composer-connector-kind", {
											"dashboard-composer-connector-kind--active": kind() === option.value,
										})}
										onClick={() => setKind(option.value)}
									>
										<Dynamic component={option.icon} class="size-3.5" strokeWidth={1.8} />
										<span>{props.i18n.tr(option.label.no, option.label.en)}</span>
									</button>
								)}
							</For>
						</div>

						<input
							type="search"
							class="dashboard-composer-connector-search"
							placeholder={props.i18n.tr("Søk ...", "Search ...")}
							value={queryInput()}
							onInput={(event) => onQueryInput(event.currentTarget.value)}
						/>

						<Show when={pickError()}>
							<div class="dashboard-composer-settings-error">
								<p>{pickError()}</p>
							</div>
						</Show>

						<div class="dashboard-composer-connector-results">
							<Show
								when={!rowsResource.loading}
								fallback={
									<div class="dashboard-composer-settings-loading">
										<Loader2 class="size-3.5" />
										<span>{props.i18n.tr("Søker ...", "Searching ...")}</span>
									</div>
								}
							>
								<Show
									when={!rowsResource.error}
									fallback={
										<div class="dashboard-composer-settings-error">
											{/* Owner ruling (5): a stale tab whose connection stopped being the
											   caller's own (personal_connection_forbidden /
											   personal_connection_no_delegated_user) gets that specific
											   message; every other browse failure keeps the generic one. */}
											<p>
												{translateApiError(rowsResource.error, props.i18n.tr, {
													no: "Kunne ikke søke.",
													en: "Could not search.",
												})}
											</p>
										</div>
									}
								>
									<Show
										when={(rowsResource() ?? []).length > 0}
										fallback={
											<div class="dashboard-composer-settings-empty">
												{props.i18n.tr("Ingen treff.", "No results.")}
											</div>
										}
									>
										<For each={rowsResource()}>
											{(row) => {
												const overCap = () =>
													typeof row.sizeBytes === "number" && row.sizeBytes > 1_000_000;
												return (
													<button
														type="button"
														disabled={overCap() || pendingRowId() !== null}
														title={
															overCap()
																? props.i18n.tr(
																		"Filen er over 1 MB og kan ikke legges ved.",
																		"The file is over 1MB and cannot be attached.",
																	)
																: undefined
														}
														class={cn(
															"verevon-menu-row dashboard-composer-settings-data-row dashboard-composer-connector-row",
															{ "dashboard-composer-connector-row--disabled": overCap() },
														)}
														onClick={() => void pick(row)}
													>
														<span class="dashboard-composer-settings-item-icon">
															<Show
																when={pendingRowId() === row.id}
																fallback={
																	<Dynamic
																		component={
																			CONNECTOR_KIND_OPTIONS.find((o) => o.value === kind())?.icon ?? FileText
																		}
																		class="size-3.5"
																	/>
																}
															>
																<Loader2 class="size-3.5 dashboard-composer-connector-spinner" />
															</Show>
														</span>
														<span class="dashboard-composer-settings-item-copy">
															<span class="verevon-menu-label">{row.title}</span>
															<Show when={row.subtitle}>
																<span class="verevon-menu-meta">{row.subtitle}</span>
															</Show>
														</span>
														<Show when={kind() === "files"}>
															<span class="verevon-menu-meta dashboard-composer-connector-size">
																{typeof row.sizeBytes === "number"
																	? formatFileSize(row.sizeBytes)
																	: "—"}
															</span>
														</Show>
													</button>
												);
											}}
										</For>
									</Show>
								</Show>
							</Show>
						</div>
					</Show>
				</Show>
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
	class?: string;
	active?: boolean;
	/** For returning focus here, e.g. when a dialog this button opened closes. */
	buttonRef?: (element: HTMLButtonElement) => void;
	children: JSX.Element;
	/** Id of an element that says more, e.g. why the button is unavailable. */
	describedBy?: string;
	disabled?: boolean;
	expanded?: boolean;
	hasPopup?: "dialog";
	label: string;
	onClick: () => void;
	/** Tooltip when it should say more than the label, e.g. why the button is disabled. */
	title?: string;
	/**
	 * Off, but still focusable and pressable (aria-disabled), so the reason in
	 * `describedBy` is announced and `onClick` can show it. `disabled` is for
	 * a control that is only briefly off and needs no explanation.
	 */
	unavailable?: boolean;
	variant: ComposerIconButtonVariant;
}) {
	const baseClass = () =>
		props.variant === "toolbar"
			? "dashboard-composer-toolbar-icon"
			: "dashboard-composer-icon-chip";

	return (
		<button
			ref={(element) => props.buttonRef?.(element)}
			type="button"
			aria-label={props.label}
			aria-pressed={props.active ? "true" : "false"}
			aria-expanded={props.expanded == null ? undefined : props.expanded ? "true" : "false"}
			aria-haspopup={props.hasPopup}
			aria-disabled={props.unavailable ? "true" : undefined}
			aria-describedby={props.describedBy}
			disabled={props.disabled}
			title={props.title ?? props.label}
			onClick={() => {
				if (!props.disabled) props.onClick();
			}}
			class={cn(
				baseClass(),
				props.class,
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
