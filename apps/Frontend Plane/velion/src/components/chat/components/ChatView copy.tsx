import {
	useRef,
	useEffect,
	useLayoutEffect,
	useState,
	useCallback,
	type CSSProperties,
	type ReactNode,
} from "react";
import { m, AnimatePresence, useReducedMotion } from "framer-motion";
import {
	ArrowDown,
	ArrowLeft,
	Copy,
	MoreHorizontal,
	RefreshCcw,
	Share2,
	Sparkles,
	ThumbsDown,
	ThumbsUp,
} from "lucide-react";

import { useI18n } from "@/components/chat/hooks/i18n";
import { ChatInput } from "./ChatInput";
import {
	ThreeJSOrb,
	type OrbAnimationState,
} from "@/components/chat/three/ThreeJSOrb";
import type { SendOptions } from "@/components/chat/providers/ChatProvider";
import { DASHBOARD_CHAT_STAGE_TRANSITION_NAME } from "@/components/chat/lib/transition";

interface MessageBubbleProps {
	message: {
		id: string;
		content: string;
		role: "user" | "assistant" | "system";
		timestamp: Date | string;
		isThinking?: boolean;
		metadata?: {
			citations?: string[];
		};
	};
	botName: string;
	orbState?: OrbAnimationState;
	onOrbStateChange?: (state: OrbAnimationState) => void;
	canRegenerate?: boolean;
	isRegenerating?: boolean;
	onRegenerate?: () => void | Promise<void>;
}

const formatTime = (timestamp: Date | string) => {
	const date = new Date(timestamp);
	if (Number.isNaN(date.getTime())) {
		return "";
	}

	return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
};

type MessageFeedback = "liked" | "disliked" | null;

const MESSAGE_FEEDBACK_STORAGE_KEY = "verevon-chat-message-feedback:v1";

async function copyTextToClipboard(text: string): Promise<boolean> {
	if (!text.trim()) {
		return false;
	}

	try {
		if (navigator.clipboard?.writeText) {
			await navigator.clipboard.writeText(text);
			return true;
		}
	} catch {
		// Fall back to a temporary textarea below.
	}

	if (typeof document === "undefined") {
		return false;
	}

	const textarea = document.createElement("textarea");
	textarea.value = text;
	textarea.setAttribute("readonly", "");
	textarea.style.position = "fixed";
	textarea.style.left = "-9999px";
	textarea.style.top = "0";
	document.body.appendChild(textarea);
	textarea.select();

	try {
		return document.execCommand("copy");
	} catch {
		return false;
	} finally {
		textarea.remove();
	}
}

function readStoredMessageFeedback(messageId: string): MessageFeedback {
	if (typeof window === "undefined") {
		return null;
	}

	try {
		const raw = window.localStorage.getItem(MESSAGE_FEEDBACK_STORAGE_KEY);
		if (!raw) {
			return null;
		}

		const parsed = JSON.parse(raw) as Record<
			string,
			Exclude<MessageFeedback, null>
		>;
		return parsed[messageId] ?? null;
	} catch {
		return null;
	}
}

function writeStoredMessageFeedback(
	messageId: string,
	feedback: MessageFeedback,
): void {
	if (typeof window === "undefined") {
		return;
	}

	try {
		const raw = window.localStorage.getItem(MESSAGE_FEEDBACK_STORAGE_KEY);
		const parsed = raw
			? (JSON.parse(raw) as Record<
					string,
					Exclude<MessageFeedback, null>
				>)
			: {};

		if (feedback) {
			parsed[messageId] = feedback;
		} else {
			delete parsed[messageId];
		}

		window.localStorage.setItem(
			MESSAGE_FEEDBACK_STORAGE_KEY,
			JSON.stringify(parsed),
		);
	} catch {
		// Non-critical UI preference; ignore storage failures.
	}
}

function messageShareUrl(messageId: string): string {
	if (typeof window === "undefined") {
		return "";
	}

	return `${window.location.href.split("#")[0]}#message-${messageId}`;
}

type MarkdownBlock =
	| { type: "paragraph"; text: string }
	| { type: "ordered-list"; items: string[] }
	| { type: "unordered-list"; items: string[] };

