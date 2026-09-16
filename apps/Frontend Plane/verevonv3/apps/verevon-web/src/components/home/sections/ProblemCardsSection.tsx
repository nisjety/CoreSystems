"use client";

import { useState, type PointerEvent } from "react";
import Link from "next/link";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import {
	KnowledgeCardVisual,
	type KnowledgeCardVisual as KnowledgeCardVisualType,
} from "@/components/home/sections/EndToEndKnowledgeVisual";
import { Reveal } from "@/components/home/sections/Reveal";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { cn } from "@/lib/utils";
import styles from "./KnowledgeCards.module.css";

type KnowledgeCard = {
	body: string;
	href: string;
	label: string;
	number: string;
	title: string;
	type: KnowledgeCardVisualType;
};

const cards: KnowledgeCard[] = [
	{
		number: "01",
		title: "Koble til systemene dere bruker.",
		body: "Samle dokumenter, data og verktøy i én arbeidsflate. Verevon kobler sammen kunnskapen der den allerede finnes.",
		href: "/plattform/felles-kontekst",
		label: "Se tilkoblingene",
		type: "connect",
	},
	{
		number: "02",
		title: "Se sammenhengen.",
		body: "Knytt kunnskap på tvers av avdelinger. Bygg agenter og arbeidsflyter med hele bedriften som grunnlag.",
		href: "/produkt/arbeidsflyten",
		label: "Utforsk kunnskapen",
		type: "knowledge",
	},
	{
		number: "03",
		title: "Verevon. Deres virksomhet.",
		body: "En AI-assistent med bedriftens kunnskap som grunnlag. Still spørsmål, lag utkast og sett arbeidet i gang.",
		href: "/plattform/kontrollert-arbeid",
		label: "Se plattformen",
		type: "business",
	},
];

function KnowledgeNavigation({
	active,
	onPointerEnter,
	onFocus,
}: {
	active: string | null;
	onPointerEnter: (event: PointerEvent<HTMLElement>, number: string) => void;
	onFocus: (number: string) => void;
}) {
	const activeIndex = cards.findIndex((card) => card.number === active);
	const [selection, setSelection] = useState<{
		active: number;
		previous: number | null;
		forward: boolean;
	}>({ active: activeIndex, previous: null, forward: true });
	const shouldReduceMotion = useReducedMotion();

	// Remember only the outgoing line; the cards own the active selection.
	if (selection.active !== activeIndex) {
		setSelection({
			active: activeIndex,
			previous: selection.active < 0 ? null : selection.active,
			forward: activeIndex < 0 ? selection.forward : activeIndex > selection.active,
		});
	}

	return (
		<nav aria-label="Utforsk Verevon" className={styles.sectionNavigation}>
			<ul>
				{cards.map((card, index) => (
					<li key={card.number}>
						<Link
							href={card.href}
							data-highlighted={selection.active === index}
							onPointerEnter={(event) => onPointerEnter(event, card.number)}
							onFocus={() => onFocus(card.number)}
						>
							{card.title}
							<AnimatePresence initial={false}>
								{selection.active === index ? (
									<motion.span
										aria-hidden="true"
										className={styles.navigationLine}
										style={selection.forward ? { left: 0 } : { right: 0 }}
										initial={{ width: shouldReduceMotion ? "100%" : "0%" }}
										animate={{ width: "100%" }}
										transition={{ duration: shouldReduceMotion ? 0 : 0.24, ease: [0.1, 1, 0.7, 1] }}
										key={`incoming-${index}`}
									/>
								) : null}
								{selection.previous === index ? (
									<motion.span
										aria-hidden="true"
										className={styles.navigationLine}
										style={selection.forward ? { right: 0 } : { left: 0 }}
										initial={{ width: "100%" }}
										animate={{ width: "0%" }}
										transition={{ duration: shouldReduceMotion ? 0 : 0.3, ease: [0.1, 1, 0.7, 1] }}
										key={`outgoing-${index}-${selection.active}`}
										onAnimationComplete={() => {
											setSelection((current) => current === selection ? { ...current, previous: null } : current);
										}}
									/>
								) : null}
							</AnimatePresence>
						</Link>
					</li>
				))}
			</ul>
		</nav>
	);
}

