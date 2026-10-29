"use client";

import { useEffect, useRef } from "react";
import { REST_PATHS } from "./flag-motion";
import { REVEAL_DURATION_MS, REVEAL_HOLD_MS, REVEAL_SETTLED_MS } from "./reveal-motion";
import { createRibbonRenderer, type RibbonRenderer } from "./ribbon-renderer";
import { createReverseRenderer } from "./reverse-renderer";
import styles from "./LogoMotionStudies.module.css";

type Props = { paused: boolean; speed?: number; seekTime?: number };
type Layer = { renderer: RibbonRenderer | null; failed: boolean };
const VARIANTS = [
	{ id: "a", label: "A · Originalen", detail: "Forrige versjon, beholdt uendret.", from: REVEAL_HOLD_MS, until: REVEAL_SETTLED_MS, create: createRibbonRenderer },
	{ id: "b", label: "B · Mot klokken", detail: "Samme original. Bare rotasjonsretningen er snudd.", from: REVEAL_HOLD_MS, until: REVEAL_SETTLED_MS, create: createReverseRenderer },
] as const;

function captureTime() {
	const query = new URLSearchParams(window.location.search);
	if (query.get("static") === "1") return REVEAL_DURATION_MS;
	const time = query.has("t") ? Number(query.get("t")) : undefined;
	return time !== undefined && Number.isFinite(time) ? Math.max(0, Math.min(REVEAL_DURATION_MS, time)) : undefined;
}