const renderInlineMarkdown = (text: string): ReactNode[] => {
	const parts = text.split(/(\*\*[^*]+\*\*)/g);

	return parts.map((part, index) => {
		if (part.startsWith("**") && part.endsWith("**") && part.length > 4) {
			return (
				<strong
					key={`${part}-${index}`}
					className="font-semibold text-[#25272d]"
				>
					{part.slice(2, -2)}
				</strong>
			);
		}

		return part;
	});
};

const parseMarkdownBlocks = (content: string): MarkdownBlock[] => {
	const blocks: MarkdownBlock[] = [];
	const paragraph: string[] = [];
	const orderedItems: string[] = [];
	const unorderedItems: string[] = [];

	const flushParagraph = () => {
		if (paragraph.length === 0) return;
		blocks.push({ type: "paragraph", text: paragraph.join(" ") });
		paragraph.length = 0;
	};

	const flushOrderedItems = () => {
		if (orderedItems.length === 0) return;
		blocks.push({ type: "ordered-list", items: [...orderedItems] });
		orderedItems.length = 0;
	};

	const flushUnorderedItems = () => {
		if (unorderedItems.length === 0) return;
		blocks.push({ type: "unordered-list", items: [...unorderedItems] });
		unorderedItems.length = 0;
	};

	for (const rawLine of content.split(/\r?\n/)) {
		const line = rawLine.trim();

		if (!line) {
			flushParagraph();
			flushOrderedItems();
			flushUnorderedItems();
			continue;
		}

		const orderedMatch = line.match(/^\d+\.\s+(.*)$/);
		if (orderedMatch) {
			flushParagraph();
			flushUnorderedItems();
			orderedItems.push(orderedMatch[1] ?? "");
			continue;
		}

		const unorderedMatch = line.match(/^[-*]\s+(.*)$/);
		if (unorderedMatch) {
			flushParagraph();
			flushOrderedItems();
			unorderedItems.push(unorderedMatch[1] ?? "");
			continue;
		}

		flushOrderedItems();
		flushUnorderedItems();
		paragraph.push(line);
	}

	flushParagraph();
	flushOrderedItems();
	flushUnorderedItems();

	return blocks;
};

const ChatMarkdown = ({ content }: { content: string }) => {
	const blocks = parseMarkdownBlocks(content);

	return (
		<div className="space-y-5">
			{blocks.map((block, blockIndex) => {
				if (block.type === "ordered-list") {
					return (
						<ol
							key={`ol-${blockIndex}`}
							className="list-decimal space-y-5 pl-6 marker:text-[#30323a]"
						>
							{block.items.map((item, itemIndex) => (
								<li
									key={`${item}-${itemIndex}`}
									className="pl-1"
								>
									{renderInlineMarkdown(item)}
								</li>
							))}
						</ol>
					);
				}

				if (block.type === "unordered-list") {
					return (
						<ul
							key={`ul-${blockIndex}`}
							className="list-disc space-y-3 pl-6 marker:text-[#30323a]"
						>
							{block.items.map((item, itemIndex) => (
								<li
									key={`${item}-${itemIndex}`}
									className="pl-1"
								>
									{renderInlineMarkdown(item)}
								</li>
							))}
						</ul>
					);
				}

				return (
					<p key={`p-${blockIndex}`}>
						{renderInlineMarkdown(block.text)}
					</p>
				);
			})}
		</div>
	);
};

