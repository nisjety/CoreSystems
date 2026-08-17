"use client";

import {
	useCallback,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
	useSyncExternalStore,
} from "react";
import { createPortal } from "react-dom";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { ArrowLeft, ArrowRight, Hand, MousePointerClick } from "lucide-react";
import {
	ModuleWorkflowCard,
	moduleCards,
} from "./FeatureWorkflowCards";

gsap.registerPlugin(ScrollTrigger);

const subscribeToClient = () => () => undefined;

export function FeaturesSection() {
	const sectionRef = useRef<HTMLElement>(null);
	const viewportRef = useRef<HTMLDivElement>(null);
	const carouselRef = useRef<HTMLDivElement>(null);
	const isDraggingRef = useRef(false);
	const hasDraggedRef = useRef(false);
	const dragStartXRef = useRef(0);
	const dragScrollLeftRef = useRef(0);
	const activeCardIndexRef = useRef(0);
	const cursorRef = useRef<HTMLDivElement>(null);
	const [activeCardIndex, setActiveCardIndex] = useState(0);
	const [lastStartIndex, setLastStartIndex] = useState(0);
	const [isCursorVisible, setIsCursorVisible] = useState(false);
	const [cursorMode, setCursorMode] = useState<
		"nav-left" | "nav-right" | "select" | "drag"
	>("nav-right");
	const isMounted = useSyncExternalStore(
		subscribeToClient,
		() => true,
		() => false,
	);

	useEffect(() => {
		activeCardIndexRef.current = activeCardIndex;
	}, [activeCardIndex]);

	const getCards = useCallback(() => {
		const carousel = carouselRef.current;
		if (!carousel) return [];

		return Array.from(
			carousel.querySelectorAll<HTMLElement>("[data-feature-card]"),
		);
	}, []);

	const getLastStartIndex = useCallback(() => {
		const carousel = carouselRef.current;
		const cards = getCards();
		if (!carousel || cards.length === 0) return 0;

		const firstCard = cards[0];
		const secondCard = cards[1];
		const stride = secondCard
			? secondCard.offsetLeft - firstCard.offsetLeft
			: firstCard.offsetWidth;
		const visibleCards = Math.max(
			1,
			Math.floor(
				(carousel.clientWidth + Math.max(0, stride - firstCard.offsetWidth)) /
					stride,
			),
		);

		return Math.max(0, cards.length - visibleCards);
	}, [getCards]);

	const findClosestCardIndex = useCallback(() => {
		const carousel = carouselRef.current;
		const cards = getCards();
		if (!carousel || cards.length === 0) return 0;

		const firstOffset = cards[0].offsetLeft;
		const maximumStartIndex = getLastStartIndex();
		let closestIndex = 0;
		let closestDistance = Number.POSITIVE_INFINITY;

		cards.slice(0, maximumStartIndex + 1).forEach((card, index) => {
			const distance = Math.abs(
				card.offsetLeft - firstOffset - carousel.scrollLeft,
			);
			if (distance < closestDistance) {
				closestDistance = distance;
				closestIndex = index;
			}
		});

		return closestIndex;
	}, [getCards, getLastStartIndex]);

	useEffect(() => {
		const carousel = carouselRef.current;
		if (!carousel) return undefined;

		let frame = 0;
		const updateActiveCard = () => {
			cancelAnimationFrame(frame);
			frame = requestAnimationFrame(() => {
				const closestIndex = findClosestCardIndex();
				activeCardIndexRef.current = closestIndex;
				setActiveCardIndex(closestIndex);
			});
		};
		const updateLayout = () => {
			setLastStartIndex(getLastStartIndex());
			updateActiveCard();
		};

		carousel.addEventListener("scroll", updateActiveCard, { passive: true });
		const resizeObserver = new ResizeObserver(updateLayout);
		resizeObserver.observe(carousel);
		requestAnimationFrame(updateLayout);

		return () => {
			cancelAnimationFrame(frame);
			resizeObserver.disconnect();
			carousel.removeEventListener("scroll", updateActiveCard);
		};
	}, [findClosestCardIndex, getLastStartIndex]);

	const scrollToCard = useCallback((index: number) => {
		const carousel = carouselRef.current;
		if (!carousel) return;

		const cards = getCards();
		const nextIndex = Math.max(0, Math.min(index, getLastStartIndex()));
		const card = cards[nextIndex];
		if (!card) return;

		activeCardIndexRef.current = nextIndex;
		setActiveCardIndex(nextIndex);
		carousel.scrollTo({
			behavior: "smooth",
			left: card.offsetLeft - cards[0].offsetLeft,
		});
	}, [getCards, getLastStartIndex]);

	const handleViewportPointerMove = (event: React.PointerEvent<HTMLElement>) => {
		setIsCursorVisible(true);
		if (cursorRef.current) {
			cursorRef.current.style.transform = `translate3d(${event.clientX}px, ${event.clientY}px, 0)`;
		}

		if (isDraggingRef.current) {
			setCursorMode("drag");
			return;
		}

		const target = event.target as HTMLElement | null;
		const card = target?.closest<HTMLElement>("[data-feature-card]");
		if (card) {
			setCursorMode("select");
			return;
		}

		const section = sectionRef.current;
		if (!section) return;
		const rect = section.getBoundingClientRect();
		setCursorMode(event.clientX - rect.left < rect.width / 2 ? "nav-left" : "nav-right");
	};

	const handleDragStart = (event: React.PointerEvent<HTMLDivElement>) => {
		if (event.button !== 0) return;

		const carousel = carouselRef.current;
		if (!carousel) return;

		isDraggingRef.current = true;
		hasDraggedRef.current = false;
		dragStartXRef.current = event.pageX;
		dragScrollLeftRef.current = carousel.scrollLeft;
		carousel.style.scrollSnapType = "none";
		carousel.setPointerCapture(event.pointerId);
		setCursorMode("drag");
	};

	const handleDragMove = (event: React.PointerEvent<HTMLDivElement>) => {
		if (!isDraggingRef.current) return;

		const carousel = carouselRef.current;
		if (!carousel) return;

		// 1:1 tracking. The old 1.35 multiplier made the strip outrun the
		// pointer, which reads as slippage rather than weight on a trackpad.
		const distance = event.pageX - dragStartXRef.current;
		if (Math.abs(distance) <= 5) return;

		hasDraggedRef.current = true;
		event.preventDefault();
		carousel.scrollLeft = dragScrollLeftRef.current - distance;
	};

	const handleDragEnd = (event: React.PointerEvent<HTMLElement>) => {
		if (!isDraggingRef.current) return;

		isDraggingRef.current = false;
		const carousel = carouselRef.current;
		if (carousel) {
			carousel.style.scrollSnapType = "x mandatory";
			if (carousel.hasPointerCapture(event.pointerId)) {
				carousel.releasePointerCapture(event.pointerId);
			}
		}

		if (hasDraggedRef.current) {
			scrollToCard(findClosestCardIndex());
		}

		const target = document.elementFromPoint(
			event.clientX,
			event.clientY,
		) as HTMLElement | null;
		const card = target?.closest<HTMLElement>("[data-feature-card]");
		if (card) {
			setCursorMode("select");
			return;
		}

		const section = sectionRef.current;
		if (section) {
			const rect = section.getBoundingClientRect();
			setCursorMode(event.clientX - rect.left < rect.width / 2 ? "nav-left" : "nav-right");
		}
	};

	const handleViewportKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
		if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;

		event.preventDefault();
		scrollToCard(
			activeCardIndexRef.current + (event.key === "ArrowLeft" ? -1 : 1),
		);
	};

	const handleViewportClick = (event: React.MouseEvent<HTMLElement>) => {
		if (hasDraggedRef.current) {
			event.preventDefault();
			event.stopPropagation();
			hasDraggedRef.current = false;
			return;
		}

		const target = event.target as HTMLElement | null;
		if (target?.closest("button")) return;

		const selectedCard = target?.closest<HTMLElement>("[data-feature-card]");
		if (selectedCard) return;

		// Capture the press before inactive card links can navigate. A card that is
		// not selected is a carousel control; only the selected card opens a link.
		event.preventDefault();
		event.stopPropagation();
		const rect = event.currentTarget.getBoundingClientRect();
		const direction = event.clientX - rect.left < rect.width / 2 ? -1 : 1;
		scrollToCard(activeCardIndexRef.current + direction);
	};

	useLayoutEffect(() => {
		const section = sectionRef.current;
		if (!section) return undefined;

		const media = gsap.matchMedia();
		media.add(
			{
				isDesktop: "(min-width: 768px)",
				reduceMotion: "(prefers-reduced-motion: reduce)",
			},
			(mediaContext) => {
				const cards = Array.from(
					section.querySelectorAll<HTMLElement>(
						"[data-feature-card]",
					),
				);
				const cardStage = section.querySelector<HTMLElement>(
					"[data-features-cards-stage]",
				);
				const intro = section.querySelector<HTMLElement>(
					"[data-features-intro]",
				);

				if (
					!mediaContext.conditions?.isDesktop ||
					!intro ||
					!cardStage ||
					cards.length === 0
				) {
					return undefined;
				}

				const context = gsap.context(() => {
					if (mediaContext.conditions?.reduceMotion) {
						cardStage.style.scrollSnapType = "x mandatory";
						gsap.set([intro, ...cards], {
							autoAlpha: 1,
							x: 0,
							xPercent: 0,
							y: 0,
							rotationX: 0,
							scale: 1,
						});
						return;
					}

					// Transformed snap targets can make the browser jump the carousel
					// while the cards fan into place. Re-enable snapping only once the
					// visual layout and the flex layout describe the same positions.
					cardStage.style.scrollSnapType = "none";

					gsap.fromTo(
						intro,
						{ autoAlpha: 0, y: 28 },
						{
							autoAlpha: 1,
							y: 0,
							ease: "none",
							scrollTrigger: {
								end: "top 52%",
								invalidateOnRefresh: true,
								start: "top 82%",
								scrub: true,
								trigger: intro,
							},
						},
					);

					// Keep this as one shared timeline, matching FeatureCardsSection.
					// The opacity beat leads, then the cards settle with the same
					// stagger, tilt, scale, and horizontal spread as the original.
					gsap.set(cards, {
						autoAlpha: 0,
						backfaceVisibility: "hidden",
						rotationX: -24,
						scale: 1.04,
						transformOrigin: "50% 0%",
						transformStyle: "preserve-3d",
						xPercent: (index) => 72 - index * 48,
						y: -8,
						force3D: true,
					});

					const cardsTimeline = gsap.timeline({
						defaults: { ease: "none" },
							scrollTrigger: {
								trigger: cardStage,
								start: "top 82%",
								end: "top 34%",
								scrub: true,
								invalidateOnRefresh: true,
								onUpdate: (self) => {
									// Only write when the value actually flips.
									// Re-assigning scroll-snap-type on every
									// scrub frame re-snaps the strip mid-drag
									// and shows up as a stutter.
									const nextSnap =
										self.progress >= 0.995
											? "x mandatory"
											: "none";
									if (
										cardStage.style.scrollSnapType !==
										nextSnap
									) {
										cardStage.style.scrollSnapType =
											nextSnap;
									}
								},
							},
					});

					cardsTimeline
						.to(
							cards,
							{
								autoAlpha: 0.78,
								stagger: 0.05,
								duration: 0.06,
							},
							0,
						)
						.to(
							cards,
							{
								scale: 1,
								xPercent: 0,
								y: 0,
								stagger: 0.05,
								duration: 0.26,
							},
							0.02,
						)
						.to(
							cards,
							{
								rotationX: 0,
								stagger: 0.05,
								duration: 0.26,
							},
							0.02,
						)
						.to(cards, { autoAlpha: 1, duration: 0.05 }, 0.45);
				}, section);

				return () => context.revert();
			},
		);

		return () => media.revert();
	}, []);

	return (
		<section
			aria-labelledby="features-title"
			className="relative isolate cursor-none overflow-x-clip bg-background py-[clamp(96px,13vh,180px)] text-verevon-j-text"
			id="features"
			onClickCapture={handleViewportClick}
			onPointerEnter={() => setIsCursorVisible(true)}
			onPointerLeave={(event) => {
				if (!isDraggingRef.current) {
					setIsCursorVisible(false);
				}
				handleDragEnd(event);
			}}
			onPointerMove={handleViewportPointerMove}
			ref={sectionRef}
		>
			<div
				aria-hidden="true"
				className="absolute inset-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.042)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.038)_1px,transparent_1px)] bg-[length:calc(100%/4)_calc(100%/3),calc(100%/4)_calc(100%/3)] max-[899px]:bg-[length:92px_92px]"
			/>
			<div
				aria-hidden="true"
				className="absolute inset-0 bg-[radial-gradient(circle_at_74%_32%,rgba(238,122,80,0.075),transparent_29%),radial-gradient(circle_at_12%_74%,rgba(41,64,74,0.05),transparent_30%),linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.8))]"
			/>

			<div className="relative z-10 w-full px-[var(--verevon-edge)] max-[760px]:px-[var(--verevon-page-pad)]">
				<header
					className="mx-auto mb-[clamp(48px,8vh,104px)] max-w-[760px] text-center md:invisible"
					data-features-intro=""
				>
					<p className="verevon-eyebrow text-verevon-coral">04 / Modulene</p>
					<h2
						className="mt-4 font-arbeit text-[clamp(2.75rem,5vw,6.1rem)] font-light leading-[0.9] tracking-[-0.07em] text-verevon-j-text text-balance"
						id="features-title"
					>
						Finn. Forstå. Få gjort.
					</h2>
					<p className="mx-auto mt-5 max-w-[600px] font-protokoll text-[clamp(0.95rem,1vw,1.1rem)] font-light leading-[1.45] text-verevon-text-muted">
						Seks moduler, én sammenheng. Dere kan starte med én og ta resten
						når arbeidet krever det.
					</p>
				</header>

				<div className="mx-auto w-full max-w-[1680px] [perspective:1400px]">
					<div
						className="relative w-full cursor-none"
						ref={viewportRef}
					>
						<div className="relative w-full">
							<div
								className="relative flex w-full snap-x snap-mandatory gap-x-[var(--feature-card-gap)] overflow-x-auto overscroll-x-contain pb-4 pt-2 [--feature-card-gap:clamp(20px,2vw,36px)] [--feature-card-width:clamp(250px,22vw,280px)] lg:[--feature-card-width:calc((100%_-_3*var(--feature-card-gap))/4)] [scrollbar-width:none] [-ms-overflow-style:none] [&::-webkit-scrollbar]:hidden touch-pan-x"
								aria-label="Verevon-modulene"
								data-features-cards-stage=""
								ref={carouselRef}
								onClickCapture={handleViewportClick}
								onKeyDown={handleViewportKeyDown}
								onPointerDown={handleDragStart}
								onPointerMove={handleDragMove}
								onPointerCancel={handleDragEnd}
								onPointerUp={handleDragEnd}
								role="group"
								tabIndex={0}
							>
								{moduleCards.map((card, index) => (
									<ModuleWorkflowCard
										card={card}
										className="w-[var(--feature-card-width)] shrink-0 snap-start"
										index={index}
										key={card.module}
										total={moduleCards.length}
									/>
								))}
							</div>
						</div>
						</div>

							<button
								aria-label="Forrige plattformkort"
								className="absolute inset-y-2 left-0 z-40 w-[clamp(36px,4vw,56px)] cursor-none bg-transparent focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-verevon-coral disabled:pointer-events-none"
								disabled={activeCardIndex === 0}
								onClick={() => scrollToCard(activeCardIndex - 1)}
								type="button"
							>
								<span className="sr-only">Forrige</span>
							</button>
							<button
								aria-label="Neste plattformkort"
								className="absolute inset-y-2 right-0 z-40 w-[clamp(36px,4vw,56px)] cursor-none bg-transparent focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-verevon-coral disabled:pointer-events-none"
								disabled={activeCardIndex === lastStartIndex}
								onClick={() => scrollToCard(activeCardIndex + 1)}
								type="button"
							>
								<span className="sr-only">Neste</span>
							</button>

					</div>
				</div>

			{isMounted
				? createPortal(
						<div
							aria-hidden="true"
							className="pointer-events-none fixed left-0 top-0 z-[100] hidden will-change-transform md:block"
							ref={cursorRef}
						>
							<div
								className={`flex size-14 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full bg-white text-black mix-blend-difference transition-[opacity,scale] duration-150 ease-out ${isCursorVisible ? "scale-100 opacity-100" : "scale-0 opacity-0"}`}
							>
								{cursorMode === "nav-left" ? (
									<ArrowLeft className="size-6" />
								) : null}
								{cursorMode === "nav-right" ? (
									<ArrowRight className="size-6" />
								) : null}
								{cursorMode === "select" ? (
									<MousePointerClick className="size-6" />
									) : null}
								{cursorMode === "drag" ? (
									<Hand className="size-6" />
								) : null}
							</div>
						</div>,
						document.body,
					)
				: null}
		</section>
	);
}

export default FeaturesSection;
