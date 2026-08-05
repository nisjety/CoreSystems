"use client";

import { useEffect, useRef, useState } from "react";
import {
	VerevonComposerPreview,
	type VerevonComposerMode,
} from "@/components/ui/VerevonComposerPreview";

const composerModes: Array<{
	label: string;
	mode: VerevonComposerMode;
	prompt: string;
}> = [
	{
		label: "Chat",
		mode: "chat",
		prompt: "Spør om hva som helst, bruk / for spesialhandlinger.",
	},
	{
		label: "Crawl",
		mode: "crawl",
		prompt: "Lim inn en nettside for å crawle.",
	},
	{
		label: "Søk",
		mode: "search",
		prompt: "Søk eller spør ...",
	},
];

type FeatureComposerCycleProps = {
	id: string;
	onModeChange?: (mode: VerevonComposerMode) => void;
};

/**
 * A single composer that cycles through Verevon's three entry points.
 * Keeping one preview mounted makes the loop lightweight and lets the
 * surrounding GSAP scene continue targeting one send state and one prompt.
 */
export function FeatureComposerCycle({
	id,
	onModeChange,
}: FeatureComposerCycleProps) {
	const rootRef = useRef<HTMLDivElement>(null);
	const [isVisible, setIsVisible] = useState(false);
	const [modeIndex, setModeIndex] = useState(0);
	const [isSwitching, setIsSwitching] = useState(false);
	const [reduceMotion, setReduceMotion] = useState(false);

	useEffect(() => {
		const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
		const updateMotionPreference = () => setReduceMotion(mediaQuery.matches);

		updateMotionPreference();
		mediaQuery.addEventListener("change", updateMotionPreference);

		return () => mediaQuery.removeEventListener("change", updateMotionPreference);
	}, []);

	useEffect(() => {
		const root = rootRef.current;

		if (!root || typeof IntersectionObserver === "undefined") {
			setIsVisible(true);
			return;
		}

		const observer = new IntersectionObserver(
			([entry]) => setIsVisible(entry.isIntersecting),
			{ threshold: 0.1 },
		);

		observer.observe(root);
		return () => observer.disconnect();
	}, []);

	useEffect(() => {
		if (!isVisible || reduceMotion) {
			return;
		}

		let switchTimer: number | undefined;
		let nextTimer: number | undefined;

		const scheduleNext = () => {
			nextTimer = window.setTimeout(() => {
				setIsSwitching(true);
				switchTimer = window.setTimeout(() => {
					setModeIndex((currentIndex) => (currentIndex + 1) % composerModes.length);
					setIsSwitching(false);
					scheduleNext();
				}, 260);
			}, 3200);
		};

		scheduleNext();

		return () => {
			if (nextTimer !== undefined) {
				window.clearTimeout(nextTimer);
			}
			if (switchTimer !== undefined) {
				window.clearTimeout(switchTimer);
			}
		};
	}, [isVisible, reduceMotion]);

	const activeMode = composerModes[modeIndex] ?? composerModes[0];

	useEffect(() => {
		onModeChange?.(activeMode.mode);
	}, [activeMode.mode, onModeChange]);

	return (
		<div
			aria-hidden="true"
			className="w-full"
			data-feature-composer-cycle=""
			data-feature-composer-mode={activeMode.mode}
			ref={rootRef}
		>
			<div
				className={`transition-[opacity,transform] duration-[260ms] ease-out ${isSwitching ? "scale-[0.995] opacity-45" : "scale-100 opacity-100"}`}
			>
				<VerevonComposerPreview
					id={id}
					mode={activeMode.mode}
					prompt={activeMode.prompt}
				/>
			</div>

			<div
				aria-hidden="true"
				className="mt-3 flex items-center justify-center gap-2 font-protokoll text-[10px] font-medium uppercase tracking-[0.2em] text-verevon-j-text/44"
			>
				{composerModes.map((composerMode) => (
					<span
						className={`inline-flex items-center gap-1.5 transition-colors duration-300 ${composerMode.mode === activeMode.mode ? "text-verevon-j-text/82" : "text-verevon-j-text/32"}`}
						key={composerMode.mode}
					>
						<span
							className={`size-1.5 rounded-full transition-colors duration-300 ${composerMode.mode === activeMode.mode ? "bg-verevon-coral" : "bg-verevon-j-text/18"}`}
						/>
						{composerMode.label}
					</span>
				))}
			</div>
		</div>
	);
}
