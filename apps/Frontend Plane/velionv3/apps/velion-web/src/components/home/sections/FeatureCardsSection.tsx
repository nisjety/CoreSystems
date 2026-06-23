"use client";

import { useEffect, useRef } from "react";
import { VelionComposerPreview } from "@/components/ui/VelionComposerPreview";

type WorkflowCard = {
	href: string;
	image: string;
	kicker: string;
	link: string;
	title: string;
	text: string;
};

const promptText =
	"Lag en løsning for dagens uløste kundesamtaler: svarutkast, kilder, prioritet og godkjenning.";

const workflowCards: WorkflowCard[] = [
	{
		href: "#technology",
		image: "/velion-product-shots/chat-draft-answer-top.png",
		kicker: "Reply",
		link: "Draft replies",
		title: "Draft replies",
		text: "Compose email, chat, and social answers with visible source trails before anything is sent.",
	},
	{
		href: "#partnership",
		image: "/velion-product-shots/dashboard-composer-prompt-state.png",
		kicker: "Build",
		link: "Create chatbot",
		title: "Build the bot",
		text: "Connect help center, products, and policies, then publish a customer-facing assistant.",
	},
	{
		href: "#company",
		image: "/velion-product-shots/chat-agent-steps.png",
		kicker: "Govern",
		link: "Route risk",
		title: "Route risk",
		text: "Find slow cases, classify urgency, and ask the right human before escalation.",
	},
	{
		href: "#contact",
		image: "/velion-product-shots/inbox-empty-workspace.png",
		kicker: "Trace",
		link: "Audit action",
		title: "Audit action",
		text: "Record approvals, policy checks, connector state, and rollback paths for every workflow.",
	},
];

function ArrowGlyph({ className = "" }: { className?: string }) {
	return (
		<svg
			aria-hidden="true"
			className={["h-[10px] fill-current lg:h-[11px]", className]
				.filter(Boolean)
				.join(" ")}
			viewBox="0 0 22.35 7.16"
			xmlns="http://www.w3.org/2000/svg"
		>
			<path
				d="m18.77 0 3.58 3.58c-.76 0-1.52-.29-2.1-.87l-2.1-2.1.62-.61zm-.61 6.54 2.1-2.1c.58-.58 1.34-.87 2.1-.87l-3.58 3.58-.62-.61zm.28-2.53v-.87H0V4h18.44z"
				strokeWidth="0.5px"
			/>
		</svg>
	);
}

function CardArrowLabel({ children }: { children: string }) {
	return (
		<span className="relative inline-flex w-fit items-center overflow-hidden px-px py-1 font-protokoll text-[clamp(0.94rem,0.95vw,1.08rem)] font-light leading-none text-velion-j-text/62 opacity-80 transition-all duration-500 group-hover:opacity-100">
			<span className="flex translate-x-[-43px] items-center gap-3 transition-transform duration-500 group-hover:translate-x-0 sm:translate-x-[-37px] sm:gap-1">
				<ArrowGlyph />
				<span className="whitespace-nowrap">{children}</span>
				<ArrowGlyph className="absolute left-full translate-x-3 sm:translate-x-1" />
			</span>
		</span>
	);
}

function SourceFrame() {
	return (
		<div
			className="relative mx-auto mb-[clamp(82px,13vh,142px)] aspect-[3/4] w-[min(29vh,270px)] max-w-[270px] overflow-visible border border-velion-j-text/10 bg-[linear-gradient(180deg,#f8f8f7_0%,#ecebea_100%)] shadow-[0_10px_28px_rgba(23,23,23,0.035)] will-change-transform"
			data-feature-source
		>
			<div className="absolute inset-0 overflow-hidden">
				<img
					alt=""
					className="h-full w-full select-none object-cover opacity-70 saturate-[0.72]"
					draggable={false}
					src="/velion-vibe/human-haze.png"
				/>
			</div>

			<div className="absolute inset-x-0 top-[-20px] flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-velion-j-text/38">
				<span className="flex items-center gap-1">
					<span className="size-[5px] rounded-full bg-velion-coral/70" />
					signal
				</span>
				<span>customer context</span>
			</div>

			{[
				"left-[-3.5px] top-[-3.5px]",
				"right-[-3.5px] top-[-3.5px]",
				"bottom-[-3.5px] left-[-3.5px]",
				"bottom-[-3.5px] right-[-3.5px]",
			].map((position) => (
				<span
					aria-hidden="true"
					className={[
						"absolute size-[6px] border border-velion-j-text/14 bg-background",
						position,
					].join(" ")}
					key={position}
				/>
			))}
		</div>
	);
}

function PromptComposer() {
	return (
		<div
			className="absolute bottom-[clamp(58px,10vh,108px)] left-1/2 z-[5] w-[min(91vw,760px)] origin-bottom -translate-x-1/2 opacity-100 will-change-transform"
			data-feature-prompt
		>
			<VelionComposerPreview animateCharacters prompt={promptText} />
		</div>
	);
}