const AssistantActions = ({
	content,
	messageId,
	canRegenerate = false,
	isRegenerating = false,
	onRegenerate,
}: {
	content: string;
	messageId: string;
	canRegenerate?: boolean;
	isRegenerating?: boolean;
	onRegenerate?: () => void | Promise<void>;
}) => {
	const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">(
		"idle",
	);
	const [feedback, setFeedback] = useState<MessageFeedback>(null);
	const [menuOpen, setMenuOpen] = useState(false);
	const copyResetRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	useEffect(() => {
		setFeedback(readStoredMessageFeedback(messageId));
	}, [messageId]);

	useEffect(() => {
		return () => {
			if (copyResetRef.current) {
				clearTimeout(copyResetRef.current);
			}
		};
	}, []);

	const resetCopyStateSoon = useCallback((nextState: "copied" | "failed") => {
		setCopyState(nextState);
		if (copyResetRef.current) {
			clearTimeout(copyResetRef.current);
		}
		copyResetRef.current = setTimeout(() => setCopyState("idle"), 1400);
	}, []);

	const handleCopy = useCallback(async () => {
		const copied = await copyTextToClipboard(content);
		resetCopyStateSoon(copied ? "copied" : "failed");
		setMenuOpen(false);
	}, [content, resetCopyStateSoon]);

	const handleFeedback = useCallback(
		(nextFeedback: Exclude<MessageFeedback, null>) => {
			setFeedback((currentFeedback) => {
				const resolvedFeedback =
					currentFeedback === nextFeedback ? null : nextFeedback;
				writeStoredMessageFeedback(messageId, resolvedFeedback);
				return resolvedFeedback;
			});
		},
		[messageId],
	);

	const handleShare = useCallback(async () => {
		const url = messageShareUrl(messageId);

		try {
			if (navigator.share && url) {
				await navigator.share({
					title: "Aquatiq answer",
					text: content.slice(0, 240),
					url,
				});
				setMenuOpen(false);
				return;
			}
		} catch (error) {
			if (error instanceof DOMException && error.name === "AbortError") {
				return;
			}
		}

		const copied = await copyTextToClipboard(url || content);
		resetCopyStateSoon(copied ? "copied" : "failed");
		setMenuOpen(false);
	}, [content, messageId, resetCopyStateSoon]);

	const handleRegenerate = useCallback(() => {
		if (!canRegenerate || isRegenerating || !onRegenerate) {
			return;
		}

		void onRegenerate();
	}, [canRegenerate, isRegenerating, onRegenerate]);

	const actionClass = (active = false) =>
		`rounded-lg p-1.5 transition disabled:cursor-not-allowed disabled:opacity-40 ${
			active
				? "bg-[#fff1ec] text-[var(--linear-accent)]"
				: "hover:bg-[#F0F0F1] hover:text-[#26282f]"
		}`;

	return (
		<div className="mt-5 flex items-center gap-1 text-[#7d828a]">
			<button
				type="button"
				onClick={() => void handleCopy()}
				className={actionClass(copyState === "copied")}
				aria-label={copyState === "copied" ? "Copied answer" : "Copy answer"}
				title={
					copyState === "failed"
						? "Could not copy"
						: copyState === "copied"
							? "Copied"
							: "Copy"
				}
			>
				<Copy className="h-4 w-4" />
			</button>
			<button
				type="button"
				onClick={() => handleFeedback("liked")}
				className={actionClass(feedback === "liked")}
				aria-pressed={feedback === "liked"}
				aria-label="Good answer"
				title="Good answer"
			>
				<ThumbsUp className="h-4 w-4" />
			</button>
			<button
				type="button"
				onClick={() => handleFeedback("disliked")}
				className={actionClass(feedback === "disliked")}
				aria-pressed={feedback === "disliked"}
				aria-label="Bad answer"
				title="Bad answer"
			>
				<ThumbsDown className="h-4 w-4" />
			</button>
			<button
				type="button"
				onClick={handleRegenerate}
				disabled={!canRegenerate || isRegenerating}
				className={actionClass(isRegenerating)}
				aria-label="Regenerate answer"
				title={
					canRegenerate
						? "Regenerate latest answer"
						: "Regenerate is available on the latest answer"
				}
			>
				<RefreshCcw
					className={`h-4 w-4 ${isRegenerating ? "animate-spin" : ""}`}
				/>
			</button>
			<div className="relative">
				<button
					type="button"
					onClick={() => setMenuOpen((open) => !open)}
					className={actionClass(menuOpen)}
					aria-label="More actions"
					aria-expanded={menuOpen}
					title="More actions"
				>
					<MoreHorizontal className="h-4 w-4" />
				</button>
				{menuOpen ? (
					<div className="absolute bottom-full left-0 z-40 mb-2 w-44 rounded-2xl border border-[var(--linear-border)] bg-white p-1.5 text-[12px] font-medium text-[#4f5660] shadow-[0_16px_38px_rgba(20,21,24,0.10)]">
						<button
							type="button"
							onClick={() => void handleCopy()}
							className="w-full rounded-xl px-3 py-2 text-left transition hover:bg-[#F5F5F6] hover:text-[#26282f]"
						>
							Copy text
						</button>
						<button
							type="button"
							onClick={() => void handleShare()}
							className="w-full rounded-xl px-3 py-2 text-left transition hover:bg-[#F5F5F6] hover:text-[#26282f]"
						>
							Copy share link
						</button>
					</div>
				) : null}
			</div>
			{copyState !== "idle" ? (
				<span className="ml-1 text-[11px] text-[#8c929b]">
					{copyState === "copied" ? "Copied" : "Copy failed"}
				</span>
			) : null}
		</div>
	);
};

