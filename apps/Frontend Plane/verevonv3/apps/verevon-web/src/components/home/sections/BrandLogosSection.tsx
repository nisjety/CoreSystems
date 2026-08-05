"use client";

import { animate, motion, useMotionValue, useReducedMotion } from "motion/react";
import type { CSSProperties, ReactNode } from "react";
import { useEffect, useState } from "react";
import useMeasure from "react-use-measure";

type InfiniteSliderProps = {
	children: ReactNode;
	gap?: number;
	speed?: number;
	speedOnHover?: number;
	direction?: "horizontal" | "vertical";
	reverse?: boolean;
	className?: string;
};

function InfiniteSlider({
	children,
	gap = 16,
	speed = 100,
	speedOnHover,
	direction = "horizontal",
	reverse = false,
	className,
}: InfiniteSliderProps) {
	const [currentSpeed, setCurrentSpeed] = useState(speed);
	const [ref, { width, height }] = useMeasure();
	const translation = useMotionValue(0);
	const [isTransitioning, setIsTransitioning] = useState(false);
	const [key, setKey] = useState(0);
	const shouldReduceMotion = useReducedMotion();

	useEffect(() => {
		const size = direction === "horizontal" ? width : height;

		if (size === 0 || shouldReduceMotion) {
			translation.set(0);
			return undefined;
		}

		const contentSize = size + gap;
		const from = reverse ? -contentSize / 2 : 0;
		const to = reverse ? 0 : -contentSize / 2;
		const distanceToTravel = Math.abs(to - from);
		const duration = distanceToTravel / currentSpeed;

		const controls = isTransitioning
			? animate(translation, [translation.get(), to], {
					duration: Math.abs(translation.get() - to) / currentSpeed,
					ease: "linear",
					onComplete: () => {
						setIsTransitioning(false);
						setKey((prevKey) => prevKey + 1);
					},
				})
			: animate(translation, [from, to], {
					duration,
					ease: "linear",
					repeat: Infinity,
					repeatDelay: 0,
					repeatType: "loop",
					onRepeat: () => {
						translation.set(from);
					},
				});

		return () => controls.stop();
	}, [
		key,
		translation,
		currentSpeed,
		width,
		height,
		gap,
		isTransitioning,
		direction,
		reverse,
		shouldReduceMotion,
	]);

	const handleHoverStart = () => {
		if (!speedOnHover || shouldReduceMotion) return;

		setIsTransitioning(true);
		setCurrentSpeed(speedOnHover);
	};

	const handleHoverEnd = () => {
		if (!speedOnHover || shouldReduceMotion) return;

		setIsTransitioning(true);
		setCurrentSpeed(speed);
	};

	return (
		<div className={className}>
			<motion.div
				className={[
					"flex w-max items-center",
					shouldReduceMotion ? "" : "will-change-transform",
				]
					.filter(Boolean)
					.join(" ")}
				onHoverEnd={handleHoverEnd}
				onHoverStart={handleHoverStart}
				ref={ref}
				style={{
					...(direction === "horizontal" ? { x: translation } : { y: translation }),
					flexDirection: direction === "horizontal" ? "row" : "column",
					gap: `${gap}px`,
				}}
			>
				{children}
				{children}
			</motion.div>
		</div>
	);
}

type BlurredInfiniteSliderProps = InfiniteSliderProps & {
	containerClassName?: string;
	fadeWidth?: number;
};

function BlurredInfiniteSlider({
	children,
	containerClassName,
	fadeWidth = 80,
	...sliderProps
}: BlurredInfiniteSliderProps) {
	const maskStyle: CSSProperties = {
		WebkitMaskImage: `linear-gradient(to right, transparent, black ${fadeWidth}px, black calc(100% - ${fadeWidth}px), transparent)`,
		maskImage: `linear-gradient(to right, transparent, black ${fadeWidth}px, black calc(100% - ${fadeWidth}px), transparent)`,
	};

	return (
		<div className={containerClassName} style={maskStyle}>
			<InfiniteSlider {...sliderProps}>{children}</InfiniteSlider>
		</div>
	);
}

// Curated to the genuinely-live connector + grounding set (see VEREVON.md):
// Brreg/Enhetsregisteret lookup is live; Slack/Gmail/Notion/SharePoint/
// OneDrive/Outlook are live connectors. Roadmap sources (e.g. Zendesk) are
// deliberately omitted — the honesty gate applies to logos too.
const logoMarks = [
	"Enhetsregisteret",
	"Slack",
	"Gmail",
	"Notion",
	"SharePoint",
	"OneDrive",
	"Outlook",
	"Microsoft 365",
];

function getLogoClassName(index: number) {
	if ((index + 1) % 5 === 0) {
		return "font-arbeit text-[clamp(1rem,1.55vw,1.48rem)] font-medium tracking-[-0.01em]";
	}

	if ((index + 1) % 3 === 0) {
		return "font-arbeit text-[clamp(1.12rem,1.45vw,1.7rem)] font-light tracking-[0.02em]";
	}

	if ((index + 1) % 2 === 0) {
		return "font-arbeit text-[clamp(0.92rem,1.35vw,1.28rem)] font-medium uppercase tracking-[0.025em]";
	}

	return "font-arbeit text-[clamp(1.12rem,1.45vw,1.7rem)] font-medium tracking-[-0.045em]";
}

export function BrandLogosSection() {
	return (
		<section
			aria-label="Systemer Verevon kobler til"
			className="grid h-[clamp(112px,12vw,154px)] grid-cols-[minmax(138px,220px)_minmax(0,1fr)] items-center gap-[clamp(34px,4.8vw,92px)] overflow-hidden border-b border-[color-mix(in_srgb,var(--verevon-j-text)_7%,transparent)] bg-background px-[clamp(56px,5.55vw,208px)] max-[760px]:h-auto max-[760px]:grid-cols-1 max-[760px]:gap-4 max-[760px]:px-[clamp(24px,4vw,56px)] max-[760px]:py-7"
		>
			<p className="m-0 grid justify-self-start border-r border-[color-mix(in_srgb,var(--verevon-j-text)_10%,transparent)] pr-[clamp(18px,2vw,34px)] text-left font-protokoll text-[clamp(0.78rem,0.72vw,0.92rem)] font-medium uppercase leading-[1.12] tracking-[0.025em] text-[color-mix(in_srgb,var(--verevon-j-text)_58%,transparent)] max-[760px]:flex max-[760px]:gap-1.5 max-[760px]:border-r-0 max-[760px]:pr-0">
				<span>Koblet til</span>
				<span>systemene deres</span>
			</p>

			<BlurredInfiniteSlider
				containerClassName="flex h-full min-w-0 items-center overflow-hidden max-[760px]:h-12"
				fadeWidth={80}
				gap={112}
				speed={40}
				speedOnHover={20}
			>
				{logoMarks.map((logo, index) => (
					<span
						className={[
							"inline-flex min-w-max flex-none items-center justify-center leading-none opacity-85 saturate-[0.2] text-[color-mix(in_srgb,var(--verevon-j-text)_62%,transparent)]",
							getLogoClassName(index),
						].join(" ")}
						key={`${logo}-${index}`}
					>
						{logo}
					</span>
				))}
			</BlurredInfiniteSlider>
		</section>
	);
}

export default BrandLogosSection;
