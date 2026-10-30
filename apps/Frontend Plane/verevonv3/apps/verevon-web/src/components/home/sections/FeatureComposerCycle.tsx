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
		label: "Spør",
		mode: "chat",
		prompt: "Spør om hva som helst",
	},
	{
		label: "Hent",
		mode: "crawl",
		prompt: "Lim inn en nettside for å hente innhold.",
	},
	{
		label: "Søk",
		mode: "search",
		prompt: "Søk på hva som helst",
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
	const manualFadeTimerRef = useRef<number | undefined>(undefined);
	const [isVisible, setIsVisible] = useState(false);
	const [modeIndex, setModeIndex] = useState(0);
	const [isSwitching, setIsSwitching] = useState(false);
	const [reduceMotion, setReduceMotion] = useState(false);
	const [autoplayPaused, setAutoplayPaused] = useState(false);

	useEffect(() => {
		const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
		const updateMotionPreference = () => {
			setReduceMotion(mediaQuery.matches);
			if (mediaQuery.matches) setIsSwitching(false);
		};

		updateMotionPreference();
		mediaQuery.addEventListener("change", updateMotionPreference);

		return () => mediaQuery.removeEventListener("change", updateMotionPreference);
	}, []);

	useEffect(() => {
		return () => {
			if (manualFadeTimerRef.current !== undefined) {
				window.clearTimeout(manualFadeTimerRef.current);
			}
		};
	}, []);

	useEffect(() => {
		const root = rootRef.current;

		if (!root || typeof IntersectionObserver === "undefined") {
			setIsVisible(true);
			return;
		}

		let intersecting = false;
		const layer = root.closest<HTMLElement>("[data-product-loop-layer]");
		const update = () => {
			const visible = intersecting && !document.hidden && !layer?.inert;
			setIsVisible(visible);
			if (!visible) setIsSwitching(false);
		};
		const observer = new IntersectionObserver(
			([entry]) => { intersecting = entry.isIntersecting; update(); },
			{ threshold: 0.1 },
		);

		observer.observe(root);
		const layerObserver = new MutationObserver(update);
		if (layer) layerObserver.observe(layer, { attributes: true, attributeFilter: ["inert"] });
		document.addEventListener("visibilitychange", update);
		return () => {
			observer.disconnect();
			layerObserver.disconnect();
			document.removeEventListener("visibilitychange", update);
		};
	}, []);

	useEffect(() => {
		if (!isVisible || reduceMotion || autoplayPaused) {
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
			}, 4000);
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
	}, [autoplayPaused, isVisible, reduceMotion]);

	const activeMode = composerModes[modeIndex] ?? composerModes[0];

	useEffect(() => {
		onModeChange?.(activeMode.mode);
	}, [activeMode.mode, onModeChange]);

	const selectMode = (nextModeIndex: number) => {
		if (manualFadeTimerRef.current !== undefined) {
			window.clearTimeout(manualFadeTimerRef.current);
		}

		const changesMode = nextModeIndex !== modeIndex;
		setAutoplayPaused(true);
		if (changesMode) {
			setModeIndex(nextModeIndex);
		}

		if (!changesMode || reduceMotion) {
			setIsSwitching(false);
			return;
		}

		setIsSwitching(true);
		manualFadeTimerRef.current = window.setTimeout(() => {
			setIsSwitching(false);
		}, 220);
	};

	return (
		<div
			className="w-full"
			data-feature-composer-cycle=""
			data-feature-composer-mode={activeMode.mode}
			ref={rootRef}
		>
			<div
				className={`transition-[opacity,transform] duration-[260ms] ease-out ${isSwitching ? "scale-[0.995] opacity-45" : "scale-100 opacity-100"}`}
			>
				<VerevonComposerPreview
					ambientMotion
					id={id}
					mode={activeMode.mode}
					prompt={activeMode.prompt}
				/>
			</div>

			<div
				aria-label="Velg inngang"
				className="mt-3 flex items-center justify-center gap-1 font-geist text-[10px] font-medium uppercase tracking-[0.2em]"
				data-composer-switcher
				role="group"
			>
				{composerModes.map((composerMode, index) => {
					const isActive = composerMode.mode === activeMode.mode;

					return (
						<button
							aria-pressed={isActive}
							className={`inline-flex min-h-7 items-center gap-1.5 rounded-full px-1.5 transition-[color,background-color] duration-200 focus-visible:bg-verevon-j-text/6 focus-visible:text-verevon-j-text focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-verevon-coral/70 ${isActive ? "text-verevon-j-text/82" : "text-verevon-j-text/32 hover:bg-verevon-j-text/[0.035] hover:text-verevon-j-text/65"}`}
							data-composer-mode-switch={composerMode.mode}
							key={composerMode.mode}
							onClick={() => selectMode(index)}
							type="button"
						>
							<span
								aria-hidden="true"
								className={`size-1.5 rounded-full transition-colors duration-200 ${isActive ? "bg-verevon-coral" : "bg-verevon-j-text/18"}`}
							/>
							{composerMode.label}
						</button>
					);
				})}
			</div>
		</div>
	);
}
