"use client";

import { useEffect, useRef, useState } from "react";

const promptLines = [
	"Lag en rolig produktfilm for Velion.",
	"Start med ett kundesignal, ikke en kampanje.",
	"Bakgrunnen skal ha samme myke blå tone som seksjonen.",
	"Vis hvordan et arbeidsområde vokser frem fra en enkel instruks.",
	"La kilder, utkast og godkjenning føles som ett sammenhengende system.",
	"Hold uttrykket stille, presist og menneskelig.",
	"Ikke selg magi. Vis arbeidet som blir gjort.",
	"Avslutt med at handlingen er klar, men fortsatt godkjennbar.",
];

type PromptFeedState = {
	activeIndex: number;
	activeText: string;
	completed: string[];
	isTyping: boolean;
};

const initialPromptFeed: PromptFeedState = {
	activeIndex: 0,
	activeText: "",
	completed: [],
	isTyping: true,
};

export function PromptSection() {
	const completedCountRef = useRef(0);
	const promptWindowRef = useRef<HTMLDivElement | null>(null);
	const sectionRef = useRef<HTMLElement | null>(null);
	const [promptFeed, setPromptFeed] =
		useState<PromptFeedState>(initialPromptFeed);

	useEffect(() => {
		const promptWindow = promptWindowRef.current;
		const section = sectionRef.current;

		if (!promptWindow || !section) {
			return;
		}

		const reduceMotion = window.matchMedia(
			"(prefers-reduced-motion: reduce)",
		).matches;

		if (reduceMotion) {
			const frame = window.requestAnimationFrame(() => {
				setPromptFeed({
					activeIndex: promptLines.length,
					activeText: "",
					completed: promptLines,
					isTyping: false,
				});
			});

			return () => window.cancelAnimationFrame(frame);
		}

		let active = false;
		let disposed = false;
		let timeout: number | undefined;

		const observer = new IntersectionObserver(
			(entries) => {
				const [entry] = entries;
				active = Boolean(entry?.isIntersecting);
			},
			{ threshold: 0.34 },
		);

		observer.observe(section);

		const clearTimer = () => {
			if (timeout !== undefined) {
				window.clearTimeout(timeout);
				timeout = undefined;
			}
		};

		const schedule = (callback: () => void, delay: number) => {
			clearTimer();
			timeout = window.setTimeout(callback, delay);
		};

		const waitUntilVisible = (callback: () => void) => {
			if (disposed) {
				return;
			}

			if (active) {
				callback();
				return;
			}

			schedule(() => waitUntilVisible(callback), 180);
		};

		const resetFeed = () => {
			setPromptFeed(initialPromptFeed);
			promptWindow.scrollTop = 0;
		};

		const typeLine = (lineIndex: number, charIndex: number) => {
			waitUntilVisible(() => {
				if (disposed) {
					return;
				}

				const line = promptLines[lineIndex];

				if (!line) {
					schedule(() => {
						resetFeed();
						typeLine(0, 0);
					}, 1250);
					return;
				}

				if (charIndex <= line.length) {
					setPromptFeed((current) => ({
						...current,
						activeIndex: lineIndex,
						activeText: line.slice(0, charIndex),
						isTyping: true,
					}));

					schedule(
						() => typeLine(lineIndex, charIndex + 1),
						charIndex === 0 ? 240 : 24,
					);
					return;
				}

				schedule(() => {
					setPromptFeed((current) => ({
						activeIndex: lineIndex + 1,
						activeText: "",
						completed: [...current.completed, line],
						isTyping: false,
					}));

					schedule(() => typeLine(lineIndex + 1, 0), 260);
				}, 560);
			});
		};

		typeLine(0, 0);

		return () => {
			disposed = true;
			observer.disconnect();
			clearTimer();
		};
	}, []);

	useEffect(() => {
		const promptWindow = promptWindowRef.current;

		if (!promptWindow) {
			return;
		}

		const frame = window.requestAnimationFrame(() => {
			const behavior =
				completedCountRef.current === promptFeed.completed.length
					? "auto"
					: "smooth";

			completedCountRef.current = promptFeed.completed.length;

			promptWindow.scrollTo({
				behavior,
				top: promptWindow.scrollHeight,
			});
		});

		return () => window.cancelAnimationFrame(frame);
	}, [promptFeed.activeText, promptFeed.completed]);

	const activeLine = promptLines[promptFeed.activeIndex] ?? "";
	const activeLineProgress =
		activeLine.length > 0
			? promptFeed.activeText.length / activeLine.length
			: 0;
	const promptProgress = Math.min(
		1,
		(promptFeed.completed.length + activeLineProgress) / promptLines.length,
	);

	return (
		<section
			aria-label="Prompt til Velion"
			className="promt-section relative isolate min-h-[100svh] overflow-hidden bg-[#dfeaf4] text-velion-j-text"
			data-promt-section
			id="promt-section"
			ref={sectionRef}
		>
			<div
				aria-hidden="true"
				className="absolute inset-0 z-0 bg-[linear-gradient(90deg,rgba(23,58,105,0.055)_1px,transparent_1px),linear-gradient(180deg,rgba(23,58,105,0.045)_1px,transparent_1px),linear-gradient(155deg,#f3f9fc_0%,#dfeaf4_38%,#bdd6eb_100%)] bg-[length:96px_96px,96px_96px,100%_100%]"
			/>

			<div
				aria-hidden="true"
				className="absolute inset-0 z-[1] bg-[linear-gradient(180deg,rgba(255,255,255,0.42)_0%,rgba(223,234,244,0)_44%),linear-gradient(90deg,rgba(236,245,250,0.98)_0%,rgba(226,239,248,0.86)_30%,rgba(208,226,241,0.28)_62%,rgba(190,213,232,0.68)_100%)]"
			/>

			<div className="relative z-[2] mx-auto grid min-h-[100svh] w-full max-w-[1760px] grid-cols-[minmax(0,0.78fr)_minmax(420px,1fr)] items-center gap-[clamp(42px,5.6vw,112px)] px-[clamp(56px,5.55vw,208px)] py-[clamp(84px,9vh,136px)] max-[1023px]:grid-cols-1 max-[1023px]:px-[clamp(24px,4vw,56px)]">
				<div className="min-w-0">
					<p
						className="fade-out-top m-0 font-protokoll text-[0.72rem] font-medium uppercase leading-none tracking-[0.32em] text-velion-j-text/38"
						data-fade-out-top
					>
						Prompt til Velion
					</p>

					<h2
						className="fade-out-top m-0 mt-5 max-w-[720px] font-arbeit text-[clamp(3rem,5vw,7.15rem)] font-light leading-[0.92] tracking-[-0.072em] text-velion-j-text"
						data-fade-out-top
					>
						Den gjør det du ber om
					</h2>

					<p
						className="fade-out-top m-0 mt-[clamp(24px,2.6vw,38px)] max-w-[640px] font-protokoll text-[clamp(1.02rem,1vw,1.18rem)] font-light leading-[1.5] text-velion-text-muted"
						data-fade-out-top
					>
						Et menneske skriver ikke en kommando til en maskin. Det
						beskriver en ønsket retning, et uttrykk og grensene
						arbeidet skal holde seg innenfor.
					</p>

					<div className="fade-out-top" data-fade-out-top>
						<div
							className="relative mt-[clamp(30px,3.6vw,54px)] h-[min(42svh,430px)] min-h-[300px] overflow-hidden [mask-image:linear-gradient(transparent,#000_14%,#000_86%,transparent)] [-webkit-mask-image:linear-gradient(transparent,#000_14%,#000_86%,transparent)] motion-reduce:h-auto motion-reduce:overflow-visible motion-reduce:[mask-image:none] motion-reduce:[-webkit-mask-image:none] max-[1023px]:h-[340px] max-[1023px]:min-h-[300px]"
							ref={promptWindowRef}
						>
							<div
								aria-hidden="true"
								className="flex min-h-full flex-col justify-end gap-3 pb-[clamp(92px,11vh,132px)] pt-[clamp(64px,7vh,96px)]"
								data-promt-track
							>
								{promptFeed.completed.map((line, index) => (
									<p
										className="m-0 grid grid-cols-[42px_minmax(0,1fr)] gap-4 border-t border-velion-j-text/10 pt-4 font-protokoll text-[clamp(1.06rem,1.18vw,1.42rem)] font-light leading-[1.38] text-velion-j-text"
										data-promt-line
										key={`${line}-${index}`}
									>
										<span className="pt-[0.18em] text-[0.68em] leading-none tracking-[0.18em] text-velion-j-text/32">
											{String(index + 1).padStart(2, "0")}
										</span>
										<span>{line}</span>
									</p>
								))}

								{promptFeed.activeText ||
								promptFeed.isTyping ? (
									<p
										className="m-0 grid grid-cols-[42px_minmax(0,1fr)] gap-4 border-t border-velion-j-text/10 pt-4 font-protokoll text-[clamp(1.06rem,1.18vw,1.42rem)] font-light leading-[1.38] text-velion-j-text"
										data-promt-line
									>
										<span className="pt-[0.18em] text-[0.68em] leading-none tracking-[0.18em] text-velion-j-text/32">
											{String(
												promptFeed.activeIndex + 1,
											).padStart(2, "0")}
										</span>
										<span>
											{promptFeed.activeText}
											<span
												aria-hidden="true"
												className="ml-1 inline-block translate-y-[-0.04em] animate-pulse text-velion-coral"
											>
												_
											</span>
										</span>
									</p>
								) : null}
							</div>
						</div>
					</div>

					<ul className="sr-only">
						{promptLines.map((line) => (
							<li key={line}>{line}</li>
						))}
					</ul>

					<p
						className="fade-out-top m-0 mt-5 font-protokoll text-[clamp(0.92rem,0.92vw,1.02rem)] font-light leading-none text-velion-j-text/52"
						data-fade-out-top
					>
						<span>Open prompt.</span>{" "}
						<span className="text-velion-j-text/70">
							Open output-_
						</span>{" "}
						<span
							data-promt-glitch-noise
							className="animate-pulse text-velion-coral/80"
						>
							{"}?!<\\--_/&"}
						</span>
					</p>
				</div>

				<div className="relative flex min-h-[min(760px,78svh)] min-w-0 items-center justify-center max-[1023px]:min-h-[min(620px,72svh)]">
					<div
						className="relative aspect-[1.28] w-full max-w-[900px] overflow-hidden bg-[#dfeaf4] will-change-[clip-path,opacity,transform]"
						data-promt-video-surface
					>
						<video
							aria-label="Velion bygger en arbeidsflate fra en norsk prompt."
							autoPlay
							className="h-full w-full object-cover object-center will-change-[filter,transform,opacity]"
							data-promt-video
							loop
							muted
							playsInline
							poster="/velion-product-shots/dashboard-expanded-prompt.png"
							preload="metadata"
						>
							<source
								src="/velion-product-shots/velion-dashboard-typing.mp4"
								type="video/mp4"
							/>
						</video>

						<div
							aria-hidden="true"
							className="pointer-events-none absolute inset-0 bg-[linear-gradient(90deg,#dfeaf4_0%,rgba(223,234,244,0)_18%,rgba(223,234,244,0)_82%,#dfeaf4_100%),linear-gradient(180deg,#dfeaf4_0%,rgba(223,234,244,0)_18%,rgba(223,234,244,0)_80%,#dfeaf4_100%)]"
						/>
						<div
							aria-hidden="true"
							className="pointer-events-none absolute inset-0 shadow-[inset_0_0_34px_32px_rgba(223,234,244,1)]"
						/>

						<div className="absolute inset-x-[clamp(28px,4vw,58px)] bottom-[clamp(28px,4vw,58px)]">
							<div className="flex items-center justify-between gap-5 font-protokoll text-[0.72rem] uppercase leading-none tracking-[0.22em] text-velion-j-text/48">
								<span>Velion lager</span>
								<span>human approved</span>
							</div>

							<div className="mt-4 h-px overflow-hidden bg-velion-j-text/16">
								<span
									aria-hidden="true"
									className="block h-full w-full origin-left bg-velion-coral/80"
									data-promt-progress
									style={{
										transform: `scaleX(${promptProgress})`,
									}}
								/>
							</div>
						</div>
					</div>
				</div>
			</div>
		</section>
	);
}

export default PromptSection;