const MessageBubble: React.FC<MessageBubbleProps> = ({
	message,
	botName,
	orbState = "waiting",
	onOrbStateChange,
	canRegenerate = false,
	isRegenerating = false,
	onRegenerate,
}) => {
	const isUser = message.role === "user";
	const timestamp = formatTime(message.timestamp);

	if (isUser) {
		return (
			<m.div
				initial={{ opacity: 0, y: 10 }}
				animate={{ opacity: 1, y: 0 }}
				transition={{ duration: 0.2, ease: "easeOut" }}
				className="mb-8 flex justify-end"
			>
				<div className="max-w-[min(650px,82%)]">
					<div className="rounded-[22px] border border-[#e7e7ea] bg-white px-5 py-3.5 text-[15px] leading-[1.65] text-[#26282f] shadow-[0_10px_28px_rgba(20,21,24,0.055)]">
						<p className="whitespace-pre-wrap">{message.content}</p>
					</div>
					{timestamp ? (
						<div className="mt-1.5 pr-2 text-right text-[11px] text-[#9aa0a8]">
							{timestamp}
						</div>
					) : null}
				</div>
			</m.div>
		);
	}

	return (
		<m.article
			id={`message-${message.id}`}
			initial={{ opacity: 0, y: 12 }}
			animate={{ opacity: 1, y: 0 }}
			transition={{ duration: 0.22, ease: "easeOut" }}
			className="mb-8 last:mb-0"
		>
			<div className="mb-4 flex items-center gap-3">
				<div className="h-8 w-8 shrink-0 overflow-hidden rounded-full bg-white shadow-[0_0_0_1px_var(--linear-border)]">
					{/* forceFallback: per-message avatars use the CSS orb so the chat
              history doesn't spawn one WebGL context per message and exhaust
              the browser's ~16-context cap. Live WebGL stays on the
              singleton composer + typing-indicator orbs. */}
					<ThreeJSOrb state={orbState} size={32} forceFallback />
				</div>
				<div className="flex min-w-0 items-center gap-2">
					<span className="text-[15px] font-semibold tracking-[-0.01em] text-[#26282f]">
						{botName}
					</span>
					<span className="rounded-full border border-[var(--linear-border)] bg-white px-2 py-0.5 text-[11px] font-medium text-[#7d828a]">
						Agent
					</span>
				</div>
				{timestamp ? (
					<span className="ml-auto text-[11px] text-[#9aa0a8]">
						{timestamp}
					</span>
				) : null}
			</div>

			<div className="max-w-none text-[16.5px] leading-[1.82] tracking-[-0.01em] text-[#2e3138]">
				<ChatMarkdown content={message.content} />
			</div>

			{message.metadata?.citations?.length ? (
				<div className="mt-4 flex flex-wrap gap-2">
					{message.metadata.citations.slice(0, 4).map((citation) => (
						<span
							key={citation}
							className="rounded-full border border-[var(--linear-border)] bg-white px-2.5 py-1 text-[12px] text-[#6d737c]"
						>
							{citation}
						</span>
					))}
				</div>
			) : null}

			<AssistantActions
				content={message.content}
				messageId={message.id}
				canRegenerate={canRegenerate}
				isRegenerating={isRegenerating}
				onRegenerate={onRegenerate}
			/>
		</m.article>
	);
};