function OutputCard({ card }: { card: WorkflowCard }) {
	return (
		<a
			aria-label={card.link}
			className="group flex min-w-0 flex-col gap-3 will-change-[transform,opacity,filter]"
			data-feature-card
			href={card.href}
		>
			<div className="flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-velion-j-text/42">
				<span className="flex items-center gap-1">
					<span className="size-[5px] rounded-full bg-velion-coral/70" />
					{card.kicker}
				</span>
				<span>Velion output</span>
			</div>

			<div className="relative aspect-[3/4] overflow-hidden border border-velion-j-text/8 bg-white/55 shadow-[0_10px_28px_rgba(23,23,23,0.035)]">
				<img
					alt=""
					className="h-full w-full select-none object-cover opacity-[0.76] saturate-[0.72] transition-transform duration-700 group-hover:scale-[1.035]"
					draggable={false}
					src={card.image}
				/>
				<div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(248,248,247,0.08),rgba(248,248,247,0.72))]" />
				<div className="absolute bottom-4 left-4 right-4">
					<h3 className="m-0 font-arbeit text-[clamp(1.45rem,2vw,2.2rem)] font-light leading-[1.04] tracking-[-0.055em] text-velion-j-text">
						{card.title}
					</h3>
				</div>
			</div>

			<p className="m-0 min-h-[4.2em] font-protokoll text-[clamp(0.92rem,0.92vw,1.02rem)] font-light leading-[1.38] text-velion-text-muted/90">
				{card.text}
			</p>

			<CardArrowLabel>{card.link}</CardArrowLabel>
		</a>
	);
}