export function ProblemCardsSection() {
	const [hovered, setHovered] = useState<string | null>(null);
	const [opened, setOpened] = useState<string | null>(null);
	const active = hovered ?? opened;

	function previewCard(event: PointerEvent<HTMLElement>, number: string) {
		if (event.pointerType === "mouse" && window.matchMedia("(min-width: 1024px) and (hover: hover)").matches) {
			setHovered(number);
		}
	}

	return (
		<section
			aria-labelledby="end-to-end-knowledge-title"
			className="relative isolate bg-background px-[var(--verevon-edge)] py-[clamp(58px,7.2vw,115px)] text-verevon-j-text max-[760px]:px-[var(--verevon-page-pad)]"
			data-problem-cards=""
			id="problemomrader"
		>
			<Reveal delay={90}>
				<header className="mx-auto max-w-[770px] text-center max-[760px]:text-left">
					<p className="font-protokoll text-[0.7rem] font-medium uppercase tracking-[0.16em] text-verevon-text-muted">
						02 / VEREVON
					</p>
					<h2
						className="mt-4 font-arbeit text-[clamp(2rem,3.2vw,3.5rem)] font-light leading-[0.98] tracking-[-0.055em] text-pretty"
						id="end-to-end-knowledge-title"
					>
						<span className="block">Samle bedriftens kunnskap,</span>{" "}
						<span className="block">ende til ende.</span>
					</h2>
					<p className="verevon-body mx-auto mt-5 max-w-[58ch] text-verevon-text-muted text-pretty max-[760px]:mx-0">
						Koble sammen kunnskapen, finn sammenhengene og få arbeidet gjort
						— med kontroll hele veien.
					</p>
				</header>
			</Reveal>

			<Reveal delay={170}>
				<div
					onPointerLeave={() => setHovered(null)}
					onBlur={(event) => {
						if (!event.currentTarget.contains(event.relatedTarget)) setOpened(null);
					}}
					onKeyDown={(event) => {
						if (event.key === "Escape") { setHovered(null); setOpened(null); }
						if (event.key === "Tab") setHovered(null);
					}}
				>
					<div className={styles.cards} data-active={active ?? "none"}>
						{cards.map((card) => {
							const isBusiness = card.type === "business";
							const expanded = active === card.number;
							return (
								<article
									aria-labelledby={`knowledge-card-${card.number}`}
									className={cn(styles.card, styles[card.type])}
									data-expanded={expanded}
									data-muted={active !== null && !expanded}
									key={card.number}
									onPointerEnter={(event) => previewCard(event, card.number)}
								>
									<span className={styles.number}>{card.number}</span>
									<button
										aria-controls={`knowledge-visual-${card.number}`}
										aria-expanded={expanded}
										aria-label={`${expanded ? "Lukk" : "Utvid"} kort ${card.number}`}
										className={styles.expand}
										type="button"
										onClick={() => {
											setHovered(null);
											setOpened(expanded ? null : card.number);
										}}
									>
										<svg aria-hidden="true" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.2"><path d={expanded ? "M3 8h5V3M17 12h-5v5" : "M3 8V3h5M17 12v5h-5"} /></svg>
									</button>
									{isBusiness ? (
										<div id={`knowledge-visual-${card.number}`}><KnowledgeCardVisual expanded={expanded} type={card.type} /></div>
									) : (
										<div className={styles.visual} id={`knowledge-visual-${card.number}`}>
											<KnowledgeCardVisual expanded={expanded} type={card.type} />
										</div>
									)}
									<div className={styles.content}>
										<h3 id={`knowledge-card-${card.number}`}>{card.title}</h3>
										<p>{card.body}</p>
										<ArrowButton className={styles.cta} href={card.href} variant={isBusiness ? "light" : "dark"}>
											{card.label}
										</ArrowButton>
									</div>
								</article>
							);
						})}
					</div>
					<KnowledgeNavigation
						active={active}
						onPointerEnter={previewCard}
						onFocus={(number) => {
							setHovered(null);
							setOpened(number);
						}}
					/>
				</div>
			</Reveal>
		</section>
	);
}

export default ProblemCardsSection;
