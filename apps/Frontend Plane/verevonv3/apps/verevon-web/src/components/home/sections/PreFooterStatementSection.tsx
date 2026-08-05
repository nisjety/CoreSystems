"use client";

import { Fragment, useEffect, useRef } from "react";
import { ArrowButton } from "@/components/ui/ArrowButton";

const preFooterTitleLines = [
	["Fra", "kunnskap", "til", "handling."],
	["Med", "kontroll", "underveis."],
] as const;

const preFooterNextPhraseLines = [
	["Finn", "grunnlaget."],
	["Få", "neste", "steg", "klart."],
] as const;

function AnimatedTitleLine({ words }: { words: readonly string[] }) {
	return (
		<span className="block whitespace-nowrap" aria-hidden="true">
			{words.map((word, wordIndex) => (
				<Fragment key={`${word}-${wordIndex}`}>
					<span className="relative inline-block">
						{[...word].map((character, characterIndex) => (
							<span
								className="inline-block will-change-[opacity,transform]"
								key={`${word}-${character}-${characterIndex}`}
								data-prefooter-title-character
							>
								{character}
							</span>
						))}
					</span>
					{wordIndex < words.length - 1 ? " " : null}
				</Fragment>
			))}
		</span>
	);
}

function AnimatedPhraseLine({ words }: { words: readonly string[] }) {
	return (
		<span className="block whitespace-nowrap" aria-hidden="true">
			{words.map((word, wordIndex) => (
				<Fragment key={`${word}-${wordIndex}`}>
					<span
						className="inline-block will-change-[opacity,transform]"
						data-prefooter-next-word
					>
						{word}
					</span>
					{wordIndex < words.length - 1 ? " " : null}
				</Fragment>
			))}
		</span>
	);
}