const TypingIndicator = ({
	botName,
	orbState = "thinking",
	onOrbStateChange,
}: {
	botName: string;
	orbState?: OrbAnimationState;
	onOrbStateChange?: (state: OrbAnimationState) => void;
}) => (
	<m.div
		initial={{ opacity: 0, y: 10 }}
		animate={{ opacity: 1, y: 0 }}
		exit={{ opacity: 0, y: -10 }}
		transition={{ duration: 0.2 }}
		className="mb-8 rounded-[22px] border border-[var(--linear-border)] bg-white px-4 py-3 shadow-[0_16px_42px_rgba(20,21,24,0.07)]"
	>
		<div className="flex items-center gap-3">
			<div className="h-7 w-7 overflow-hidden rounded-full bg-white shadow-[0_0_0_1px_var(--linear-border)]">
				<ThreeJSOrb
					state={orbState}
					size={28}
					onStateChange={onOrbStateChange}
				/>
			</div>
			<div>
				<div className="text-[13px] font-semibold text-[#26282f]">
					{botName} is working
				</div>
				<div className="mt-1 flex items-center gap-1 text-[12px] text-[#8c929b]">
					<span className="h-1.5 w-1.5 rounded-full bg-[var(--linear-accent)]" />
					<span>Thinking</span>
				</div>
			</div>
		</div>
	</m.div>
);

interface ChatMessage {
	id: string;
	content: string;
	role: "user" | "assistant" | "system";
	timestamp: Date | string;
	isThinking?: boolean;
	metadata?: {
		citations?: string[];
	};
}

interface ChatSessionLike {
	id: string;
	title?: string;
	messages: ChatMessage[];
}

interface ChatViewProps {
	currentChat: ChatSessionLike | null;
	message: string;
	setMessage: (message: string) => void;
	onMessageSubmit: (
		e: React.FormEvent,
		options?: SendOptions,
	) => void | Promise<void>;
	isTyping: boolean;
	onBack?: () => void;
	botAvatarSrc?: string;
	botName?: string;
	userAvatarSrc?: string;
	orbState?: OrbAnimationState;
	onOrbStateChange?: (state: OrbAnimationState) => void;
	composerViewTransitionName?: string;
	onRegenerate?: () => void | Promise<void>;
}

const samplePrompts = [
	"Planlegg de første ukene mine",
	"Søk i selskapets kunnskap",
	"Oppsummer siste kundedialog",
	"Lag et konkret neste steg",
];

