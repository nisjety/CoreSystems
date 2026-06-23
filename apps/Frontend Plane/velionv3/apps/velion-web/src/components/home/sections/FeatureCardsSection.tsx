"use client";

import { useEffect, useRef } from "react";
import { ArrowButton } from "@/components/ui/ArrowButton";

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

function PromptChars() {
	return (
		<span aria-label={promptText}>
			{promptText.split("").map((char, index) => (
				<span
					aria-hidden="true"
					className="inline-block opacity-100"
					data-feature-char
					key={`${char}-${index}`}
				>
					{char === " " ? "\u00A0" : char}
				</span>
			))}
		</span>
	);
}

function SourceFrame() {
	return (
		<div
			className="relative mx-auto mb-[clamp(82px,13vh,142px)] aspect-[3/4] w-[min(29vh,270px)] max-w-[270px] overflow-visible border border-velion-c-white/16 bg-[linear-gradient(180deg,#262625_0%,#424140_100%)] shadow-[0_20px_80px_rgba(0,0,0,0.24)] will-change-transform"
			data-feature-source
		>
			<div className="absolute inset-0 overflow-hidden">
				<img
					alt=""
					className="h-full w-full select-none object-cover opacity-75 saturate-[0.76]"
					draggable={false}
					src="/velion-vibe/human-haze.png"
				/>
			</div>

			<div className="absolute inset-x-0 top-[-20px] flex h-[15px] select-none items-center justify-between text-[10px] leading-none text-velion-c-white/38">
				<span className="flex items-center gap-1 font-protokoll">
					<span className="size-[5px] rounded-full bg-velion-coral/70" />
					signal
				</span>
				<span className="font-protokoll">customer context</span>
			</div>

			{["left-[-3.5px] top-[-3.5px]", "right-[-3.5px] top-[-3.5px]", "bottom-[-3.5px] left-[-3.5px]", "bottom-[-3.5px] right-[-3.5px]"].map(
				(position) => (
					<span
						aria-hidden="true"
						className={[
							"absolute size-[6px] border border-velion-c-white/22 bg-velion-c-white",
							position,
						].join(" ")}
						key={position}
					/>
				),
			)}
		</div>
	);
}

function PromptComposer() {
	return (
		<div
			className="absolute bottom-[clamp(68px,12vh,118px)] left-1/2 z-[5] w-[min(91vw,520px)] origin-bottom -translate-x-1/2 rounded-[22px] border border-velion-c-white/12 bg-[rgba(248,248,247,0.92)] p-4 text-velion-j-text opacity-100 shadow-[0_24px_90px_rgba(0,0,0,0.24)] backdrop-blur-2xl will-change-transform"
			data-feature-prompt
		>
			<p className="mb-4 font-protokoll text-[clamp(1rem,1.1vw,1.16rem)] font-light leading-[1.45] tracking-[-0.015em]">
				<span className="mr-2 inline-flex h-8 items-center rounded-xl bg-velion-j-text/[0.06] px-2 font-protokoll text-[0.86em] text-velion-j-text/70">
					Velion
				</span>
				<PromptChars />
			</p>

			<div className="flex items-center justify-between">
				<div className="flex items-center gap-2 text-velion-j-text/42">
					<span className="grid size-9 place-items-center rounded-full border border-velion-j-text/10">
						+
					</span>
					<span className="h-5 w-px bg-velion-j-text/10" />
					<span className="text-sm">sources</span>
				</div>

				<div className="flex items-center gap-2">
					<span className="grid size-9 place-items-center rounded-full bg-velion-j-text/[0.06] text-sm text-velion-j-text/52">
						↯
					</span>
					<span className="grid size-9 place-items-center rounded-full bg-velion-j-text text-velion-c-white">
						↑
					</span>
				</div>
			</div>
		</div>
	);
}

