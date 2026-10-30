"use client";

import Image from "next/image";
import { animate, motion, useMotionValue, useReducedMotion } from "motion/react";
import type { CSSProperties, ReactNode } from "react";
import { useEffect, useState } from "react";
import useMeasure from "react-use-measure";
import { usePrefersReducedMotion } from "@/shared/hooks/usePrefersReducedMotion";
import { LIVE_CONNECTORS } from "./brand-connector-data";

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
				<span aria-hidden="true" className="contents">{children}</span>
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

export function BrandLogosSection() {
	const prefersReducedMotion = usePrefersReducedMotion();
	const marks = LIVE_CONNECTORS.map((connector) => (
		<span
			className="inline-flex min-w-max flex-none items-center gap-3 font-manrope text-[clamp(1rem,1.3vw,1.3rem)] font-medium leading-none tracking-[-0.035em] text-[color-mix(in_srgb,var(--verevon-j-text)_62%,transparent)]"
			key={connector.name}
		>
			{connector.logo ? (
				<Image alt="" className="size-[clamp(26px,2.3vw,36px)] object-contain grayscale opacity-65" height={36} src={`/brand-logos/${connector.logo}`} width={36} />
			) : null}
			{connector.name}
		</span>
	));

	return (
		<section
			aria-label="Systemer Verevon kobler til"
			className="grid h-[clamp(112px,12vw,154px)] grid-cols-[minmax(138px,220px)_minmax(0,1fr)] items-center gap-[clamp(34px,4.8vw,92px)] overflow-hidden bg-background px-[var(--verevon-edge)] max-[760px]:h-auto max-[760px]:grid-cols-1 max-[760px]:gap-4 max-[760px]:px-[var(--verevon-page-pad)] max-[760px]:py-7"
			style={prefersReducedMotion ? { height: "auto", minHeight: 112, paddingBlock: 24 } : undefined}
		>
			<p className="m-0 grid justify-self-start border-r border-[color-mix(in_srgb,var(--verevon-j-text)_10%,transparent)] pr-[clamp(18px,2vw,34px)] text-left font-geist text-[clamp(0.78rem,0.72vw,0.92rem)] font-medium uppercase leading-[1.12] tracking-[0.025em] text-[color-mix(in_srgb,var(--verevon-j-text)_58%,transparent)] max-[760px]:flex max-[760px]:gap-1.5 max-[760px]:border-r-0 max-[760px]:pr-0">
				<span>Koblet til</span>
				<span>systemene deres</span>
			</p>

			{prefersReducedMotion ? (
				<div className="flex flex-wrap items-center gap-x-10 gap-y-5 py-2">
					{marks}
				</div>
			) : (
				<BlurredInfiniteSlider
					containerClassName="flex h-full min-w-0 items-center overflow-hidden max-[760px]:h-12"
					fadeWidth={80}
					gap={88}
					speed={36}
					speedOnHover={14}
				>
					{marks}
				</BlurredInfiniteSlider>
			)}
		</section>
	);
}

export default BrandLogosSection;
