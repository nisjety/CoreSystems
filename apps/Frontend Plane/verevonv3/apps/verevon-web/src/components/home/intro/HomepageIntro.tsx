"use client";

import { useEffect, useRef, useState } from "react";
import {
	VEREVON_MARK_LEFT_V_PATH,
	VEREVON_MARK_RIGHT_WING_PATH,
} from "@/components/home/sections/VerevonMark";
import { REVEAL_HOLD_MS, REVEAL_SETTLED_MS } from "@/components/dev/logo-motion-studies/reveal-motion";
import { createReverseRenderer } from "@/components/dev/logo-motion-studies/reverse-renderer";
import styles from "./HomepageIntro.module.css";

export const HOMEPAGE_INTRO_SESSION_KEY = "verevon.home-intro-seen-v1";

type IntroDecision = "pending" | "play" | "skip";
type IntroPhase = "checking" | "playing" | "complete";
type Props = {
	onComplete: () => void;
	playback?: "session" | "preview";
};

// ── Quick tuning ──────────────────────────────────────────────────────────
// Change the values in this block to retime or resize the intro. All times use
// one shared clock in milliseconds, so they can be read directly as seconds.

/** The permanent wordmark. Keep this in sync with the final header label. */
const WORDMARK = "VEREVON";

/** Maximum emphasis size while the completed lockup is held (135%). */
const LOCKUP_PAUSE_SCALE = 1.35;
/**
 * Matches Original B in the motion study: `width: min(78%, 355px)`.
 * Change these two values together only if the approved B reference changes.
 */
const ORIGINAL_B_MARK_VIEWPORT_WIDTH_RATIO = 0.78;
const ORIGINAL_B_MARK_MAX_WIDTH = 355;
/** SVG viewBox dimensions, also used for the high-resolution ribbon buffer. */
const MARK_VIEWBOX_WIDTH = 82;
const MARK_VIEWBOX_HEIGHT = 68;
/**
 * Exact settled geometry from the approved "Merke + navn" study:
 * the V is moved -36.5% of its own width and scaled to 27%, while the
 * word begins at 30% of the same viewport and is set in 12cqw.
 */
const LOCKUP_MARK_FINAL_SCALE = 0.27;
const LOCKUP_MARK_FINAL_X_PERCENT = -36.5;
const LOCKUP_WORDMARK_LEFT_PERCENT = 30;
const LOCKUP_WORDMARK_SIZE_CQW = 12;
/** A safe header text size if computed styles are unavailable. */
const FALLBACK_HEADER_WORDMARK_FONT_SIZE = 15.68;

/** 2.50–3.50 s: the V takes its approved lockup pose and letters roll in. */
export const HOMEPAGE_WORDMARK_START_MS = 2_500;
export const HOMEPAGE_WORDMARK_END_MS = 3_500;
/** Derived aliases: keep these aligned with the wordmark interval. */
export const HOMEPAGE_MARK_SHRINK_START_MS = HOMEPAGE_WORDMARK_START_MS;
export const HOMEPAGE_MARK_SHRINK_END_MS = HOMEPAGE_WORDMARK_END_MS;
/** Per-letter overlap. The seventh letter finishes exactly at WORDMARK_END_MS. */
const WORDMARK_LETTER_STAGGER_MS = 80;
const WORDMARK_LETTER_DURATION_MS = 520;