export function RibbonComparison({ paused, speed = 1, seekTime }: Props) {
	const rootRef = useRef<HTMLDivElement>(null);
	const elapsed = useRef(0);
	const layers = useRef<readonly Layer[]>(VARIANTS.map(() => ({ renderer: null, failed: false })));
	useEffect(() => () => {
		layers.current.forEach(({ renderer }) => renderer?.dispose());
		// StrictMode reconnects effects while retaining refs: never reuse a
		// disposed GPU program from the preceding effect lifecycle.
		layers.current = VARIANTS.map(() => ({ renderer: null, failed: false }));
	}, []);

	useEffect(() => {
		const root = rootRef.current;
		if (!root) return;
		const targets = VARIANTS.map((variant) => {
			const svg = root.querySelector<SVGSVGElement>(`#logo-root-${variant.id}`)!;
			return { ...variant, svg, canonical: svg.querySelector<SVGGElement>("[data-canonical]")!,
				surface: svg.querySelector<SVGForeignObjectElement>("foreignObject")!, canvas: svg.querySelector("canvas")! };
		});
		const selected = seekTime ?? captureTime();
		if (selected !== undefined) elapsed.current = selected;
		let frame: number | null = null;
		let previous: number | null = null;
		let visible = typeof IntersectionObserver === "undefined";
		let preferenceCheck: number | undefined;
		const update = (index: number, change: Partial<Layer>) => {
			layers.current = layers.current.map((layer, i) => i === index ? { ...layer, ...change } : layer);
		};
		const initialize = (index: number) => {
			const current = layers.current[index];
			if (current.renderer || current.failed) return current.renderer;
			const target = targets[index];
			// Keep a real-sized, unpainted surface so the first active frame
			// doesn't have to reallocate a1px WebGL framebuffer.
			target.surface.style.display = "inline";
			target.surface.style.visibility = "hidden";
			const renderer = target.create(target.canvas, getComputedStyle(target.svg).color, () => {
				update(index, { failed: true });
				target.canonical.style.display = "inline";
				target.canonical.style.opacity = "1";
				target.surface.style.display = "none";
				target.svg.dataset.renderer = "canonical-svg";
			});
			update(index, { renderer, failed: !renderer });
			return renderer;
		};
		const draw = () => {
			targets.forEach((target, index) => {
				let moving = !layers.current[index].failed && elapsed.current > target.from && elapsed.current < target.until;
				if (moving && !initialize(index)) moving = false;
				target.canonical.style.display = moving ? "none" : "inline";
				target.canonical.style.opacity = moving ? "0" : "1";
				const warming = !!layers.current[index].renderer && !layers.current[index].failed && elapsed.current <= target.from;
				target.surface.style.display = moving || warming ? "inline" : "none";
				target.surface.style.visibility = moving ? "visible" : "hidden";
				target.surface.style.opacity = "1";
				if (moving) layers.current[index].renderer?.draw(elapsed.current);
				target.svg.dataset.time = String(Math.round(elapsed.current));
				target.svg.dataset.renderer = moving ? "depth-ribbon" : "canonical-svg";
			});
			root.dataset.time = String(Math.round(elapsed.current));
		};
		const stop = () => {
			if (frame !== null) cancelAnimationFrame(frame);
			frame = null; previous = null;
			if (elapsed.current >= REVEAL_DURATION_MS || layers.current.every((layer) => layer.failed)) {
				window.clearInterval(preferenceCheck); preferenceCheck = undefined;
			}
		};
		const tick = (timestamp: number) => {
			frame = null;
			if (previous !== null) elapsed.current = Math.min(REVEAL_DURATION_MS, elapsed.current + Math.max(0, timestamp - previous) * speed);
			previous = timestamp;
			draw();
			if (elapsed.current < REVEAL_DURATION_MS && !layers.current.every((layer) => layer.failed)) frame = requestAnimationFrame(tick);
			else stop();
		};
		const sync = () => {
			const reduced = getComputedStyle(targets[0].svg).getPropertyValue("--flag-motion").trim() === "reduce";
			if (reduced) elapsed.current = REVEAL_DURATION_MS;
			if (!reduced && !paused && selected === undefined && visible && !document.hidden && elapsed.current === 0) {
				targets.forEach((_, i) => initialize(i)?.draw(0));
			}
			draw();
			if (reduced || paused || selected !== undefined || !visible || document.hidden || elapsed.current >= REVEAL_DURATION_MS
				|| layers.current.every((layer) => layer.failed)) stop();
			else if (frame === null) frame = requestAnimationFrame(tick);
		};
		const observer = typeof IntersectionObserver === "undefined" ? null
			: new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; sync(); });
		observer?.observe(root);
		document.addEventListener("visibilitychange", sync);
		preferenceCheck = window.setInterval(sync, 400);
		sync();
		(window as Window & { __p2mReady?: boolean }).__p2mReady = true;
		return () => { stop(); window.clearInterval(preferenceCheck); observer?.disconnect(); document.removeEventListener("visibilitychange", sync); };
	}, [paused, speed, seekTime]);

	return <div id="logo-root" className={styles.comparison} ref={rootRef}>
		{VARIANTS.map(({ id, label, detail }) => <article className={styles.variant} key={id}>
			<div className={`${styles.stage} ${styles.comparisonStage}`}>
				<span className={styles.studyNumber}>{label}</span>
				<svg id={`logo-root-${id}`} role="img" aria-label={label} className={styles.mark}
					data-variant={id} data-motion-mark="comparison" viewBox="20 26 82 68" fill="currentColor">
					<g className={styles.animatedMark}>
						<g data-canonical="">{REST_PATHS.map((d, index) => <path key={index} d={d} data-reveal-contour={index} id={`logo-${id}-${index}`} />)}</g>
						<foreignObject x="20" y="26" width="82" height="68" style={{ display: "none" }}>
							<canvas className={styles.ribbonCanvas} aria-hidden="true" />
						</foreignObject>
					</g>
					<g className={styles.staticMark}>{REST_PATHS.map((d, i) => <path d={d} key={i} />)}</g>
				</svg>
				<span className={styles.stageCaption}>2,5 s · samme klokke</span>
			</div>
			<p className={styles.variantDetail}>{detail}</p>
		</article>)}
	</div>;
}
