"use client";

import * as React from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { Section } from "./section";
import { SensesGuidelines } from "./SensesGuidelines";
import { SensesMotion } from "../senses/SensesMotion";
import type { SenseKind } from "../senses/SenseScene";
import styles from "../senses/SensesSection.module.css";

gsap.registerPlugin(ScrollTrigger);

// One "chapter" of the editorial scroll story below.
type SenseChapter = {
	kind: SenseKind;
	id: "search" | "trace" | "operate";
	subtitle: string;
	text: string;
	highlight: string;
	titleLines: string[];
};

// Each chapter pairs an outcome with a complete, illustrated workflow.
export const chapters: SenseChapter[] = [
	{
		kind: "delegate",
		id: "search",
		subtitle: "Fremdrift",
		titleLines: ["La arbeidet", "gå videre."],
		text: "Når noe krever oppfølging, samler Verevon relevant informasjon og foreslår veien videre. Arbeid som følger reglene deres kan gå videre. Når det trengs en vurdering, går saken til riktig person.",
		highlight: "går saken til riktig person",
	},
	{
		kind: "learn",
		id: "trace",
		subtitle: "Felles grunnlag",
		titleLines: ["Kunnskapen", "blir felles."],
		text: "Verevon knytter dokumenter, tidligere vurderinger og måten dere arbeider på til nye saker. Hver sak starter med konteksten virksomheten allerede har, uten at dere må forklare alt på nytt.",
		highlight: "konteksten virksomheten allerede har",
	},
	{
		kind: "oversee",
		id: "operate",
		subtitle: "Kontroll",
		titleLines: ["Behold oversikten.", "Grip inn ved behov."],
		text: "Følg hva AI bruker som grunnlag, hva som skjer videre og hvem som har ansvar. Når noe avviker eller krever en beslutning, får riktig person saken med kildene og alternativene som trengs.",
		highlight: "riktig person",
	},
];

// Wraps the `highlight` substring of `text` in a coral <span>, if present.
function renderHighlightText(text: string, highlight: string) {
	const parts = text.split(highlight);

	if (parts.length === 1) {
		return text;
	}

	return (
		<>
			{parts.map((part, index) => (
				<React.Fragment key={`${highlight}-${index}`}>
					{part}
					{index < parts.length - 1 ? (
						<span className="font-normal text-[color-mix(in_srgb,var(--verevon-coral)_76%,var(--verevon-j-text))]">
							{highlight}
						</span>
					) : null}
				</React.Fragment>
			))}
		</>
	);
}

export function SensesSection() {
	const containerRef = React.useRef<HTMLDivElement>(null);

	React.useLayoutEffect(() => {
		const container = containerRef.current;
		if (!container) return;
		const items = gsap.utils.toArray<HTMLElement>("[data-sense-chapter]", container);
		const mm = gsap.matchMedia();
		mm.add("(min-width: 1024px)", () => {
			// The site-level full-motion setting is deliberate. Honor a system
			// reduced-motion preference only when that explicit setting is absent.
			if (
				document.documentElement.dataset.motion !== "full" &&
				window.matchMedia("(prefers-reduced-motion: reduce)").matches
			) {
				return;
			}

			container.dataset.sensesScroll = "active";
			items.forEach((item, index) => {
				const copy = item.querySelector<HTMLElement>("[data-sense-copy]");
				// Restore the original scroll ranges and 25% / 50% / 25% copy rhythm.
				// The clipping cell confines each fixed text block to its own chapter.
				if (copy) {
					const timeline = gsap.timeline({ scrollTrigger: {
						trigger: item, start: "top bottom", end: "bottom top", scrub: true,
						id: `verevon-senses-copy-${index + 1}`, invalidateOnRefresh: true,
					} });
					if (index === 0) {
						timeline.set(copy, { autoAlpha: 1, y: 0 }, 0);
					} else {
						timeline.fromTo(copy, { autoAlpha: 0, y: 50 }, { autoAlpha: 1, y: 0, ease: "none", force3D: true, duration: .25 }, 0);
					}
					timeline.to(copy, { autoAlpha: 0, y: -50, ease: "none", force3D: true, duration: .25 }, .75);
				}
			});
			return () => { delete container.dataset.sensesScroll; };
		}, container);

		// Recalculate after fonts and upstream pinned sections settle, in page order.
		let disposed = false;
		let settleTimer: ReturnType<typeof setTimeout> | undefined;
		const refresh = () => { if (!disposed) ScrollTrigger.refresh(); };
		const refreshOnceSettled = () => {
			refresh();
			settleTimer = setTimeout(refresh, 500);
		};
		document.fonts?.ready.then(refresh);
		const frame = requestAnimationFrame(refresh);
		if (document.readyState === "complete") refreshOnceSettled();
		else window.addEventListener("load", refreshOnceSettled, { once: true });
		return () => {
			disposed = true;
			cancelAnimationFrame(frame);
			clearTimeout(settleTimer);
			window.removeEventListener("load", refreshOnceSettled);
			mm.revert();
		};
	}, []);

	return (
		<Section containerClassName="p-0" id="kunnskap" title="Når helheten er synlig, kan arbeidet gå videre." titleClassName="sr-only" variant="full-bleed-tight">
			<div className={styles.section} ref={containerRef}>
				<header className={styles.header}>
					<h2 className="verevon-home-heading max-w-[26ch] text-verevon-j-text text-balance"><span className="block">Når helheten er synlig.</span>{" "}<span className="block">Kan arbeidet gå videre.</span></h2>
					<p className={styles.intro}>Verevon gir virksomheten et felles grunnlag for å forstå, vurdere og følge opp arbeidet. AI bidrar innenfor rammene dere bestemmer.</p>
				</header>
				<ol className={styles.chapters}>
					{chapters.map((chapter, index) => (
						<li className={styles.chapter} key={chapter.id} data-sense-chapter={chapter.id}>
							<div className={styles.copyClip}>
								<div className={styles.copy} data-sense-copy>
									<p className={styles.eyebrow}>Kapittel 0{index + 1} / {chapter.subtitle}</p>
									<h3 className={`verevon-home-heading ${styles.title}`}>{chapter.titleLines.map((part, lineIndex) => <React.Fragment key={part}>{lineIndex > 0 ? " " : null}<span>{part}</span></React.Fragment>)}</h3>
									<p className={styles.body}>{renderHighlightText(chapter.text, chapter.highlight)}</p>
									{index === 0 ? <ArrowButton className={styles.link} href="/produkt/arbeidsflyten">Se arbeidsflaten i bruk</ArrowButton> : null}
								</div>
							</div>
							<div className={styles.media}>
								<SensesMotion kind={chapter.kind} />
								<SensesGuidelines className={styles.guideline} delaySeconds={index * .15} orientation="vertical" />
							</div>
						</li>
					))}
				</ol>
				<div className={styles.closing}><p>Felles grunnlag. Styrt arbeid.</p></div>
			</div>
		</Section>
	);
}

export default SensesSection;