/** 3.00–3.30 s: the full lockup grows to LOCKUP_PAUSE_SCALE. */
export const HOMEPAGE_LOCKUP_SCALE_START_MS = 3_000;
export const HOMEPAGE_LOCKUP_SCALE_END_MS = 3_300;
/** The centered lockup is complete when the final wordmark letter settles. */
export const HOMEPAGE_LOCKUP_LANDS_MS = HOMEPAGE_WORDMARK_END_MS;
/** 3.30–4.30 s: intentional one-second hold before the header journey. */
/** 4.30–4.70 s: travel to the header while returning to normal size. */
export const HOMEPAGE_LOCKUP_RISE_START_MS = 4_300;
export const HOMEPAGE_LOCKUP_RISE_END_MS = 4_700;
/** 4.70–5.00 s: only the V fades; VEREVON remains as the header wordmark. */
export const HOMEPAGE_MARK_FADE_START_MS = HOMEPAGE_LOCKUP_RISE_END_MS;
export const HOMEPAGE_MARK_FADE_END_MS = 5_000;
/** Total duration. Reduced motion and fallback paths jump straight here. */
export const HOMEPAGE_INTRO_DURATION_MS = HOMEPAGE_MARK_FADE_END_MS;
/** Derived header-travel interval; edit the exported rise values above instead. */
const LANDING_START_MS = HOMEPAGE_LOCKUP_RISE_START_MS;
const LANDING_END_MS = HOMEPAGE_LOCKUP_RISE_END_MS;

function easedProgress(timeMs: number, start: number, end: number) {
	const value = Math.max(0, Math.min(1, (timeMs - start) / (end - start)));
	return value ** 3 * (value * (value * 6 - 15) + 10);
}

function wordmarkLetterPose(index: number, timeMs: number) {
	const progress = easedProgress(
		timeMs,
		HOMEPAGE_WORDMARK_START_MS + index * WORDMARK_LETTER_STAGGER_MS,
		HOMEPAGE_WORDMARK_START_MS + index * WORDMARK_LETTER_STAGGER_MS + WORDMARK_LETTER_DURATION_MS,
	);

	return {
		angle: progress === 1 ? 0 : -90 * (1 - progress),
		opacity: progress === 0 ? 0 : 1,
		yEm: 1.12 * (1 - progress),
	};
}

function lockupScaleAt(timeMs: number) {
	// The scale reaches 135% before the hold, stays there, then returns to 100%
	// only while the lockup travels to the permanent header position.
	const scaleUp = easedProgress(timeMs, HOMEPAGE_LOCKUP_SCALE_START_MS, HOMEPAGE_LOCKUP_SCALE_END_MS);
	const scaleDown = easedProgress(timeMs, HOMEPAGE_LOCKUP_RISE_START_MS, HOMEPAGE_LOCKUP_RISE_END_MS);
	return 1 + (LOCKUP_PAUSE_SCALE - 1) * (scaleUp - scaleDown);
}

function interpolate(from: number, to: number, progress: number) {
	return from + (to - from) * progress;
}

function colorAt(progress: number) {
	const mix = (from: number, to: number) => Math.round(from + (to - from) * progress);
	return `rgb(${mix(23, 247)} ${mix(23, 244)} ${mix(23, 238)})`;
}

function readDecision() {
	try {
		if (window.sessionStorage.getItem(HOMEPAGE_INTRO_SESSION_KEY) === "1") {
			return "skip" as const;
		}
		window.sessionStorage.setItem(HOMEPAGE_INTRO_SESSION_KEY, "1");
		return "play" as const;
	} catch {
		// If private browsing or an embedded browser blocks session storage, the
		// homepage must remain usable instead of presenting a permanent overlay.
		return "skip" as const;
	}
}