function OutputCard({ card }: { card: WorkflowCard }) {
	return (
		<article
			className="group flex min-w-0 flex-col gap-3 will-change-[transform,opacity,filter]"
			data-feature-card
		>
			<div className="flex h-[15px] select-none items-center justify-between text-[10px] leading-none text-velion-c-white/42">
				<span className="flex items-center gap-1 font-protokoll">
					<span className="size-[5px] rounded-full bg-velion-coral/70" />
					{card.kicker}
				</span>
				<span className="font-protokoll">Velion output</span>
			</div>

			<div className="relative aspect-[3/4] overflow-hidden bg-velion-c-white/8">
				<img
					alt=""
					className="h-full w-full select-none object-cover opacity-[0.78] saturate-[0.72] transition-transform duration-700 group-hover:scale-[1.035]"
					draggable={false}
					src={card.image}
				/>
				<div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(23,23,23,0.08),rgba(23,23,23,0.52))]" />
				<div className="absolute bottom-4 left-4 right-4">
					<h3 className="font-arbeit text-[clamp(1.45rem,2vw,2.2rem)] font-light leading-[1.04] tracking-[-0.055em] text-velion-c-white">
						{card.title}
					</h3>
				</div>
			</div>

			<p className="m-0 min-h-[4.2em] font-protokoll text-[clamp(0.92rem,0.92vw,1.02rem)] font-light leading-[1.38] text-velion-c-white/58">
				{card.text}
			</p>

			<ArrowButton href={card.href} variant="light">
				{card.link}
			</ArrowButton>
		</article>
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

				const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
				const isCompact = window.matchMedia("(max-width: 899px)").matches;
				const source = section.querySelector<HTMLElement>("[data-feature-source]");
				const prompt = section.querySelector<HTMLElement>("[data-feature-prompt]");
				const outputStage = section.querySelector<HTMLElement>("[data-feature-output-stage]");
				const cards = Array.from(section.querySelectorAll<HTMLElement>("[data-feature-card]"));
				const chars = Array.from(section.querySelectorAll<HTMLElement>("[data-feature-char]"));

				if (!source || !prompt || !outputStage || cards.length === 0) {
					return;
				}

				if (reduceMotion || isCompact) {
					gsap.set([source, prompt, outputStage, cards, chars], {
						autoAlpha: 1,
						clearProps: "filter,transform",
					});
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
							id: "velion-feature-cards-lovart-flow",
							invalidateOnRefresh: true,
							refreshPriority: 2,
						},
					})
					.to(source, { y: -18, scale: 0.965, ease: "none", duration: 0.16 }, 0.04)
					.to(prompt, { autoAlpha: 1, y: 0, scale: 1, ease: "none", duration: 0.2 }, 0.12)
					.to(chars, { autoAlpha: 1, duration: 0.22, ease: "none", stagger: 0.003 }, 0.2)
					.to(prompt, { autoAlpha: 0.18, y: 76, scale: 0.92, ease: "none", duration: 0.22 }, 0.5)
					.to(source, { autoAlpha: 0.42, scale: 0.76, y: 4, ease: "none", duration: 0.22 }, 0.5)
					.to(outputStage, { autoAlpha: 1, ease: "none", duration: 0.1 }, 0.52)
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
						0.56,
					)
					.to(source, { autoAlpha: 0, ease: "none", duration: 0.12 }, 0.82)
					.to(prompt, { autoAlpha: 0, ease: "none", duration: 0.12 }, 0.82);
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
			className="relative isolate min-h-[100svh] overflow-hidden bg-[#1f211c] text-velion-c-white"
			data-nav-dark-section
			id="product-loop"
			ref={sectionRef}
		>
			<span className="absolute top-0" id="partnership" />

			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-0 bg-[radial-gradient(circle_at_50%_34%,rgba(238,122,80,0.09),transparent_26%),radial-gradient(circle_at_12%_18%,rgba(79,125,243,0.11),transparent_24%),linear-gradient(180deg,rgba(255,255,255,0.025),transparent_44%)]"
			/>

			<div className="relative z-[1] mx-auto flex min-h-[100svh] w-full max-w-[1760px] flex-col items-center px-[clamp(24px,4vw,72px)] py-[clamp(92px,11vh,128px)]">
				<div className="mx-auto max-w-[660px] text-center">
					<p className="mb-4 font-protokoll text-[0.72rem] font-medium uppercase leading-none tracking-[0.34em] text-velion-c-white/38">
						Agentic intelligence
					</p>
					<h2 className="m-0 font-arbeit text-[clamp(3.35rem,5.25vw,7.35rem)] font-light leading-[0.93] tracking-[-0.072em] text-velion-c-white/78">
						Thinking, in workflows
					</h2>
					<p className="mx-auto mt-6 max-w-[520px] font-protokoll text-[clamp(0.98rem,1vw,1.12rem)] font-light leading-[1.48] text-velion-c-white/48">
						One customer signal can become a reply, a route, a policy check, and an audited action.
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