export function PreFooterStatementSection() {
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

				const footerBg = section.querySelector<HTMLElement>(
					"[data-prefooter-footer-bg]",
				);
				const frame = section.querySelector<HTMLElement>(
					"[data-prefooter-frame]",
				);
				const firstPhase = section.querySelector<HTMLElement>(
					"[data-prefooter-first-phase]",
				);
				const secondPhase = section.querySelector<HTMLElement>(
					"[data-prefooter-second-phase]",
				);
				const secondPhaseButtons = section.querySelector<HTMLElement>(
					"[data-prefooter-second-buttons]",
				);
				const ctaSafeZone = section.querySelector<HTMLElement>(
					"[data-prefooter-cta-safe-zone]",
				);
				const nextPhraseTitle = section.querySelector<HTMLElement>(
					"[data-prefooter-next-title]",
				);
				const titleCharacters = section.querySelectorAll<HTMLElement>(
					"[data-prefooter-title-character]",
				);
				const nextPhraseWords = section.querySelectorAll<HTMLElement>(
					"[data-prefooter-next-word]",
				);
				const cards = section.querySelectorAll<HTMLElement>(
					"[data-prefooter-card]",
				);

				if (
					!frame ||
					!footerBg ||
					!firstPhase ||
					!secondPhase ||
					!secondPhaseButtons ||
					!nextPhraseTitle ||
					!ctaSafeZone
				) {
					return;
				}

				if (reduceMotion) {
					gsap.set(footerBg, { autoAlpha: 1 });
					gsap.set(firstPhase, { autoAlpha: 0, y: 0, scale: 1 });
					gsap.set(titleCharacters, {
						autoAlpha: 1,
						x: 0,
						y: 0,
						z: 0,
					});
					gsap.set(secondPhase, { autoAlpha: 1, y: 0, scale: 1 });
					gsap.set(nextPhraseTitle, { y: 0 });
					gsap.set(nextPhraseWords, { autoAlpha: 1, yPercent: 0 });
					gsap.set(secondPhaseButtons, { y: 0 });
					gsap.set(ctaSafeZone, { y: 0 });
					gsap.set(cards, {
						autoAlpha: 1,
						y: 0,
						scale: 1,
						rotate: 0,
					});
					return;
				}

				gsap.set(footerBg, { autoAlpha: 0 });
				gsap.set(firstPhase, {
					autoAlpha: 1,
					y: () => -frame.offsetHeight * 0.3,
					scale: 1,
				});
				gsap.set(secondPhase, { autoAlpha: 0, y: 42, scale: 0.985 });
				gsap.set(nextPhraseTitle, { y: 0 });
				gsap.set(secondPhaseButtons, { y: 0 });
				gsap.set(ctaSafeZone, { y: 0 });
				gsap.set(titleCharacters, { autoAlpha: 0, x: 0, y: 0, z: 0 });
				gsap.set(nextPhraseWords, { autoAlpha: 0, yPercent: 100 });
				gsap.set(cards, {
					autoAlpha: 0,
					y: 48,
					scale: 0.94,
					rotate: (index) => [-5, 4, -3, 5, -4][index] ?? 0,
				});

				const keepCtaBelowNavbar = () => {
					const navbar = document.querySelector<HTMLElement>("[data-site-nav]");

					if (!navbar) {
						return;
					}

					const safeTop =
						navbar.getBoundingClientRect().bottom + window.innerHeight * 0.1;
					const ctaTop = ctaSafeZone.getBoundingClientRect().top;

					gsap.set(ctaSafeZone, {
						y: Math.max(0, safeTop - ctaTop),
					});
				};

				gsap.timeline({
					scrollTrigger: {
						trigger: section,
						start: "top 75%",
						end: "bottom 10%",
						scrub: 1.25,
						id: "verevon-prefooter-two-phase",
						invalidateOnRefresh: true,
						onUpdate: keepCtaBelowNavbar,
					},
				})
					.to(
						cards,
						{
							autoAlpha: 1,
							y: 0,
							scale: 1,
							stagger: 0.06,
							ease: "none",
							duration: 0.5,
						},
						0,
					)
					.to(
						titleCharacters,
						{
							autoAlpha: 1,
							force3D: true,
							stagger: 0.012,
							ease: "none",
							duration: 0.28,
						},
						0.2,
					)
					.to(
						firstPhase,
						{
							y: 0,
							ease: "power2.out",
							duration: 0.55,
						},
						0,
					)
					.to(
						firstPhase,
						{
							autoAlpha: 0,
							y: -12,
							scale: 0.86,
							ease: "power2.in",
							duration: 0.46,
						},
						1.3,
					)
					.to(
						footerBg,
						{
							autoAlpha: 1,
							ease: "none",
							duration: 0.46,
						},
						1.36,
					)
					.to(
						secondPhase,
						{
							autoAlpha: 1,
							y: 0,
							scale: 1,
							ease: "none",
							duration: 0.38,
						},
						1.46,
					)
					.to(
						nextPhraseWords,
						{
							autoAlpha: 1,
							yPercent: 0,
							stagger: 0.03,
							ease: "power2.out",
							duration: 0.24,
						},
						1.54,
					)
					.to(
						nextPhraseTitle,
						{
							y: () => frame.offsetHeight * 0.1,
							ease: "none",
							duration: 0.62,
						},
						1.82,
					)
					.to(
						secondPhaseButtons,
						{
							y: () => {
								const safeBottom =
									frame.getBoundingClientRect().bottom - 24;
								const currentBottom =
									secondPhaseButtons.getBoundingClientRect()
										.bottom;

								return Math.max(
									0,
									Math.min(
										frame.offsetHeight * 0.1,
										safeBottom - currentBottom,
									),
								);
							},
							ease: "none",
							duration: 0.62,
						},
						1.82,
					)
					.to(
						secondPhaseButtons,
						{
							y: () => {
								const safeBottom =
									frame.getBoundingClientRect().bottom - 24;
								const currentBottom =
									secondPhaseButtons.getBoundingClientRect()
										.bottom;
								const phraseOffset = frame.offsetHeight * 0.1;

								return (
									phraseOffset +
									Math.max(
										0,
										Math.min(
											window.innerHeight * 0.53,
											safeBottom - currentBottom,
										),
									)
								);
							},
							ease: "none",
							duration: 0.86,
						},
						2.44,
					);

				window.requestAnimationFrame(keepCtaBelowNavbar);
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
			ref={sectionRef}
			aria-label="Verevon avsluttende oppfordring"
			className="relative min-h-[85svh] overflow-clip bg-verevon-footer-bg text-verevon-j-text"
			data-prefooter-scroll
		>
			<div
				className="relative z-[1] h-[85svh] min-h-[560px] overflow-hidden bg-background"
				data-prefooter-frame
			>
				<div
					aria-hidden="true"
					className="absolute inset-0 z-0 bg-verevon-footer-bg opacity-0"
					data-prefooter-footer-bg
				/>

				<div
					aria-hidden="true"
					className="pointer-events-none absolute inset-0 z-[2]"
				>
					<div
						className="absolute -left-[6%] top-[12%] aspect-square w-20 overflow-hidden rounded-[2px] bg-verevon-bg-mist shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:left-[2%] md:top-[10%] md:w-[100px] lg:w-[10%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[radial-gradient(circle_at_34%_28%,rgba(238,122,80,0.32),transparent_28%),linear-gradient(135deg,#f8f8f7,#d9e3e4)]" />
						<div className="absolute inset-x-[18%] bottom-[24%] h-px bg-verevon-j-text/25" />
						<div className="absolute left-[28%] top-[26%] size-7 rounded-full border border-verevon-j-text/20" />
					</div>

					<div
						className="absolute left-[34%] top-[2%] aspect-[0.91] w-[72px] overflow-hidden rounded-[2px] bg-verevon-bg-mist shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:left-[32%] md:top-0 md:w-[88px] lg:w-[8%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.08)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.08)_1px,transparent_1px),linear-gradient(145deg,#ffffff,#ecebea)] bg-[length:24px_24px,24px_24px,100%_100%]" />
						<div className="absolute bottom-[18%] left-[18%] h-[42%] w-[22%] bg-verevon-j-text/20 blur-[1px]" />
						<div className="absolute bottom-[18%] right-[24%] h-[34%] w-[18%] bg-verevon-j-text/16 blur-[1px]" />
					</div>

					<div
						className="absolute -right-[4%] top-[4%] aspect-square w-20 overflow-hidden rounded-full bg-verevon-coral-soft shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:right-[14%] md:top-[2%] md:w-[88px] lg:w-[8%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[radial-gradient(circle_at_50%_45%,rgba(238,122,80,0.44),transparent_30%),radial-gradient(circle_at_46%_52%,rgba(121,56,25,0.18),transparent_46%)]" />
						<div className="absolute left-1/2 top-[18%] h-[62%] w-px -translate-x-1/2 bg-verevon-a-earth/30" />
						<div className="absolute left-[28%] top-[42%] h-px w-[44%] rotate-[-28deg] bg-verevon-a-earth/30" />
					</div>

					<div
						className="absolute bottom-[6%] left-[4%] aspect-[1.08] w-24 overflow-hidden rounded-[2px] bg-verevon-coral shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:bottom-[8%] md:left-[18%] md:w-[100px] lg:w-[8%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[linear-gradient(145deg,rgba(238,122,80,0.94),rgba(247,200,168,0.88))]" />
						<div className="absolute left-[28%] top-[34%] size-2 rounded-full bg-verevon-j-text/55" />
						<div className="absolute right-[28%] top-[34%] size-2 rounded-full bg-verevon-j-text/55" />
						<div className="absolute left-[30%] top-[58%] h-px w-[40%] rounded-full bg-verevon-j-text/55" />
					</div>

					<div
						className="absolute top-[64%] right-[16%] aspect-square w-[100px] overflow-hidden rounded-[2px] bg-verevon-bg-mist shadow-[0_10px_28px_rgba(23,23,23,0.035)] md:bottom-[3%] md:right-[26%] md:w-[110px] lg:w-[10%]"
						data-prefooter-card
					>
						<div className="absolute inset-0 bg-[linear-gradient(180deg,#eef4ff,#f8f8f7)]" />
						<div className="absolute bottom-[12%] left-1/2 h-[70%] w-px -translate-x-1/2 bg-verevon-h-teal-deep/35" />
						<div className="absolute left-[20%] top-[28%] h-px w-[48%] rotate-[28deg] bg-verevon-h-teal-deep/35" />
						<div className="absolute right-[18%] top-[38%] h-px w-[42%] rotate-[-18deg] bg-verevon-h-teal-deep/35" />
					</div>
				</div>

				<div className="absolute inset-0 z-[3] grid place-items-center overflow-hidden px-[clamp(24px,4vw,72px)]">
					<div
						className="absolute left-1/2 top-[40%] grid w-[min(94vw,1280px)] -translate-x-1/2 -translate-y-1/2 justify-items-center text-center"
						data-prefooter-first-phase
					>
						<h2
							aria-label="Fra kunnskap til handling. Med kontroll underveis."
							className="m-0 max-w-none whitespace-nowrap font-arbeit text-[clamp(1.5rem,4vw,5.5rem)] font-light leading-[0.88] tracking-[-0.08em] text-verevon-j-text max-[760px]:whitespace-normal"
						>
							{preFooterTitleLines.map((line, index) => (
								<AnimatedTitleLine
									key={`title-line-${index}`}
									words={line}
								/>
							))}
						</h2>
					</div>

					<div
						className="absolute left-1/2 top-[40%] w-[min(94vw,1280px)] -translate-x-1/2 -translate-y-1/2 text-center opacity-0"
						data-prefooter-second-phase
					>
						<h2
							aria-label="Finn grunnlaget. Få neste steg klart."
							className="m-0 max-w-none whitespace-nowrap font-arbeit text-[clamp(1.5rem,4vw,5.5rem)] font-light leading-[0.88] tracking-[-0.08em] text-verevon-j-text max-[760px]:whitespace-normal"
							data-prefooter-next-title
						>
							{preFooterNextPhraseLines.map((line, index) => (
								<AnimatedPhraseLine
									key={`phrase-line-${index}`}
									words={line}
								/>
							))}
						</h2>

					<div
						className="absolute left-1/2 top-full mt-[clamp(32px,4vw,58px)] w-max max-w-[calc(100vw-48px)] -translate-x-1/2"
						data-prefooter-second-buttons
					>
						<div
							className="flex flex-wrap items-center justify-center gap-5"
							data-prefooter-cta-safe-zone
						>
							<ArrowButton href="#produkt" variant="muted">
								Se hvordan Verevon fungerer
							</ArrowButton>

							<ArrowButton href="#kontakt" variant="dark">
								Snakk med oss
							</ArrowButton>
						</div>
					</div>
					</div>
				</div>
			</div>

			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-x-0 bottom-0 z-0 h-[42svh] bg-[linear-gradient(180deg,transparent_0%,rgba(246,246,244,0.18)_24%,rgba(246,246,244,0.64)_58%,var(--verevon-footer-bg)_100%)]"
			/>
		</section>
	);
}

export default PreFooterStatementSection;