export function ChatView({
	currentChat,
	message,
	setMessage,
	onMessageSubmit,
	isTyping,
	onBack,
	botName,
	orbState = "waiting",
	onOrbStateChange,
	composerViewTransitionName,
	onRegenerate,
}: ChatViewProps) {
	const { t } = useI18n();
	const effectiveBotName = botName || "Aquatiq";
	const viewportRef = useRef<HTMLDivElement | null>(null);
	const composerFrameRef = useRef<HTMLDivElement | null>(null);
	const previousMessageCountRef = useRef(0);
	const [isAtBottom, setIsAtBottom] = useState(true);
	const [canShowScrollCta, setCanShowScrollCta] = useState(false);
	const [composerInsetHeight, setComposerInsetHeight] = useState(220);
	const [conversationShareState, setConversationShareState] = useState<
		"idle" | "copied" | "shared" | "failed"
	>("idle");
	const [conversationMenuOpen, setConversationMenuOpen] = useState(false);
	const conversationShareResetRef = useRef<ReturnType<typeof setTimeout> | null>(
		null,
	);
	const prefersReducedMotion = useReducedMotion();
	const messages = currentChat?.messages ?? [];
	const hasMessages = messages.length > 0;
	const hasStreamingAssistant = messages.some(
		(msg) => msg.role === "assistant" && msg.isThinking,
	);
	const latestAssistantMessageId = [...messages]
		.reverse()
		.find((msg) => msg.role === "assistant")?.id;

	const scrollToBottom = useCallback(
		(smooth = true) => {
			const viewport = viewportRef.current;
			if (viewport) {
				viewport.scrollTo({
					top: viewport.scrollHeight,
					behavior:
						smooth && !prefersReducedMotion ? "smooth" : "auto",
				});
			}
		},
		[prefersReducedMotion],
	);

	useLayoutEffect(() => {
		if (isAtBottom) {
			scrollToBottom(!isTyping);
		}
	}, [messages, isTyping, scrollToBottom, isAtBottom]);

	useLayoutEffect(() => {
		const frame = composerFrameRef.current;
		if (!frame || !hasMessages) return;

		const updateInset = () => {
			const nextHeight = Math.ceil(
				frame.getBoundingClientRect().height + 34,
			);
			setComposerInsetHeight((currentHeight) =>
				Math.abs(currentHeight - nextHeight) > 2
					? nextHeight
					: currentHeight,
			);
		};

		updateInset();

		const resizeObserver = new ResizeObserver(updateInset);
		resizeObserver.observe(frame);

		return () => resizeObserver.disconnect();
	}, [hasMessages]);

	useLayoutEffect(() => {
		if (isAtBottom) {
			scrollToBottom(false);
		}
	}, [composerInsetHeight, isAtBottom, scrollToBottom]);

	useLayoutEffect(() => {
		const nextMessageCount = messages.length;
		const previousMessageCount = previousMessageCountRef.current;
		previousMessageCountRef.current = nextMessageCount;

		if (nextMessageCount <= previousMessageCount) {
			return;
		}

		const shouldStickToBottom = isAtBottom || previousMessageCount === 0;

		if (!shouldStickToBottom) {
			setCanShowScrollCta(true);
			return;
		}

		window.requestAnimationFrame(() => {
			setCanShowScrollCta(false);
			scrollToBottom(previousMessageCount > 0 && !isTyping);
		});
	}, [messages.length, isTyping, scrollToBottom, isAtBottom]);

	useEffect(() => {
		const el = viewportRef.current;
		if (!el) return;
		const onScroll = () => {
			const delta = el.scrollHeight - el.scrollTop - el.clientHeight;
			const nextIsAtBottom = delta < 24;
			setIsAtBottom(nextIsAtBottom);

			if (nextIsAtBottom) {
				setCanShowScrollCta(false);
			}
		};
		el.addEventListener("scroll", onScroll, { passive: true });
		onScroll();
		return () => el.removeEventListener("scroll", onScroll);
	}, []);

	useEffect(() => {
		return () => {
			if (conversationShareResetRef.current) {
				clearTimeout(conversationShareResetRef.current);
			}
		};
	}, []);

	const resetConversationShareStateSoon = useCallback(
		(nextState: "copied" | "shared" | "failed") => {
			setConversationShareState(nextState);
			if (conversationShareResetRef.current) {
				clearTimeout(conversationShareResetRef.current);
			}
			conversationShareResetRef.current = setTimeout(
				() => setConversationShareState("idle"),
				1400,
			);
		},
		[],
	);

	const handleShareConversation = useCallback(async () => {
		const url =
			typeof window === "undefined" ? "" : window.location.href.split("#")[0];
		const title = currentChat?.title || "Aquatiq chat";

		try {
			if (navigator.share && url) {
				await navigator.share({ title, url });
				resetConversationShareStateSoon("shared");
				setConversationMenuOpen(false);
				return;
			}
		} catch (error) {
			if (error instanceof DOMException && error.name === "AbortError") {
				return;
			}
		}

		const copied = await copyTextToClipboard(url);
		resetConversationShareStateSoon(copied ? "copied" : "failed");
		setConversationMenuOpen(false);
	}, [currentChat?.title, resetConversationShareStateSoon]);

	const stageTransitionStyle = {
		viewTransitionName: DASHBOARD_CHAT_STAGE_TRANSITION_NAME,
	} as CSSProperties;
	const composerTransitionStyle = composerViewTransitionName
		? ({ viewTransitionName: composerViewTransitionName } as CSSProperties)
		: undefined;

	const composer = (
		<div style={composerTransitionStyle}>
			<ChatInput
				message={message}
				setMessage={setMessage}
				onSubmit={onMessageSubmit}
				disabled={false}
				isLoading={isTyping}
				isTyping={isTyping}
				context="chatpage"
			/>
		</div>
	);

	return (
		<div
			className="relative flex h-full min-h-0 flex-col overflow-hidden bg-[var(--linear-main-bg)]"
			style={stageTransitionStyle}
		>
			<header className="absolute inset-x-0 top-0 z-30 hidden h-16 items-center justify-between bg-[linear-gradient(180deg,var(--linear-main-bg)_0%,rgba(252,252,253,0.92)_70%,rgba(252,252,253,0))] px-6 py-3 lg:flex">
				<div className="flex min-w-0 items-center gap-3">
					{onBack ? (
						<button
							onClick={onBack}
							className="rounded-xl p-2 text-[#7d828a] transition hover:bg-[#F0F0F1] hover:text-[#26282f]"
							aria-label="Back"
							type="button"
						>
							<ArrowLeft className="h-4 w-4" />
						</button>
					) : null}
					<div className="flex min-w-0 items-center gap-2">
						<span className="truncate text-[16px] font-semibold tracking-[-0.02em] text-[#26282f]">
							{effectiveBotName}
						</span>
						<span className="rounded-full border border-[var(--linear-border)] bg-white px-2 py-0.5 text-[11px] font-medium text-[#7d828a]">
							Chat
						</span>
					</div>
				</div>

				<div className="flex items-center gap-2 text-[#6f757e]">
					<button
						type="button"
						onClick={() => void handleShareConversation()}
						className="inline-flex items-center gap-1.5 rounded-full border border-[var(--linear-border)] bg-white px-3 py-1.5 text-[12px] font-medium transition hover:border-[#d5d7db] hover:text-[#26282f]"
						title={
							conversationShareState === "failed"
								? "Could not copy link"
								: conversationShareState === "copied"
									? "Copied"
									: "Share"
						}
					>
						<Share2 className="h-3.5 w-3.5" />
						{conversationShareState === "copied"
							? "Copied"
							: conversationShareState === "shared"
								? "Shared"
								: "Share"}
					</button>
					<div className="relative">
						<button
							type="button"
							onClick={() => setConversationMenuOpen((open) => !open)}
							className="rounded-full border border-[var(--linear-border)] bg-white p-1.5 transition hover:border-[#d5d7db] hover:text-[#26282f]"
							aria-label="More actions"
							aria-expanded={conversationMenuOpen}
						>
							<MoreHorizontal className="h-4 w-4" />
						</button>
						{conversationMenuOpen ? (
							<div className="absolute right-0 top-full z-40 mt-2 w-48 rounded-2xl border border-[var(--linear-border)] bg-white p-1.5 text-[12px] font-medium text-[#4f5660] shadow-[0_16px_38px_rgba(20,21,24,0.10)]">
								<button
									type="button"
									onClick={() => void handleShareConversation()}
									className="w-full rounded-xl px-3 py-2 text-left transition hover:bg-[#F5F5F6] hover:text-[#26282f]"
								>
									Copy conversation link
								</button>
							</div>
						) : null}
					</div>
				</div>
			</header>

			<div
				ref={viewportRef}
				onWheelCapture={() => {
					if (hasMessages && !isTyping) {
						setCanShowScrollCta(true);
					}
				}}
				onTouchMoveCapture={() => {
					if (hasMessages && !isTyping) {
						setCanShowScrollCta(true);
					}
				}}
				className="relative z-0 min-h-0 flex-1 overflow-y-auto overscroll-contain"
			>
				{!hasMessages ? (
					<div className="mx-auto flex min-h-full w-full max-w-[860px] flex-col justify-center px-5 py-12 sm:px-8">
						<m.div
							initial={{ opacity: 0, y: 16 }}
							animate={{ opacity: 1, y: 0 }}
							transition={{ duration: 0.35, ease: "easeOut" }}
							className="pb-12"
						>
							<div className="mb-8 flex justify-center">
								<div className="rounded-full border border-[var(--linear-border)] bg-white px-3 py-1.5 text-[12px] font-medium text-[#7d828a] shadow-[0_10px_30px_rgba(20,21,24,0.05)]">
									<span className="inline-flex items-center gap-1.5">
										<Sparkles className="h-3.5 w-3.5 text-[var(--linear-accent)]" />
										Verevon workspace agent
									</span>
								</div>
							</div>

							<h2
								className="mb-8 text-center text-[42px] font-normal leading-none tracking-[-0.045em] text-[#2b2d33] sm:text-[54px]"
								style={{
									fontFamily:
										"var(--font-cormorant-garamond), Georgia, serif",
								}}
							>
								What can I do for you?
							</h2>

							<div className="mx-auto w-full max-w-[760px]">
								{composer}
							</div>

							<div className="mx-auto mt-8 grid w-full max-w-[760px] grid-cols-1 gap-3 sm:grid-cols-2">
								{samplePrompts.map((prompt, index) => (
									<button
										key={prompt}
										type="button"
										onClick={() => setMessage(prompt)}
										className="group rounded-2xl border border-[var(--linear-border)] bg-white px-4 py-3 text-left text-[13px] font-medium leading-snug text-[#4f5660] shadow-[0_14px_34px_rgba(20,21,24,0.04)] transition hover:-translate-y-0.5 hover:border-[#d8dade] hover:text-[#26282f] hover:shadow-[0_18px_38px_rgba(20,21,24,0.08)]"
									>
										<span className="mb-3 flex h-6 w-6 items-center justify-center rounded-full bg-[#F3F3F4] text-[11px] text-[#7d828a] transition group-hover:bg-[#fff1ec] group-hover:text-[#ff6b35]">
											{index + 1}
										</span>
										{prompt}
									</button>
								))}
							</div>
						</m.div>
					</div>
				) : (
					<m.div
						className="mx-auto w-full max-w-[900px] px-5 pt-24 sm:px-8 lg:pt-24"
						style={{ paddingBottom: composerInsetHeight }}
					>
						<div>
							{messages.map((msg) => (
								<MessageBubble
									key={msg.id}
									message={msg}
									botName={effectiveBotName}
									orbState={orbState}
									onOrbStateChange={onOrbStateChange}
									canRegenerate={
										msg.role === "assistant" &&
										msg.id === latestAssistantMessageId
									}
									isRegenerating={isTyping}
									onRegenerate={onRegenerate}
								/>
							))}
						</div>

						<AnimatePresence>
							{isTyping && !hasStreamingAssistant ? (
								<TypingIndicator
									botName={effectiveBotName}
									orbState={orbState}
									onOrbStateChange={onOrbStateChange}
								/>
							) : null}
						</AnimatePresence>
					</m.div>
				)}
			</div>

			<AnimatePresence>
				{!isTyping && canShowScrollCta && !isAtBottom && hasMessages ? (
					<m.button
						initial={{ opacity: 0, y: 6 }}
						animate={{ opacity: 1, y: 0 }}
						exit={{ opacity: 0, y: 6 }}
						onClick={() => scrollToBottom(true)}
						className="absolute bottom-36 right-6 z-30 flex items-center gap-2 rounded-full border border-[var(--linear-border)] bg-white px-3 py-2 text-[12px] font-medium text-[#6f757e] shadow-[0_16px_38px_rgba(20,21,24,0.08)] transition hover:border-[#d5d7db] hover:text-[#26282f]"
						type="button"
					>
						<ArrowDown className="h-3.5 w-3.5" />
						<span>{t("chat.action.scrollBottom")}</span>
					</m.button>
				) : null}
			</AnimatePresence>

			{hasMessages ? (
				<div className="pointer-events-none absolute inset-x-0 bottom-0 z-20 px-5 pb-5 pt-24 sm:px-8">
					<div
						aria-hidden
						className="absolute inset-x-0 top-0 h-28 bg-[linear-gradient(180deg,rgba(252,252,253,0),var(--linear-main-bg)_72%)]"
					/>
					<div
						aria-hidden
						className="absolute inset-x-0 bottom-0 top-16 bg-[var(--linear-main-bg)]"
					/>
					<div
						ref={composerFrameRef}
						className="pointer-events-auto relative mx-auto w-full max-w-[760px]"
					>
						{composer}
					</div>
				</div>
			) : null}
		</div>
	);
}