export function HomepageIntro({ onComplete, playback = "session" }: Props) {
	const rootRef = useRef<HTMLDivElement>(null);
	const frameRef = useRef<HTMLDivElement>(null);
	const brandRef = useRef<HTMLDivElement>(null);
	const markRef = useRef<SVGSVGElement>(null);
	const sourceCanvasRef = useRef<HTMLCanvasElement>(null);
	const displayCanvasRef = useRef<HTMLCanvasElement>(null);
	const canonicalRef = useRef<SVGGElement>(null);
	const surfaceRef = useRef<SVGForeignObjectElement>(null);
	const wordmarkRef = useRef<HTMLSpanElement>(null);
	const lettersRef = useRef<HTMLSpanElement[]>([]);
	const decisionRef = useRef<IntroDecision>("pending");
	const finishedRef = useRef(false);
	const onCompleteRef = useRef(onComplete);
	const [phase, setPhase] = useState<IntroPhase>("checking");

	useEffect(() => {
		onCompleteRef.current = onComplete;
	}, [onComplete]);

	useEffect(() => {
		const root = rootRef.current;
		const frame = frameRef.current;
		const brand = brandRef.current;
		const mark = markRef.current;
		const sourceCanvas = sourceCanvasRef.current;
		const displayCanvas = displayCanvasRef.current;
		const canonical = canonicalRef.current;
		const surface = surfaceRef.current;
		const wordmark = wordmarkRef.current;
		if (!root || !frame || !brand || !mark || !sourceCanvas || !displayCanvas || !canonical || !surface || !wordmark) return;
		const introRoot = root;
		const brandFrame = frame;
		const brandLockup = brand;
		const logoMark = mark;
		const renderSource = sourceCanvas;
		const renderDisplay = displayCanvas;
		const staticMark = canonical;
		const ribbonSurface = surface;
		const animatedWordmark = wordmark;
		const headerTarget = document.querySelector<HTMLElement>("[data-site-brand-target]");
		const headerWordmarkFontSize = headerTarget
			? Number.parseFloat(getComputedStyle(headerTarget).fontSize) || FALLBACK_HEADER_WORDMARK_FONT_SIZE
			: FALLBACK_HEADER_WORDMARK_FONT_SIZE;
		// 12cqw at this width equals the permanent header's wordmark size. The
		// lockup can therefore scale as one composition during the handoff.
		const headerViewportWidth = headerWordmarkFontSize / (LOCKUP_WORDMARK_SIZE_CQW / 100);

		if (decisionRef.current === "pending") {
			const nativeReducedMotion = getComputedStyle(introRoot)
				.getPropertyValue("--home-intro-motion")
				.trim() === "reduce";
			// Record a reduced-motion visit as handled as well. Otherwise a person who
			// changes their preference during this session would be left with an
			// invisible, but still blocking, replay on the next mount.
			const storageDecision = playback === "preview" ? "play" : readDecision();
			decisionRef.current = nativeReducedMotion ? "skip" : storageDecision;
		}

		let animationFrame: number | null = null;
		let renderer: ReturnType<typeof createReverseRenderer> = null;
		let previous: number | null = null;
		let elapsed = 0;
		let disposed = false;
		let preferenceCheck: number | undefined;
		let resizeAttached = false;
		const displayContext = renderDisplay.getContext("2d");

		const copyRibbonFrame = () => {
			if (!displayContext || renderSource.width === 0 || renderSource.height === 0) return;
			if (renderDisplay.width !== renderSource.width || renderDisplay.height !== renderSource.height) {
				renderDisplay.width = renderSource.width;
				renderDisplay.height = renderSource.height;
			}
			displayContext.clearRect(0, 0, renderDisplay.width, renderDisplay.height);
			displayContext.drawImage(renderSource, 0, 0, renderDisplay.width, renderDisplay.height);
		};

		const syncRibbonRasterSize = (visibleMarkWidth: number) => {
			// The WebGL canvas is deliberately offscreen, while its painted copy fills
			// the SVG foreignObject. It must still use the visible V's footprint: an
			// 82×68 canvas scaled to 355px makes the moving ribbon visibly pixelated.
			const width = Math.max(1, Math.ceil(visibleMarkWidth));
			const height = Math.max(1, Math.ceil(width * MARK_VIEWBOX_HEIGHT / MARK_VIEWBOX_WIDTH));
			const widthValue = `${width}px`;
			const heightValue = `${height}px`;
			if (renderSource.style.width !== widthValue) renderSource.style.width = widthValue;
			if (renderSource.style.height !== heightValue) renderSource.style.height = heightValue;
		};

		function paint(timeMs: number) {
			const markProgress = easedProgress(timeMs, HOMEPAGE_MARK_SHRINK_START_MS, HOMEPAGE_MARK_SHRINK_END_MS);
			const landingProgress = easedProgress(timeMs, LANDING_START_MS, LANDING_END_MS);
			const referenceViewportWidth = Math.min(
				window.innerWidth * ORIGINAL_B_MARK_VIEWPORT_WIDTH_RATIO,
				ORIGINAL_B_MARK_MAX_WIDTH,
			);
			const lockupWidth = interpolate(referenceViewportWidth, headerViewportWidth, landingProgress);
			const markScale = interpolate(1, LOCKUP_MARK_FINAL_SCALE, markProgress);
			const markX = interpolate(0, LOCKUP_MARK_FINAL_X_PERCENT, markProgress);

			// The renderer additionally applies device-pixel-ratio resolution. This
			// gives the ribbon a full-size source buffer before the SVG presents it.
			syncRibbonRasterSize(referenceViewportWidth);
			const ribbonIsMoving = timeMs > REVEAL_HOLD_MS && timeMs < REVEAL_SETTLED_MS;
			staticMark.style.display = ribbonIsMoving ? "none" : "inline";
			ribbonSurface.style.display = ribbonIsMoving ? "inline" : "none";
			if (ribbonIsMoving) {
				renderer?.draw(timeMs);
				copyRibbonFrame();
			}

			// Build from the same coordinate system as "Merke + navn". This is not
			// an optical approximation: its V scale, word position, word size and
			// letter spacing are shared with the approved comparison study.
			brandLockup.style.width = `${lockupWidth}px`;
			logoMark.style.transform = `translateY(-50%) translateX(${markX}%) scale(${markScale})`;
			// The wordmark's position and font size live in CSS as 30% / 12cqw so
			// they scale from this exact study proportion all the way to the header.
			animatedWordmark.style.transform = "translateY(-50%)";

			lettersRef.current.forEach((letter, index) => {
				const pose = wordmarkLetterPose(index, timeMs);
				letter.style.transform = `translateY(${pose.yEm}em) rotateX(${pose.angle}deg)`;
				letter.style.opacity = String(pose.opacity);
			});

			// In the final 300 ms the full lockup is already in header position. Move
			// the wordmark into its text-only target as the V fades away.
			const markFadeProgress = easedProgress(timeMs, HOMEPAGE_MARK_FADE_START_MS, HOMEPAGE_MARK_FADE_END_MS);
			const target = headerTarget;
			if (target) {
				const rect = target.getBoundingClientRect();
				const left = (window.innerWidth - brandFrame.offsetWidth) / 2;
				const top = (window.innerHeight - brandFrame.offsetHeight) / 2;
				introRoot.style.setProperty("--home-intro-x", `${(rect.left - left) * landingProgress}px`);
				introRoot.style.setProperty("--home-intro-y", `${(rect.top - top) * landingProgress}px`);
			}
			introRoot.style.setProperty("--home-intro-background-opacity", String(1 - landingProgress));
			introRoot.style.setProperty("--home-intro-scale", String(lockupScaleAt(timeMs)));
			brandLockup.style.color = colorAt(landingProgress);
			brandLockup.style.opacity = String(easedProgress(timeMs, REVEAL_HOLD_MS, 520));
			brandLockup.style.transform = `translateX(${-1 * lockupWidth * (LOCKUP_WORDMARK_LEFT_PERCENT / 100) * markFadeProgress}px)`;
			logoMark.style.opacity = String(1 - markFadeProgress);
			introRoot.dataset.time = String(Math.round(timeMs));
		}

		const repaint = () => {
			if (disposed || finishedRef.current) return;
			paint(elapsed);
		};
		const stopResources = () => {
			if (animationFrame !== null) cancelAnimationFrame(animationFrame);
			animationFrame = null;
			if (preferenceCheck !== undefined) window.clearInterval(preferenceCheck);
			preferenceCheck = undefined;
			if (resizeAttached) window.removeEventListener("resize", repaint);
			resizeAttached = false;
			renderer?.dispose();
			renderer = null;
		};

		const finish = () => {
			if (finishedRef.current) return;
			finishedRef.current = true;
			document.documentElement.dataset.homeIntroSession = "seen";
			stopResources();
			setPhase("complete");
			onCompleteRef.current();
		};

		if (decisionRef.current === "skip") {
			finish();
			return;
		}

		const tick = (timestamp: number) => {
			animationFrame = null;
			if (getComputedStyle(introRoot).getPropertyValue("--home-intro-motion").trim() === "reduce") {
				elapsed = HOMEPAGE_INTRO_DURATION_MS;
				paint(elapsed);
				finish();
				return;
			}
			if (previous !== null) {
				elapsed = Math.min(HOMEPAGE_INTRO_DURATION_MS, elapsed + Math.max(0, timestamp - previous));
			}
			previous = timestamp;
			paint(elapsed);
			if (elapsed >= HOMEPAGE_INTRO_DURATION_MS) {
				finish();
				return;
			}
			animationFrame = requestAnimationFrame(tick);
		};

		setPhase("playing");
		renderer = createReverseRenderer(renderSource, getComputedStyle(introRoot).color, () => {
			if (disposed) return;
			elapsed = HOMEPAGE_INTRO_DURATION_MS;
			paint(elapsed);
			finish();
		});
		if (!renderer) {
			elapsed = HOMEPAGE_INTRO_DURATION_MS;
			paint(elapsed);
			finish();
			return;
		}

		renderer.draw(0);
		copyRibbonFrame();
		paint(0);
		window.addEventListener("resize", repaint);
		resizeAttached = true;
		void document.fonts?.ready.then(repaint);
		preferenceCheck = window.setInterval(() => {
			if (getComputedStyle(introRoot).getPropertyValue("--home-intro-motion").trim() !== "reduce") return;
			elapsed = HOMEPAGE_INTRO_DURATION_MS;
			paint(elapsed);
			finish();
		}, 100);
		animationFrame = requestAnimationFrame(tick);

		return () => {
			disposed = true;
			stopResources();
		};
	}, [playback]);

	return (
		<div
			aria-label="Verevon laster"
			aria-live="polite"
			className={styles.intro}
			data-home-intro=""
			data-home-intro-preview={playback === "preview" || undefined}
			data-phase={phase}
			ref={rootRef}
			role="status"
		>
			<div className={styles.brandFrame} data-intro-frame="" ref={frameRef}>
				<div className={styles.brand} data-intro-brand="" ref={brandRef}>
					<svg
						aria-hidden="true"
						className={styles.mark}
						fill="currentColor"
						ref={markRef}
						viewBox="20 26 82 68"
					>
						<g data-intro-canonical="" ref={canonicalRef}>
							<path d={VEREVON_MARK_LEFT_V_PATH} />
							<path d={VEREVON_MARK_RIGHT_WING_PATH} />
						</g>
						<foreignObject height="68" ref={surfaceRef} style={{ display: "none" }} width="82" x="20" y="26">
							<canvas aria-hidden="true" className={styles.canvas} ref={displayCanvasRef} />
						</foreignObject>
					</svg>
					<span aria-hidden="true" className={styles.wordmark} ref={wordmarkRef}>
						{Array.from(WORDMARK, (letter, index) => (
							<span className={styles.letterWindow} key={index}>
								<span
									className={styles.letter}
									data-intro-letter={index}
									ref={(node) => {
										if (node) lettersRef.current[index] = node;
									}}
								>
									{letter}
								</span>
							</span>
						))}
					</span>
				</div>
			</div>
			<canvas aria-hidden="true" className={styles.renderSource} data-intro-render-source="" ref={sourceCanvasRef} />
		</div>
	);
}