export function FeatureCardsSection() {
	const sectionRef = useRef<HTMLElement | null>(null);

	useEffect(() => {
		let ctx: { revert: () => void } | undefined;
		let disposed = false;

		async function setupAnimation() {
			const [{ default: gsap }, { ScrollTrigger }] = await Promise.all([
				import("gsap"),
				import("gsap/ScrollTrigger"),
			]);

			if (disposed || !sectionRef.current) {
				return;
			}

			gsap.registerPlugin(ScrollTrigger);

			ctx = gsap.context(() => {
				const section = sectionRef.current;

				if (!section) {
					return;
				}

				const reduceMotion = window.matchMedia(
					"(prefers-reduced-motion: reduce)",
				).matches;
				const isCompact = window.matchMedia("(max-width: 899px)").matches;
				const source = section.querySelector<HTMLElement>(
					"[data-feature-source]",
				);
				const prompt = section.querySelector<HTMLElement>(
					"[data-feature-prompt]",
				);
				const outputStage = section.querySelector<HTMLElement>(
					"[data-feature-output-stage]",
				);
				const processing = section.querySelector<HTMLElement>(
					"[data-feature-processing]",
				);
				const sendArrow = section.querySelector<HTMLElement>(
					"[data-feature-send-arrow]",
				);
				const sendButton = section.querySelector<HTMLElement>(
					"[data-feature-send-button]",
				);
				const sendRing = section.querySelector<HTMLElement>(
					"[data-feature-send-ring]",
				);
				const cards = Array.from(
					section.querySelectorAll<HTMLElement>("[data-feature-card]"),
				);
				const chars = Array.from(
					section.querySelectorAll<HTMLElement>("[data-feature-char]"),
				);

				if (
					!source ||
					!prompt ||
					!outputStage ||
					!processing ||
					!sendArrow ||
					!sendButton ||
					!sendRing ||
					cards.length === 0
				) {
					return;
				}

				if (reduceMotion || isCompact) {
					gsap.set(
						[
							source,
							prompt,
							cards,
							chars,
							sendArrow,
							sendButton,
							sendRing,
							processing,
						],
						{
							autoAlpha: 1,
							clearProps: "filter,transform",
						},
					);
					gsap.set(outputStage, { autoAlpha: 1, pointerEvents: "auto" });
					return;
				}

				gsap.set(source, { autoAlpha: 1, scale: 1, y: 0, force3D: true });
				gsap.set(prompt, {
					autoAlpha: 0,
					scale: 0.94,
					y: 42,
					force3D: true,
				});
				gsap.set(chars, { autoAlpha: 0 });
				gsap.set(outputStage, { autoAlpha: 0, pointerEvents: "none" });
				gsap.set(sendButton, {
					backgroundColor: "#111111",
					scale: 1,
					transformOrigin: "50% 50%",
					force3D: true,
				});
				gsap.set(sendArrow, { y: 0, force3D: true });
				gsap.set(sendRing, {
					autoAlpha: 0,
					scale: 0.86,
					transformOrigin: "50% 50%",
				});
				gsap.set(processing, {
					autoAlpha: 0,
					scaleX: 0,
					transformOrigin: "0% 50%",
				});
				gsap.set(cards, {
					autoAlpha: 0,
					filter: "blur(16px)",
					scale: 1.12,
					xPercent: (index) => [142, 48, -48, -142][index] ?? 0,
					y: -20,
					force3D: true,
				});

				gsap
					.timeline({
						scrollTrigger: {
							trigger: section,
							start: "top top",
							end: "+=220%",
							scrub: 0.6,
							pin: true,
							id: "velion-feature-cards-flow",
							invalidateOnRefresh: true,
							refreshPriority: 2,
						},
					})
					.to(
						source,
						{ y: -18, scale: 0.965, ease: "none", duration: 0.16 },
						0.04,
					)
					.to(
						prompt,
						{ autoAlpha: 1, y: 0, scale: 1, ease: "none", duration: 0.2 },
						0.12,
					)
					.to(
						chars,
						{ autoAlpha: 1, duration: 0.22, ease: "none", stagger: 0.003 },
						0.2,
					)
					.to(
						sendButton,
						{
							backgroundColor: "#ee7a50",
							scale: 0.88,
							ease: "none",
							duration: 0.06,
						},
						0.48,
					)
					.to(sendArrow, { y: -3, ease: "none", duration: 0.06 }, 0.48)
					.to(
						sendRing,
						{
							autoAlpha: 1,
							scale: 1.12,
							ease: "none",
							duration: 0.08,
						},
						0.49,
					)
					.to(
						processing,
						{
							autoAlpha: 1,
							scaleX: 1,
							ease: "none",
							duration: 0.16,
						},
						0.52,
					)
					.to(
						sendButton,
						{
							backgroundColor: "#111111",
							scale: 1,
							ease: "none",
							duration: 0.1,
						},
						0.56,
					)
					.to(sendArrow, { y: 0, ease: "none", duration: 0.1 }, 0.56)
					.to(
						sendRing,
						{
							autoAlpha: 0,
							scale: 1.45,
							ease: "none",
							duration: 0.14,
						},
						0.58,
					)
					.to(
						prompt,
						{
							autoAlpha: 0.18,
							y: 76,
							scale: 0.92,
							ease: "none",
							duration: 0.22,
						},
						0.66,
					)
					.to(
						source,
						{
							autoAlpha: 0.42,
							scale: 0.76,
							y: 4,
							ease: "none",
							duration: 0.22,
						},
						0.66,
					)
					.to(outputStage, { autoAlpha: 1, ease: "none", duration: 0.1 }, 0.7)
					.set(outputStage, { pointerEvents: "auto" }, 0.7)
					.to(
						cards,
						{
							autoAlpha: 1,
							filter: "blur(0px)",
							scale: 1,
							xPercent: 0,
							y: 0,
							ease: "none",
							stagger: 0.055,
							duration: 0.34,
						},
						0.73,
					)
					.to(source, { autoAlpha: 0, ease: "none", duration: 0.12 }, 0.9)
					.to(prompt, { autoAlpha: 0, ease: "none", duration: 0.12 }, 0.9);
			}, sectionRef);

			window.requestAnimationFrame(() => ScrollTrigger.refresh());
		}

		setupAnimation();

		return () => {
			disposed = true;
			ctx?.revert();
		};
	}, []);

	return (
		<section
			aria-label="Velion workflow generator"
			className="relative isolate min-h-[100svh] overflow-hidden bg-background text-velion-j-text"
			id="product-loop"
			ref={sectionRef}
		>
			<span className="absolute top-0" id="partnership" />

			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-0 bg-[radial-gradient(circle_at_50%_34%,rgba(238,122,80,0.06),transparent_26%),radial-gradient(circle_at_12%_18%,rgba(79,125,243,0.08),transparent_24%),linear-gradient(180deg,rgba(23,23,23,0.018),transparent_44%)]"
			/>

			<div className="relative z-[1] mx-auto flex min-h-[100svh] w-full max-w-[1760px] flex-col items-center px-[clamp(24px,4vw,72px)] py-[clamp(92px,11vh,128px)]">
				<div className="mx-auto max-w-[660px] text-center">
					<p className="mb-4 font-protokoll text-[0.72rem] font-medium uppercase leading-none tracking-[0.34em] text-velion-j-text/38">
						Agentic intelligence
					</p>

					<h2 className="m-0 font-arbeit text-[clamp(3.35rem,5.25vw,7.35rem)] font-light leading-[0.93] tracking-[-0.072em] text-velion-j-text">
						Thinking, in workflows
					</h2>

					<p className="mx-auto mt-6 max-w-[520px] font-protokoll text-[clamp(0.98rem,1vw,1.12rem)] font-light leading-[1.48] text-velion-text-muted">
						One customer signal can become a reply, a route, a policy check,
						and an audited action.
					</p>
				</div>

				<div className="relative mt-[clamp(42px,5vh,64px)] flex flex-1 items-center justify-center overflow-visible max-[899px]:w-full">
					<div className="relative flex w-full flex-col items-center justify-center overflow-visible px-5">
						<SourceFrame />
						<PromptComposer />

						<div
							className="absolute bottom-[clamp(16px,4vh,44px)] grid w-[min(90vw,1180px)] grid-cols-4 gap-[clamp(14px,1.4vw,24px)] max-[899px]:static max-[899px]:mt-16 max-[899px]:grid-cols-1 max-[899px]:opacity-100"
							data-feature-output-stage
						>
							{workflowCards.map((card) => (
								<OutputCard card={card} key={card.title} />
							))}
						</div>
					</div>
				</div>
			</div>
		</section>
	);
}

export default FeatureCardsSection;